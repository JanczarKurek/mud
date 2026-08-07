//! Clap `value_parser` for octal permission modes (`600`, `660`, etc.).
//!
//! Returning `Result<u32, String>` lets clap render a nice error message
//! that includes the offending flag name and value; an `Err` here turns the
//! whole `parse()` call into a clean exit-2, replacing the prior
//! warn-and-default behaviour of `src/main.rs` / `src/bin/server.rs`.

pub fn parse_octal_mode(s: &str) -> Result<u32, String> {
    u32::from_str_radix(s, 8).map_err(|err| format!("`{s}` is not a valid octal mode: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_three_digit_octal() {
        assert_eq!(parse_octal_mode("600").unwrap(), 0o600);
        assert_eq!(parse_octal_mode("660").unwrap(), 0o660);
        assert_eq!(parse_octal_mode("777").unwrap(), 0o777);
    }

    #[test]
    fn rejects_non_octal_digits() {
        assert!(parse_octal_mode("9zz").is_err());
        assert!(parse_octal_mode("800").is_err());
    }
}
