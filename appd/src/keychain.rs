//! Secrets live in a macOS keychain, not in `app.db`: one generic password per secret, all
//! under one service: the launchd label, which is made per home, so two services with different
//! homes never read, overwrite or delete each other's.
//!
//! A build with a Team ID (Developer ID) uses the login keychain. macOS knows any other build
//! there by its code hash, new at every build, and asks about each secret again after each one.
//! A build signed with a certificate that has no team (the self-made `Bate Dev`) keeps
//! `secrets.keychain` in its home instead: macOS makes a keychain file outside
//! `~/Library/Keychains` without that check, so the signer alone decides. An ad-hoc build has no
//! signer to go by and stays with the login keychain. `APPD_KEYCHAIN=<file>` names another
//! file; the tests use it so they never touch the user's.
//!
//! A question macOS asks (may this program use that secret?) stops the one that asks until the
//! person answers. The service therefore turns the questions off for itself ([`serve`]): what
//! would have asked fails at once, and a read is asked for again in a short-lived child of the
//! same program (`appd keychain ask <account>`, see [`ask`]), where the person sees the
//! question. The service goes on meanwhile ([`Error::Waiting`]).

use crate::sys::log;
use security_framework::base;
use security_framework::os::macos::keychain::{CreateOptions, SecKeychain};
use security_framework::os::macos::keychain_item::SecKeychainItem;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// What `get`, `put` and `delete` fail with.
#[derive(Debug)]
pub enum Error {
    /// macOS reported this.
    Os(base::Error),
    /// macOS would ask whether Bate may use the secret, and is being asked in a child.
    Waiting,
    /// The person said no (or the child failed), or said yes and macOS still asks. Not asked
    /// again by itself: `ask_again` does.
    Refused,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Os(e) => write!(f, "keychain: {e}"),
            Error::Waiting => f.write_str(
                "macOS is asking whether Bate may read its secrets. Answer it, then try again.",
            ),
            Error::Refused => f.write_str("macOS did not let Bate read its secrets."),
        }
    }
}

impl std::error::Error for Error {}

impl From<base::Error> for Error {
    fn from(e: base::Error) -> Self {
        Error::Os(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Protects nothing: a keychain file of Bate's own is guarded as `app.db` is, by the mode of the
/// home (0700), not by the login password.
const FILE_PASSWORD: &str = "bate-test";
const ITEM_NOT_FOUND: i32 = -25300;
/// What a read, write or delete that macOS would have asked about reports, questions being off.
const INTERACTION_NOT_ALLOWED: i32 = -25308;
const DUPLICATE_KEYCHAIN: i32 = -25296;
/// What a delete that did not remove the item reports.
const NO_ACCESS_FOR_ITEM: i32 = -25243;

/// The keychain in use and, when that is the file in the home, the login keychain: an earlier
/// version kept the secrets there.
fn open() -> base::Result<(SecKeychain, Option<SecKeychain>)> {
    let (file, before) = match std::env::var_os("APPD_KEYCHAIN") {
        Some(file) => (PathBuf::from(file), None),
        // Ad hoc: a new program to the file's items at every build too, and to allow it macOS
        // would want the file's password, where the login keychain wants the one the user knows.
        None if proto::signer::has_team() || proto::signer::own_requirement().is_none() => {
            return Ok((SecKeychain::default()?, None));
        }
        None => (
            crate::paths::home().join("secrets.keychain"),
            Some(SecKeychain::default()?),
        ),
    };
    let own = match CreateOptions::new().password(FILE_PASSWORD).create(&file) {
        Err(e) if e.code() == DUPLICATE_KEYCHAIN => {
            let mut keychain = SecKeychain::open(&file)?;
            keychain.unlock(Some(FILE_PASSWORD))?;
            keychain
        }
        made => made?,
    };
    Ok((own, before))
}

fn find(keychain: &SecKeychain, account: &str) -> base::Result<Option<(String, SecKeychainItem)>> {
    match keychain.find_generic_password(&crate::launch::label(), account) {
        Ok((secret, item)) => Ok(Some((String::from_utf8_lossy(&secret).into_owned(), item))),
        Err(e) if e.code() == ITEM_NOT_FOUND => Ok(None),
        Err(e) => Err(e),
    }
}

/// The secret `own` has; one only `before` has is moved to `own` first, so macOS asks about it
/// one last time.
fn read(
    own: &SecKeychain,
    before: Option<&SecKeychain>,
    account: &str,
) -> base::Result<Option<String>> {
    if let Some((secret, _)) = find(own, account)? {
        return Ok(Some(secret));
    }
    let Some((secret, item)) = before.map(|b| find(b, account)).transpose()?.flatten() else {
        return Ok(None);
    };
    own.set_generic_password(&crate::launch::label(), account, secret.as_bytes())?;
    item.delete();
    Ok(Some(secret))
}

/// Removes `account` from each keychain: one left in `before` would come back at the next read.
fn remove(own: &SecKeychain, before: Option<&SecKeychain>, account: &str) -> base::Result<()> {
    for keychain in std::iter::once(own).chain(before) {
        if let Some((_, item)) = find(keychain, account)? {
            // `delete` reports nothing: look again.
            item.delete();
            if find(keychain, account)?.is_some() {
                return Err(base::Error::from_code(NO_ACCESS_FOR_ITEM));
            }
        }
    }
    Ok(())
}

/// What the child of an account is at.
enum Ask {
    /// A child is asking.
    Asking,
    /// The child ended well; the service has not read the secret since.
    Tried,
    Refused,
}

static ASKS: Mutex<BTreeMap<String, Ask>> = Mutex::new(BTreeMap::new());
/// Called, from the thread that waits for a child, when one has ended; set by [`serve`].
static ENDED: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

fn asks() -> std::sync::MutexGuard<'static, BTreeMap<String, Ask>> {
    ASKS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Called once by the service: from now on a read that macOS would ask about fails at once, and
/// is asked in a child; `ended` is called whenever one ends.
pub fn serve(ended: impl Fn() + Send + Sync + 'static) {
    match SecKeychain::disable_user_interaction() {
        // Dropping the guard would turn the questions on again.
        Ok(guard) => std::mem::forget(guard),
        Err(e) => log!("the Keychain's questions are not off: {e}"),
    }
    let _ = ENDED.set(Box::new(ended));
}

/// Whether some secret was refused to the service, and `ask_again` has something to do.
pub fn refused() -> bool {
    asks().values().any(|a| matches!(a, Ask::Refused))
}

/// Asks, in a child, about every secret that was refused.
pub fn ask_again() {
    let mut asks = asks();
    for (account, ask) in asks.iter_mut() {
        if matches!(ask, Ask::Refused) {
            *ask = Ask::Asking;
            ask_in_child(account.clone());
        }
    }
}

fn ask_in_child(account: String) {
    std::thread::spawn(move || {
        let ok = std::env::current_exe()
            .and_then(|me| {
                std::process::Command::new(me)
                    .args(["keychain", "ask", &account])
                    .stdin(std::process::Stdio::null())
                    .status()
            })
            .is_ok_and(|status| status.success());
        asks().insert(account, if ok { Ask::Tried } else { Ask::Refused });
        if let Some(ended) = ENDED.get() {
            ended();
        }
    });
}

/// What the outcome of an operation on `account` means for the caller: a failure that macOS
/// would have asked about starts the child, or says that one is asking or was refused.
fn settle<T>(account: &str, done: base::Result<T>) -> Result<T> {
    match done {
        Ok(v) => {
            asks().remove(account);
            return Ok(v);
        }
        Err(e) if e.code() == INTERACTION_NOT_ALLOWED && ENDED.get().is_some() => {}
        Err(e) => return Err(e.into()),
    }
    let mut asks = asks();
    match asks.get(account) {
        Some(Ask::Asking) => Err(Error::Waiting),
        Some(Ask::Refused) => Err(Error::Refused),
        // The child was told yes, and the service is asked still: asking again would only loop.
        Some(Ask::Tried) => {
            asks.insert(account.to_owned(), Ask::Refused);
            Err(Error::Refused)
        }
        None => {
            asks.insert(account.to_owned(), Ask::Asking);
            ask_in_child(account.to_owned());
            Err(Error::Waiting)
        }
    }
}

/// The secret kept under `account`, if there is one. A secret macOS would ask about is asked
/// for in a child, and this fails with `Waiting` meanwhile: never `None`.
pub fn get(account: &str) -> Result<Option<String>> {
    settle(account, hook(account).and_then(|()| read_account(account)))
}

fn read_account(account: &str) -> base::Result<Option<String>> {
    let (own, before) = open()?;
    read(&own, before.as_ref(), account)
}

/// Keeps `secret` under `account`; an empty one forgets it, as `delete` does. A value is never
/// asked about: the caller keeps it in `app.db` when this fails.
pub fn put(account: &str, secret: &str) -> Result<()> {
    if secret.is_empty() {
        return delete(account);
    }
    hook(account)?;
    open()?
        .0
        .set_generic_password(&crate::launch::label(), account, secret.as_bytes())?;
    Ok(())
}

/// Forgets `account`; fine if there is none.
pub fn delete(account: &str) -> Result<()> {
    settle(account, remove_account(account))
}

fn remove_account(account: &str) -> base::Result<()> {
    hook(account)?;
    let (own, before) = open()?;
    remove(&own, before.as_ref(), account)
}

/// `appd keychain ask <account>`: reads the secret with questions on, so that the person is
/// asked here and the answer holds for the service, and moves it to the file where that is due.
/// Prints nothing of it.
pub fn ask(account: &str) -> base::Result<()> {
    #[cfg(debug_assertions)]
    person(account)?;
    read_account(account).map(drop)
}

/// For tests, in debug builds only: `APPD_KEYCHAIN_ASK=<folder>` makes an account whose file in
/// the folder is missing behave as if macOS would ask. The child stands in for the person: it
/// notes itself in `<account>.started`, then waits for `<account>` (yes) or `<account>.no`.
#[cfg(debug_assertions)]
fn marker(account: &str, suffix: &str) -> Option<PathBuf> {
    let dir = std::env::var_os("APPD_KEYCHAIN_ASK")?;
    Some(PathBuf::from(dir).join(format!("{}{suffix}", account.replace('/', "_"))))
}

#[cfg(debug_assertions)]
fn hook(account: &str) -> base::Result<()> {
    match marker(account, "") {
        Some(yes) if !yes.exists() => Err(base::Error::from_code(INTERACTION_NOT_ALLOWED)),
        _ => Ok(()),
    }
}

#[cfg(not(debug_assertions))]
fn hook(_: &str) -> base::Result<()> {
    Ok(())
}

#[cfg(debug_assertions)]
fn person(account: &str) -> base::Result<()> {
    use std::io::Write;
    let (Some(yes), Some(no), Some(started)) = (
        marker(account, ""),
        marker(account, ".no"),
        marker(account, ".started"),
    ) else {
        return Ok(());
    };
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(started)
    {
        let _ = writeln!(f, "asked");
    }
    while !yes.exists() {
        if no.exists() {
            return Err(base::Error::from_code(-128));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(dir: &std::path::Path, name: &str) -> SecKeychain {
        CreateOptions::new()
            .password(FILE_PASSWORD)
            .create(dir.join(name))
            .unwrap()
    }

    #[test]
    fn a_secret_of_an_earlier_version_is_moved_once_and_a_removed_one_stays_removed() {
        let dir = std::env::temp_dir().join(format!("appd-keychain-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (own, before) = (file(&dir, "own"), file(&dir, "before"));
        let label = crate::launch::label();
        before
            .set_generic_password(&label, "moved", b"old")
            .unwrap();
        before
            .set_generic_password(&label, "stale", b"old")
            .unwrap();
        own.set_generic_password(&label, "stale", b"new").unwrap();

        let got = read(&own, Some(&before), "moved").unwrap();
        assert_eq!(got.as_deref(), Some("old"));
        assert!(find(&before, "moved").unwrap().is_none(), "left behind");
        assert_eq!(find(&own, "moved").unwrap().unwrap().0, "old");
        assert_eq!(read(&own, Some(&before), "none").unwrap(), None);

        let got = read(&own, Some(&before), "stale").unwrap();
        assert_eq!(got.as_deref(), Some("new"), "the newer one is the file's");
        remove(&own, Some(&before), "stale").unwrap();
        assert_eq!(read(&own, Some(&before), "stale").unwrap(), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
