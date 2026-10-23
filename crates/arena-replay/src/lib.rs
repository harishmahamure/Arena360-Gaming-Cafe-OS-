//! `arena-replay` placeholder.

/// Returns this crate's name.
pub fn crate_name() -> &'static str {
    "arena-replay"
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name_is_set() {
        assert_eq!(super::crate_name(), "arena-replay");
    }
}
