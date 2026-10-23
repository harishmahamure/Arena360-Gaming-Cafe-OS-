//! `arena-pi` binary placeholder.

fn crate_name() -> &'static str {
    "arena-pi"
}

fn main() {
    println!("{}", crate_name());
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name_is_set() {
        assert_eq!(super::crate_name(), "arena-pi");
    }
}
