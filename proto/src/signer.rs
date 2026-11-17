//! Who signed this program, and whether the program on the other end of a socket carries the
//! same signature. The service uses it for the window and the helper app for the service, so a
//! program of the user's that is not Bate cannot drive either.
//!
//! Only a program signed with a certificate (Developer ID, or the self-made `Bate Dev` of
//! `window/sign-dev.sh`) has a signer to compare. An unsigned or ad-hoc build (a dev run, the
//! tests) has none, and nothing is refused for it.

use core_foundation::array::CFArrayGetValueAtIndex;
use core_foundation::base::TCFType;
use core_foundation::data::CFData;
use core_foundation::dictionary::{CFDictionaryGetValue, CFDictionaryRef};
use core_foundation::string::CFStringRef;
use security_framework::certificate::SecCertificate;
use security_framework::os::macos::code_signing::{
    Flags, GuestAttributes, SecCode, SecRequirement,
};
use std::ffi::c_void;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::LazyLock;

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SecCodeCopySigningInformation(
        code: *const c_void,
        flags: u32,
        information: *mut CFDictionaryRef,
    ) -> i32;
    static kSecCodeInfoCertificates: CFStringRef;
}

/// `kSecCSSigningInformation`: the flag that makes the certificates part of the answer.
const SIGNING_INFORMATION: u32 = 1 << 1;
/// `val[5]` of an audit token is the process id.
const PID: usize = 5;

/// A requirement that only code signed with `leaf` (the DER of a certificate) meets.
fn requirement(leaf: &[u8]) -> String {
    let sha1 = ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, leaf);
    let hex: String = sha1.as_ref().iter().map(|b| format!("{b:02x}")).collect();
    format!("certificate leaf = H\"{hex}\"")
}

/// The certificate this program is signed with, as a requirement; `None` when it has none.
/// A debug build takes the certificate's SHA-1 from `$BATE_SIGNER_LEAF` (the tests have no
/// certificate to sign with).
pub fn own_requirement() -> Option<&'static str> {
    static OWN: LazyLock<Option<String>> = LazyLock::new(|| {
        #[cfg(debug_assertions)]
        if let Some(sha1) = std::env::var_os("BATE_SIGNER_LEAF") {
            return Some(format!(
                "certificate leaf = H\"{}\"",
                sha1.to_string_lossy()
            ));
        }
        let me = SecCode::for_self(Flags::NONE).ok()?;
        let mut info: CFDictionaryRef = std::ptr::null();
        // SAFETY: a code object and a valid out-pointer; the docs allow a running code object
        // where a static one is named.
        let status = unsafe {
            SecCodeCopySigningInformation(me.as_CFTypeRef(), SIGNING_INFORMATION, &mut info)
        };
        if status != 0 || info.is_null() {
            return None;
        }
        // SAFETY: `info` is a dictionary we own (a "copy"), so the wrapper releases it; its
        // certificate list, when present, is an array of certificates, the leaf first.
        let der = unsafe {
            let info = core_foundation::dictionary::CFDictionary::<*const c_void, *const c_void>::wrap_under_create_rule(info);
            let list =
                CFDictionaryGetValue(info.as_concrete_TypeRef(), kSecCodeInfoCertificates.cast());
            if list.is_null() {
                return None;
            }
            let leaf = CFArrayGetValueAtIndex(list.cast(), 0);
            SecCertificate::wrap_under_get_rule(leaf.cast_mut().cast()).to_der()
        };
        Some(requirement(&der))
    });
    OWN.as_deref()
}

/// The audit token of the process on the other end of `stream`: it names that process alone,
/// where a process id can be reused.
// shortcut: the token is read when this runs, not per message, so a process of the same user can
// connect, queue frames, then exec a signed program and pass; upgrade to per-message audit tokens (XPC).
fn peer_token(stream: &UnixStream) -> Option<[u32; 8]> {
    let mut token = [0u32; 8];
    let mut len = std::mem::size_of_val(&token) as libc::socklen_t;
    // SAFETY: valid descriptor; the pointers describe `token` and `len`.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERTOKEN,
            token.as_mut_ptr().cast(),
            &mut len,
        )
    };
    (rc == 0).then_some(token)
}

/// Whether the process with this audit token is validly signed and meets `requirement`.
fn meets(token: &[u32; 8], requirement: &str) -> bool {
    let bytes: Vec<u8> = token.iter().flat_map(|w| w.to_ne_bytes()).collect();
    let mut attributes = GuestAttributes::new();
    attributes.set_audit_token(CFData::from_buffer(&bytes).as_concrete_TypeRef());
    let Ok(code) = SecCode::copy_guest_with_attribues(None, &attributes, Flags::NONE) else {
        return false;
    };
    let Ok(requirement) = requirement.parse::<SecRequirement>() else {
        return false;
    };
    code.check_validity(Flags::NONE, &requirement).is_ok()
}

/// Whether the program on the other end of `stream` may be trusted as Bate's own: it is this
/// very process (the LAN relay connects to the service from inside it), or it is validly signed
/// with this program's certificate. True for any peer when this program has no certificate.
pub fn peer_is_same_signer(stream: &UnixStream) -> bool {
    allowed(own_requirement(), stream)
}

fn allowed(requirement: Option<&str>, stream: &UnixStream) -> bool {
    let Some(requirement) = requirement else {
        return true;
    };
    let Some(token) = peer_token(stream) else {
        return false;
    };
    token[PID] == std::process::id() || meets(&token, requirement)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_certificate_becomes_a_requirement_on_its_sha1() {
        // SHA-1 of "abc".
        assert_eq!(
            requirement(b"abc"),
            "certificate leaf = H\"a9993e364706816aba3e25717850c26c9cd0d89d\""
        );
        // The requirement is one the system reads.
        assert!(requirement(b"abc").parse::<SecRequirement>().is_ok());
    }

    #[test]
    fn the_process_itself_is_its_own_peer_and_another_signer_is_not() {
        let (a, b) = UnixStream::pair().unwrap();
        let token = peer_token(&a).unwrap();
        assert_eq!(token[PID], std::process::id());
        drop(b);
        // This test program carries no certificate, so it cannot meet one.
        assert!(!meets(
            &token,
            &requirement(b"not this program's certificate")
        ));
    }

    #[test]
    fn this_process_and_an_unsigned_service_are_never_refused() {
        let (a, _b) = UnixStream::pair().unwrap();
        // The LAN relay: the peer is the service itself, which no certificate decides.
        assert!(allowed(Some(&requirement(b"some certificate")), &a));
        // No certificate to compare.
        assert!(allowed(None, &a));
    }
}
