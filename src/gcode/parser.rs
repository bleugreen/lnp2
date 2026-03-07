use regex::Regex;
use std::sync::LazyLock;

use crate::gcode::Position;

static CONFIRM_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^ok").unwrap());
static POSITION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"X:(?P<X>-?\d+\.?\d*)\s*Y:(?P<Y>-?\d+\.?\d*)\s*Z:(?P<Z>-?\d+\.?\d*)\s*A:(?P<A>-?\d+\.?\d*)\s*B:(?P<B>-?\d+\.?\d*)").unwrap()
});
static VACUUM_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"data:(?P<Value>.+)").unwrap());
static ERROR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(Error:|!!)").unwrap());
static RS485_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"rs485-reply:\s*(?P<hex>[0-9A-Fa-f]+)").unwrap());

pub fn is_ok(line: &str) -> bool {
    CONFIRM_RE.is_match(line)
}

pub fn is_error(line: &str) -> bool {
    ERROR_RE.is_match(line)
}

pub fn parse_position(text: &str) -> Option<Position> {
    let caps = POSITION_RE.captures(text)?;
    Some(Position {
        x: caps["X"].parse().ok()?,
        y: caps["Y"].parse().ok()?,
        z: caps["Z"].parse().ok()?,
        a: caps["A"].parse().ok()?,
        b: caps["B"].parse().ok()?,
    })
}

/// Extract the hex payload from an RS-485 reply line.
pub fn parse_rs485(text: &str) -> Option<String> {
    for line in text.lines() {
        if let Some(caps) = RS485_RE.captures(line) {
            return Some(caps["hex"].to_string());
        }
    }
    None
}

pub fn parse_vacuum(text: &str) -> Option<f64> {
    for line in text.lines() {
        if let Some(caps) = VACUUM_RE.captures(line) {
            return caps["Value"].trim().parse().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_position() {
        let line = "X:100.0000 Y:200.0000 Z:31.5000 A:0.0000 B:0.0000 Count X:8000 Y:16000 Z:2520";
        let pos = parse_position(line).unwrap();
        assert!((pos.x - 100.0).abs() < 0.001);
        assert!((pos.y - 200.0).abs() < 0.001);
        assert!((pos.z - 31.5).abs() < 0.001);
        assert!((pos.a - 0.0).abs() < 0.001);
        assert!((pos.b - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_is_ok() {
        assert!(is_ok("ok"));
        assert!(is_ok("ok T:20.0"));
        assert!(!is_ok("X:0.0000"));
    }

    #[test]
    fn test_is_error() {
        assert!(is_error("Error: Line 1"));
        assert!(is_error("!! Emergency stop"));
        assert!(!is_error("ok"));
    }

    #[test]
    fn test_parse_rs485() {
        let text = "rs485-reply: 0013000D0A000007800B4248571720343331";
        let hex = parse_rs485(text).unwrap();
        assert_eq!(hex, "0013000D0A000007800B4248571720343331");
    }

    #[test]
    fn test_parse_rs485_with_ok() {
        let text = "rs485-reply: 2B1347010A03\nok";
        assert!(parse_rs485(text).is_some());
    }

    #[test]
    fn test_parse_rs485_no_match() {
        assert!(parse_rs485("ok").is_none());
        assert!(parse_rs485("").is_none());
    }

    #[test]
    fn test_parse_vacuum() {
        let text = "echo: i2c-reply: from:109 bytes:1 data:230";
        assert!((parse_vacuum(text).unwrap() - 230.0).abs() < 0.001);
    }
}
