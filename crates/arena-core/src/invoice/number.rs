//! The shape of an invoice number (Story 5.14): `<branch code>/<FY>/E<epoch counter><instance
//! tag>/<seq>`, for example `KOR/26-27/E3a/000142`. The sequence restarts at 1 in every
//! `(fy, segment)`: a new financial year or a new epoch opens a fresh segment, so a number is
//! never repeated, and two Hubs that minted the same provisional counter still differ by tag.

/// The financial year as printed on an invoice: `FY2026-27` is `26-27`, a calendar-year `FY2027`
/// is `27`.
pub fn fy_label(fy: &str) -> String {
    let years = fy.strip_prefix("FY").unwrap_or(fy);
    match years.split_once('-') {
        Some((start, end)) => format!("{}-{end}", short_year(start)),
        None => short_year(years).to_owned(),
    }
}

fn short_year(year: &str) -> &str {
    if year.len() > 2 && year.is_char_boundary(year.len() - 2) {
        &year[year.len() - 2..]
    } else {
        year
    }
}

/// The segment of an epoch: `E` + the counter + a one-letter tag drawn from the writer's
/// instance id (its last character), `E3a`.
pub fn segment(counter: i64, hub_instance_id: &str) -> String {
    let tag = hub_instance_id
        .bytes()
        .last()
        .map_or('a', |b| char::from(b'a' + b % 26));
    format!("E{counter}{tag}")
}

/// The full number for the `seq`th invoice of `segment` in the financial year `fy` (as stamped on
/// the order).
pub fn format(branch_code: &str, fy: &str, segment: &str, seq: i64) -> String {
    format!("{branch_code}/{}/{segment}/{seq:06}", fy_label(fy))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_financial_year_prints_short() {
        assert_eq!(fy_label("FY2026-27"), "26-27");
        assert_eq!(fy_label("FY2099-00"), "99-00");
        assert_eq!(fy_label("FY2027"), "27");
        assert_eq!(fy_label("2026-27"), "26-27");
    }

    #[test]
    fn a_segment_carries_the_counter_and_a_tag_of_the_instance() {
        assert_eq!(segment(3, "01J0000000000000000000000A"), segment(3, "XA"));
        assert_ne!(segment(3, "A"), segment(4, "A"));
        assert_ne!(segment(3, "A"), segment(3, "B"));
        let s = segment(12, "01HZZ");
        assert!(s.starts_with("E12") && s.len() == 4, "{s}");
        assert!(s.ends_with(|c: char| c.is_ascii_lowercase()));
        assert_eq!(segment(1, ""), "E1a");
    }

    #[test]
    fn a_number_is_six_digits_and_grows_past_a_million() {
        assert_eq!(
            format("KOR", "FY2026-27", "E3a", 142),
            "KOR/26-27/E3a/000142"
        );
        assert_eq!(format("K", "FY2027", "E1a", 1_234_567), "K/27/E1a/1234567");
    }
}
