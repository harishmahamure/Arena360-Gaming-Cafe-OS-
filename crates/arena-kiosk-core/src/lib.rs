//! `arena-kiosk-core` placeholder.

/// Returns this crate's name.
pub fn crate_name() -> &'static str {
    "arena-kiosk-core"
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name_is_set() {
        assert_eq!(super::crate_name(), "arena-kiosk-core");
    }
}
