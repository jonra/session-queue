//! Session names and message IDs.
//!
//! Names must match `^[a-z0-9][a-z0-9-]{0,31}$`, which blocks path tricks
//! such as `../x`. IDs are `<UTC yyyymmddThhmmssZ>-<from>-<4 hex>`.

use crate::error::{CqError, Result};

pub const MAX_NAME_LEN: usize = 32;

/// Length of the `yyyymmddThhmmssZ` prefix of an ID.
const STAMP_LEN: usize = 16;

pub fn is_valid_name(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > MAX_NAME_LEN {
        return false;
    }
    let ok = |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit();
    ok(b[0]) && b[1..].iter().all(|&c| ok(c) || c == b'-')
}

/// Refuse a name that does not match the name pattern. `what` names the
/// flag or field in the error message.
pub fn check_name(what: &str, s: &str) -> Result<()> {
    if is_valid_name(s) {
        Ok(())
    } else {
        Err(CqError::refused(format!(
            "invalid {what} {s:?}: names must match ^[a-z0-9][a-z0-9-]{{0,31}}$"
        )))
    }
}

/// Split a valid ID into its timestamp stamp and sender name.
pub fn id_parts(id: &str) -> Option<(&str, &str)> {
    let b = id.as_bytes();
    // stamp, '-', name (at least 1), '-', 4 hex
    if b.len() < STAMP_LEN + 1 + 1 + 1 + 4 {
        return None;
    }
    let stamp = &b[..STAMP_LEN];
    let digits = |r: &[u8]| r.iter().all(u8::is_ascii_digit);
    if !(digits(&stamp[..8]) && stamp[8] == b'T' && digits(&stamp[9..15]) && stamp[15] == b'Z') {
        return None;
    }
    if b[STAMP_LEN] != b'-' {
        return None;
    }
    let tail = &b[b.len() - 5..];
    if tail[0] != b'-'
        || !tail[1..]
            .iter()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
    {
        return None;
    }
    let from = &id[STAMP_LEN + 1..id.len() - 5];
    if !is_valid_name(from) {
        return None;
    }
    Some((&id[..STAMP_LEN], from))
}

pub fn is_valid_id(id: &str) -> bool {
    id_parts(id).is_some()
}

pub fn check_id(what: &str, id: &str) -> Result<()> {
    if is_valid_id(id) {
        Ok(())
    } else {
        Err(CqError::refused(format!(
            "invalid {what} {id:?}: IDs look like 20261024T000615Z-lead-ab12"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(is_valid_name("worker-a"));
        assert!(is_valid_name("0"));
        assert!(is_valid_name(&"a".repeat(32)));
        assert!(!is_valid_name(&"a".repeat(33)));
        assert!(!is_valid_name(""));
        assert!(!is_valid_name("-a"));
        assert!(!is_valid_name("../x"));
        assert!(!is_valid_name("Worker"));
        assert!(!is_valid_name("a_b"));
        assert!(!is_valid_name("a.b"));
    }

    #[test]
    fn ids() {
        assert_eq!(
            id_parts("20261024T000615Z-ui-lead-ab12"),
            Some(("20261024T000615Z", "ui-lead"))
        );
        assert!(is_valid_id("20261024T000615Z-a-0000"));
        assert!(!is_valid_id("20261024T000615Z--0000"));
        assert!(!is_valid_id("20261024T000615Z-a-ABCD"));
        assert!(!is_valid_id("20261024X000615Z-a-abcd"));
        assert!(!is_valid_id("20261024T000615Z-../x-abcd"));
        assert!(!is_valid_id("short"));
        assert!(!is_valid_id(""));
    }
}
