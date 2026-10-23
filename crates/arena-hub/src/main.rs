//! `arena-hub` binary placeholder.

fn crate_name() -> &'static str {
    "arena-hub"
}

fn main() {
    println!("{}", crate_name());
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name_is_set() {
        assert_eq!(super::crate_name(), "arena-hub");
    }
}
