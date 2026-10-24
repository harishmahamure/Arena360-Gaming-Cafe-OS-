use arena_proto::{BRANCH_CODE_MAX, BRANCH_CODE_MIN};

const SUFFIXES: &[u8] = b"23456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

const FALLBACK: &str = "BRX";

fn from_text(text: &str) -> Option<String> {
    let words: Vec<String> = text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .map(|w| w.to_ascii_uppercase())
        .filter(|w| !w.is_empty())
        .collect();
    if words.is_empty() {
        return None;
    }
    let mut code: String = if words.len() > 1 {
        words
            .iter()
            .take(BRANCH_CODE_MAX)
            .filter_map(|w| w.chars().next())
            .collect()
    } else {
        String::new()
    };
    if code.len() < BRANCH_CODE_MIN {
        code = words.concat().chars().take(BRANCH_CODE_MIN).collect();
    }
    while code.len() < BRANCH_CODE_MIN {
        code.push('X');
    }
    Some(code)
}

pub fn derive(name: &str, city: &str) -> String {
    from_text(name)
        .or_else(|| from_text(city))
        .unwrap_or_else(|| FALLBACK.to_owned())
}

pub fn unique(base: &str, taken: &[String]) -> Option<String> {
    let free = |c: &str| !taken.iter().any(|t| t == c);
    if free(base) {
        return Some(base.to_owned());
    }
    let stem = &base[..base.len() - 1];
    SUFFIXES
        .iter()
        .map(|&s| format!("{stem}{}", s as char))
        .find(|c| c != base && free(c))
}

pub fn assign(name: &str, city: &str, taken: &[String]) -> Option<String> {
    unique(&derive(name, city), taken)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn taken(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn multi_word_names_take_initials() {
        assert_eq!(derive("Pixel Arena Gaming", "Pune"), "PAG");
        assert_eq!(derive("pixel  arena-gaming zone", "Pune"), "PAGZ");
        assert_eq!(derive("A B C D E F G H", "Pune"), "ABCDEF");
        assert_eq!(derive("Level Up 2 Gaming", "Pune"), "LU2G");
    }

    #[test]
    fn short_initials_use_the_joined_name() {
        assert_eq!(derive("Pixel Arena", "Pune"), "PIX");
        assert_eq!(derive("Arena", "Pune"), "ARE");
        assert_eq!(derive("Café Zone", "Pune"), "CAF");
    }

    #[test]
    fn short_names_pad_with_x() {
        assert_eq!(derive("Go", "Pune"), "GOX");
        assert_eq!(derive("Q", "Pune"), "QXX");
        assert_eq!(derive("ゲーム", "Pune"), "PUN");
        assert_eq!(derive("", "Navi Mumbai City"), "NMC");
        assert_eq!(derive("पिक्सेल अखाड़ा", "पुणे"), "BRX");
        assert_eq!(derive("", ""), "BRX");
        assert_eq!(unique("BRX", &taken(&["BRX"])).as_deref(), Some("BR2"));
    }

    #[test]
    fn clashes_replace_the_last_character() {
        assert_eq!(unique("PAG", &[]).as_deref(), Some("PAG"));
        assert_eq!(unique("PAG", &taken(&["PAG"])).as_deref(), Some("PA2"));
        assert_eq!(
            unique("PAG", &taken(&["PAG", "PA2", "PA3"])).as_deref(),
            Some("PA4")
        );
        let nine = taken(&[
            "PAG", "PA2", "PA3", "PA4", "PA5", "PA6", "PA7", "PA8", "PA9",
        ]);
        assert_eq!(unique("PAG", &nine).as_deref(), Some("PAA"));
        assert_eq!(
            assign("Pixel Arena Gaming", "Pune", &taken(&["PAG"])).as_deref(),
            Some("PA2")
        );
        let all: Vec<String> = std::iter::once("PAG".to_owned())
            .chain(SUFFIXES.iter().map(|&s| format!("PA{}", s as char)))
            .collect();
        assert_eq!(unique("PAG", &all), None);
        for c in [
            derive("Pixel Arena Gaming", "Pune"),
            unique("PAG", &taken(&["PAG"])).unwrap(),
        ] {
            assert!(arena_proto::valid_branch_code(&c), "{c}");
        }
    }
}
