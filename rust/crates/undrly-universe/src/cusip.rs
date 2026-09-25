//! CUSIP syntax and check digit, used **only** to recognise security rows in
//! the SPY holdings file and as a build-local key. A CUSIP is never stored
//! or exposed as an identifier, and no ISIN is constructed from one
//! (docs/v1.1-universe.md §2).

/// Whether `s` is 9 characters (`0-9 A-Z * @ #`) with a valid check digit.
pub fn is_valid(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 9 || !b[8].is_ascii_digit() {
        return false;
    }
    let mut sum = 0u32;
    for (i, c) in b[..8].iter().enumerate() {
        let mut v = match c {
            b'0'..=b'9' => u32::from(c - b'0'),
            b'A'..=b'Z' => u32::from(c - b'A') + 10,
            b'*' => 36,
            b'@' => 37,
            b'#' => 38,
            _ => return false,
        };
        if i % 2 == 1 {
            v *= 2;
        }
        sum += v / 10 + v % 10;
    }
    (10 - sum % 10) % 10 == u32::from(b[8] - b'0')
}

#[cfg(test)]
mod tests {
    use super::is_valid;

    #[test]
    fn check_digit() {
        for valid in [
            "67066G104",
            "037833100",
            "594918104",
            "G54950103",
            "H1467J104",
            "806857108",
        ] {
            assert!(is_valid(valid), "{valid}");
        }
        for invalid in [
            "67066G105",
            "67066G10",
            "67066g104",
            "-",
            "CASH_USD1",
            "67066G10X",
        ] {
            assert!(!is_valid(invalid), "{invalid}");
        }
    }
}
