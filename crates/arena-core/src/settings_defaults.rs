//! The settings every branch starts with, written by `branch.bootstrap`. Every value is TEXT.

/// `(key, value)` pairs inserted at bootstrap, in this order.
pub const DEFAULT_SETTINGS: &[(&str, &str)] = &[
    ("idle_lock_minutes", "5"),
    ("hub.locale", "en-IN"),
    ("receipt.footer", ""),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_unique_and_required_ones_present() {
        let mut keys: Vec<_> = DEFAULT_SETTINGS.iter().map(|(k, _)| *k).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), DEFAULT_SETTINGS.len());
        for (k, v) in [
            ("idle_lock_minutes", "5"),
            ("hub.locale", "en-IN"),
            ("receipt.footer", ""),
        ] {
            assert!(DEFAULT_SETTINGS.contains(&(k, v)), "{k}");
        }
    }
}
