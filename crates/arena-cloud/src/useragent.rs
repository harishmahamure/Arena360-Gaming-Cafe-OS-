fn browser(ua: &str) -> Option<&'static str> {
    let has = |s: &str| ua.contains(s);
    if has("Edg/") || has("EdgA/") || has("EdgiOS/") {
        Some("Edge")
    } else if has("OPR/") || has("Opera") {
        Some("Opera")
    } else if has("SamsungBrowser/") {
        Some("Samsung Internet")
    } else if has("Firefox/") || has("FxiOS/") {
        Some("Firefox")
    } else if has("Chrome/") || has("CriOS/") || has("Chromium/") {
        Some("Chrome")
    } else if has("Safari/") && has("Version/") {
        Some("Safari")
    } else {
        None
    }
}

fn platform(ua: &str) -> Option<&'static str> {
    let has = |s: &str| ua.contains(s);
    if has("Android") {
        Some("Android")
    } else if has("iPhone") || has("iPad") || has("iPod") {
        Some("iOS")
    } else if has("CrOS") {
        Some("ChromeOS")
    } else if has("Windows") {
        Some("Windows")
    } else if has("Macintosh") || has("Mac OS X") {
        Some("macOS")
    } else if has("Linux") {
        Some("Linux")
    } else {
        None
    }
}

pub fn device_label(user_agent: Option<&str>) -> String {
    let ua = user_agent.unwrap_or_default();
    match (browser(ua), platform(ua)) {
        (Some(b), Some(p)) => format!("{b} on {p}"),
        (Some(b), None) => b.to_owned(),
        (None, Some(p)) => format!("Browser on {p}"),
        (None, None) => "Unknown browser".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_coarse() {
        for (ua, want) in [
            (
                "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Mobile Safari/537.36",
                "Chrome on Android",
            ),
            (
                "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1",
                "Safari on iOS",
            ),
            (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36 Edg/129.0.0.0",
                "Edge on Windows",
            ),
            (
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 14.5; rv:131.0) Gecko/20100101 Firefox/131.0",
                "Firefox on macOS",
            ),
            (
                "Mozilla/5.0 (Linux; Android 14; SM-S918B) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/25.0 Chrome/121.0.0.0 Mobile Safari/537.36",
                "Samsung Internet on Android",
            ),
            (
                "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36",
                "Chrome on Linux",
            ),
            ("curl/8.7.1", "Unknown browser"),
        ] {
            assert_eq!(device_label(Some(ua)), want, "{ua}");
        }
        assert_eq!(device_label(None), "Unknown browser");
    }
}
