//! Undo: the earlier content of a file that Bate's own engine changes with `write` or `edit`,
//! kept by the service. The rules are in `PROTOCOL.md`.

/// Most bytes of one file that are kept: a file over it is changed without a copy.
pub const MAX_FILE: u64 = 1 << 20;

/// Most bytes kept for one bot (or one project's notes): each copy counts its bytes and
/// `ENTRY` more. The oldest are dropped first.
pub const MAX_KEPT: u64 = 50 << 20;

/// What a copy counts besides its bytes, so a bot that creates many small files cannot make
/// the store grow without end.
pub const ENTRY: u64 = 512;

/// SHA-256 of `bytes` as 64 lower-case hex digits: how the engine says what a change left,
/// and how the service sees that the file is still so.
pub fn digest(bytes: &[u8]) -> String {
    hex(ring::digest::digest(&ring::digest::SHA256, bytes).as_ref())
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_is_sha256_in_hex() {
        assert_eq!(
            digest(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
