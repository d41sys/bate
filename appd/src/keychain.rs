//! Secrets live in a macOS keychain, not in `app.db`: one generic password per secret, all
//! under one service: the launchd label, which is made per home, so two services with different
//! homes never read, overwrite or delete each other's.
//!
//! A build with a Team ID (Developer ID) uses the login keychain. macOS knows any other build (ad
//! hoc, the self-made `Bate Dev`) there by its code hash, new at every build, and asks about each
//! secret again after each one. Such a build keeps `secrets.keychain` in its home instead: macOS
//! makes a keychain file outside `~/Library/Keychains` without that check. `APPD_KEYCHAIN=<file>`
//! names another file; the tests use it so they never touch the user's.

use security_framework::base::Error;
use security_framework::os::macos::keychain::{CreateOptions, SecKeychain};
use security_framework::os::macos::keychain_item::SecKeychainItem;
use std::path::PathBuf;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Protects nothing: a keychain file of Bate's own is guarded as `app.db` is, by the mode of the
/// home (0700), not by the login password.
const FILE_PASSWORD: &str = "bate-test";
const ITEM_NOT_FOUND: i32 = -25300;
const DUPLICATE_KEYCHAIN: i32 = -25296;
/// What a delete that did not remove the item reports.
const NO_ACCESS_FOR_ITEM: i32 = -25243;

/// The keychain in use and, when that is the file in the home, the login keychain: an earlier
/// version kept the secrets there.
fn open() -> Result<(SecKeychain, Option<SecKeychain>)> {
    let (file, before) = match std::env::var_os("APPD_KEYCHAIN") {
        Some(file) => (PathBuf::from(file), None),
        None if proto::signer::has_team() => return Ok((SecKeychain::default()?, None)),
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

fn find(keychain: &SecKeychain, account: &str) -> Result<Option<(String, SecKeychainItem)>> {
    match keychain.find_generic_password(&crate::launch::label(), account) {
        Ok((secret, item)) => Ok(Some((String::from_utf8_lossy(&secret).into_owned(), item))),
        Err(e) if e.code() == ITEM_NOT_FOUND => Ok(None),
        Err(e) => Err(e),
    }
}

/// The secret `own` has; one only `before` has is moved to `own` first, so macOS asks about it
/// one last time.
fn read(own: &SecKeychain, before: Option<&SecKeychain>, account: &str) -> Result<Option<String>> {
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
fn remove(own: &SecKeychain, before: Option<&SecKeychain>, account: &str) -> Result<()> {
    for keychain in std::iter::once(own).chain(before) {
        if let Some((_, item)) = find(keychain, account)? {
            // `delete` reports nothing: look again.
            item.delete();
            if find(keychain, account)?.is_some() {
                return Err(Error::from_code(NO_ACCESS_FOR_ITEM));
            }
        }
    }
    Ok(())
}

/// The secret kept under `account`, if there is one.
pub fn get(account: &str) -> Result<Option<String>> {
    let (own, before) = open()?;
    read(&own, before.as_ref(), account)
}

/// Keeps `secret` under `account`; an empty one forgets it.
pub fn put(account: &str, secret: &str) -> Result<()> {
    if secret.is_empty() {
        return delete(account);
    }
    open()?
        .0
        .set_generic_password(&crate::launch::label(), account, secret.as_bytes())
}

/// Forgets `account`; fine if there is none.
pub fn delete(account: &str) -> Result<()> {
    let (own, before) = open()?;
    remove(&own, before.as_ref(), account)
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
