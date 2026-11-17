//! Secrets live in the macOS Keychain, not in `app.db`: one generic password per secret, all
//! under one service. `APPD_KEYCHAIN=<file>` swaps the login keychain for a keychain file of
//! its own, through the same Security.framework calls; the tests use it so they never touch the
//! user's.

use security_framework::base::Error;
use security_framework::os::macos::keychain::{CreateOptions, SecKeychain};

pub type Result<T, E = Error> = std::result::Result<T, E>;

const SERVICE: &str = "com.oursnever.bate";
/// Protects nothing: a keychain file made through `APPD_KEYCHAIN` holds test data only.
const FILE_PASSWORD: &str = "bate-test";
const ITEM_NOT_FOUND: i32 = -25300;
const DUPLICATE_KEYCHAIN: i32 = -25296;
/// What a delete that did not remove the item reports.
const NO_ACCESS_FOR_ITEM: i32 = -25243;

fn open() -> Result<SecKeychain> {
    let Some(file) = std::env::var_os("APPD_KEYCHAIN") else {
        return SecKeychain::default();
    };
    match CreateOptions::new().password(FILE_PASSWORD).create(&file) {
        Err(e) if e.code() == DUPLICATE_KEYCHAIN => {
            let mut keychain = SecKeychain::open(&file)?;
            keychain.unlock(Some(FILE_PASSWORD))?;
            Ok(keychain)
        }
        made => made,
    }
}

/// The secret kept under `account`, if there is one.
pub fn get(account: &str) -> Result<Option<String>> {
    match open()?.find_generic_password(SERVICE, account) {
        Ok((secret, _)) => Ok(Some(String::from_utf8_lossy(&secret).into_owned())),
        Err(e) if e.code() == ITEM_NOT_FOUND => Ok(None),
        Err(e) => Err(e),
    }
}

/// Keeps `secret` under `account`; an empty one forgets it.
pub fn put(account: &str, secret: &str) -> Result<()> {
    if secret.is_empty() {
        return delete(account);
    }
    open()?.set_generic_password(SERVICE, account, secret.as_bytes())
}

/// Forgets `account`; fine if there is none.
pub fn delete(account: &str) -> Result<()> {
    match open()?.find_generic_password(SERVICE, account) {
        // `delete` reports nothing: look again.
        Ok((_, item)) => item.delete(),
        Err(e) if e.code() == ITEM_NOT_FOUND => return Ok(()),
        Err(e) => return Err(e),
    }
    match get(account)? {
        None => Ok(()),
        Some(_) => Err(Error::from_code(NO_ACCESS_FOR_ITEM)),
    }
}
