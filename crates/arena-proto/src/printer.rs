use std::net::{Ipv4Addr, SocketAddrV4};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const KEY_PRINTER_SELECTED: &str = "printer.selected";
pub const KEY_PRINTER_PAPER_MM: &str = "printer.paper_mm";
pub const KEY_PRINTER_STATUS: &str = "printer.status";

pub const DEFAULT_PRINTER_SELECTED: &str = "";
pub const DEFAULT_PRINTER_PAPER_MM: &str = "80";
pub const PRINTER_PAPER_WIDTHS_MM: &[&str] = &["58", "80"];
pub const DEFAULT_PRINTER_STATUS: &str =
    r#"{"state":"unset","error_kind":null,"error":null,"at":null}"#;
pub const DEFAULT_PRINTER_PORT: u16 = 9100;
pub const MAX_PRINTER_ERROR_CHARS: usize = 200;
pub const MAX_PRINTER_NAME_CHARS: usize = 200;
pub const PRINTER_IP_FIELD: &str = "ip";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum PrinterKind {
    Usb,
    Lan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct FoundPrinter {
    pub id: String,
    pub kind: PrinterKind,
    pub name: String,
    #[ts(optional = nullable)]
    pub addr: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PrinterFindResult {
    pub printers: Vec<FoundPrinter>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PrinterSelected {
    pub kind: PrinterKind,
    pub id_or_addr: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrinterSelectedError {
    Shape,
    Ip,
}

impl PrinterSelectedError {
    pub fn field(self) -> &'static str {
        match self {
            Self::Shape => KEY_PRINTER_SELECTED,
            Self::Ip => PRINTER_IP_FIELD,
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::Shape => {
                "the printer is empty or {\"kind\": \"usb\"|\"lan\", \"id_or_addr\", \"name\"}"
            }
            Self::Ip => "the printer address is an IPv4 address with an optional port (ip[:port])",
        }
    }
}

fn plain_text(s: &str, max: usize) -> bool {
    let n = s.chars().count();
    n > 0 && n <= max && s.trim() == s && !s.chars().any(char::is_control)
}

impl PrinterSelected {
    pub fn parse(value: &str) -> Result<Option<Self>, PrinterSelectedError> {
        if value.is_empty() {
            return Ok(None);
        }
        let mut p: Self = serde_json::from_str(value).map_err(|_| PrinterSelectedError::Shape)?;
        if !plain_text(&p.name, MAX_PRINTER_NAME_CHARS) {
            return Err(PrinterSelectedError::Shape);
        }
        match p.kind {
            PrinterKind::Lan => {
                let addr = parse_printer_addr(&p.id_or_addr).ok_or(PrinterSelectedError::Ip)?;
                p.id_or_addr = addr.to_string();
            }
            PrinterKind::Usb => {
                if !plain_text(&p.id_or_addr, MAX_PRINTER_NAME_CHARS) {
                    return Err(PrinterSelectedError::Shape);
                }
            }
        }
        Ok(Some(p))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

pub fn parse_printer_addr(text: &str) -> Option<SocketAddrV4> {
    let text = text.trim();
    let (ip, port) = match text.split_once(':') {
        Some((ip, port)) => {
            if port.is_empty() || port.len() > 5 || !port.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            (ip, port.parse::<u16>().ok().filter(|p| *p != 0)?)
        }
        None => (text, DEFAULT_PRINTER_PORT),
    };
    let ip: Ipv4Addr = ip.parse().ok()?;
    ip.is_private().then_some(SocketAddrV4::new(ip, port))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum PrinterState {
    #[default]
    Unset,
    Ok,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum PrinterErrorKind {
    Refused,
    Timeout,
    Unreachable,
    Spooler,
    Other,
}

pub fn valid_printer_error(detail: &str) -> bool {
    !detail.trim().is_empty() && detail.chars().count() <= MAX_PRINTER_ERROR_CHARS
}

pub fn valid_printer_outcome(
    state: PrinterState,
    kind: Option<PrinterErrorKind>,
    detail: Option<&str>,
) -> bool {
    match (state, kind) {
        (PrinterState::Failed, Some(_)) => detail.is_none_or(valid_printer_error),
        (PrinterState::Failed, None) => false,
        (_, Some(_)) => false,
        (_, None) => detail.is_none(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PrinterStatus {
    pub state: PrinterState,
    #[serde(default)]
    pub error_kind: Option<PrinterErrorKind>,
    pub error: Option<String>,
    #[ts(type = "number | null")]
    pub at: Option<i64>,
}

impl PrinterStatus {
    pub fn parse(value: &str) -> Option<Self> {
        let s: Self = serde_json::from_str(value).ok()?;
        s.is_consistent().then_some(s)
    }

    pub fn is_consistent(&self) -> bool {
        valid_printer_outcome(self.state, self.error_kind, self.error.as_deref())
            && (self.state == PrinterState::Unset) == self.at.is_none()
    }

    pub fn unset() -> Self {
        Self {
            state: PrinterState::Unset,
            error_kind: None,
            error: None,
            at: None,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| DEFAULT_PRINTER_STATUS.to_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PrinterStatusSet {
    pub state: PrinterState,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub error_kind: Option<PrinterErrorKind>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct PrinterStatusReply {
    pub outcome: PrinterOutcome,
    pub status: Option<PrinterStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "export-ts", ts(export))]
pub enum PrinterOutcome {
    Recorded,
    PrintedNotRecorded,
}

pub fn printer_error_text(message: &str) -> String {
    let cleaned: String = message
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let trimmed = cleaned.trim();
    let text = if trimmed.is_empty() {
        "the printer did not answer"
    } else {
        trimmed
    };
    text.chars().take(MAX_PRINTER_ERROR_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_take_an_optional_port_and_default_to_9100() {
        assert_eq!(
            parse_printer_addr("192.168.1.50"),
            Some("192.168.1.50:9100".parse().unwrap())
        );
        assert_eq!(
            parse_printer_addr(" 10.0.0.7:9101 "),
            Some("10.0.0.7:9101".parse().unwrap())
        );
        assert_eq!(
            parse_printer_addr("172.31.255.9"),
            Some("172.31.255.9:9100".parse().unwrap())
        );
        for bad in [
            "abc",
            "",
            "192.168.1",
            "192.168.1.50:",
            "192.168.1.50:0",
            "192.168.1.50:65536",
            "192.168.1.50:+1",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "127.0.0.1",
            "127.0.0.1:9100",
            "169.254.169.254",
            "169.254.1.20:9100",
            "8.8.8.8",
            "192.0.2.50",
            "172.32.0.1",
            "11.0.0.1",
            "printer.local",
            "::1",
            "192.168.1.50:91:00",
        ] {
            assert_eq!(parse_printer_addr(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_selection_is_empty_or_a_known_printer_with_its_address_normalised() {
        assert_eq!(PrinterSelected::parse(""), Ok(None));
        let lan = PrinterSelected::parse(
            r#"{"kind":"lan","id_or_addr":"192.168.1.50","name":"Counter printer"}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(lan.id_or_addr, "192.168.1.50:9100");
        assert_eq!(PrinterSelected::parse(&lan.to_json()), Ok(Some(lan)));
        let usb = PrinterSelected::parse(
            r#"{"kind":"usb","id_or_addr":"EPSON TM-T82","name":"EPSON TM-T82"}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(usb.kind, PrinterKind::Usb);
        for addr in ["abc", "127.0.0.1", "169.254.169.254", "8.8.8.8:9100"] {
            let value = format!(r#"{{"kind":"lan","id_or_addr":"{addr}","name":"P"}}"#);
            assert_eq!(
                PrinterSelected::parse(&value),
                Err(PrinterSelectedError::Ip),
                "{addr}"
            );
        }
        assert_eq!(PrinterSelectedError::Ip.field(), "ip");
        for bad in [
            "x",
            "{}",
            r#"{"kind":"bt","id_or_addr":"a","name":"a"}"#,
            r#"{"kind":"usb","id_or_addr":"","name":"a"}"#,
            r#"{"kind":"usb","id_or_addr":"a","name":""}"#,
            r#"{"kind":"usb","id_or_addr":"a","name":"a\n"}"#,
            r#"{"kind":"usb","id_or_addr":"a","name":"a","extra":1}"#,
        ] {
            assert_eq!(
                PrinterSelected::parse(bad),
                Err(PrinterSelectedError::Shape),
                "{bad}"
            );
        }
    }

    #[test]
    fn statuses_are_consistent_with_their_state() {
        let unset = PrinterStatus::parse(DEFAULT_PRINTER_STATUS).unwrap();
        assert_eq!(unset.state, PrinterState::Unset);
        assert_eq!(unset, PrinterStatus::unset());
        assert_eq!(PrinterStatus::unset().to_json(), DEFAULT_PRINTER_STATUS);
        assert_eq!(
            PrinterStatus::parse(r#"{"state":"unset","error":null,"at":null}"#),
            Some(PrinterStatus::unset())
        );
        let ok = PrinterStatus {
            state: PrinterState::Ok,
            error_kind: None,
            error: None,
            at: Some(5),
        };
        assert_eq!(PrinterStatus::parse(&ok.to_json()), Some(ok));
        let failed = PrinterStatus {
            state: PrinterState::Failed,
            error_kind: Some(PrinterErrorKind::Refused),
            error: Some("refused".into()),
            at: Some(5),
        };
        assert_eq!(PrinterStatus::parse(&failed.to_json()), Some(failed));
        assert!(
            PrinterStatus::parse(
                r#"{"state":"failed","error_kind":"timeout","error":null,"at":5}"#
            )
            .is_some()
        );
        for bad in [
            r#"{"state":"failed","error":"refused","at":5}"#,
            r#"{"state":"failed","error_kind":null,"error":"x","at":5}"#,
            r#"{"state":"failed","error_kind":"refused","error":"","at":5}"#,
            r#"{"state":"failed","error_kind":"refused","error":"   ","at":5}"#,
            r#"{"state":"ok","error_kind":"refused","error":null,"at":5}"#,
            r#"{"state":"ok","error":"x","at":5}"#,
            r#"{"state":"ok","error":null,"at":null}"#,
            r#"{"state":"unset","error":null,"at":5}"#,
            r#"{"state":"done","error":null,"at":5}"#,
            "junk",
        ] {
            assert_eq!(PrinterStatus::parse(bad), None, "{bad}");
        }
        let long = format!(
            r#"{{"state":"failed","error_kind":"other","error":"{}","at":5}}"#,
            "e".repeat(201)
        );
        assert_eq!(PrinterStatus::parse(&long), None);
        assert_eq!(
            serde_json::to_value(PrinterState::Skipped).unwrap(),
            serde_json::json!("skipped")
        );
    }

    #[test]
    fn error_text_is_one_line_of_at_most_200_characters() {
        assert_eq!(printer_error_text("  refused\nby host "), "refused by host");
        assert_eq!(printer_error_text(""), "the printer did not answer");
        assert_eq!(printer_error_text(&"é".repeat(300)).chars().count(), 200);
        assert!(!valid_printer_error("  "));
        assert!(valid_printer_error("x"));
        assert!(!valid_printer_error(&"e".repeat(201)));
    }
}
