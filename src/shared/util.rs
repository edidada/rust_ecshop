use serde::Deserialize;

use crate::shared::error::AppError;

/// Standard list pagination parameters: page starts at 1, page_size default 20, max 100.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct PageParams {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

impl PageParams {
    pub fn resolve(&self) -> (i64, i64, i64) {
        let page = self.page.unwrap_or(1).max(1);
        let page_size = self.page_size.unwrap_or(20).clamp(1, 100);
        let offset = (page - 1) * page_size;
        (page, page_size, offset)
    }
}

pub fn parse_i64_param(value: Option<&str>, field: &str) -> Result<Option<i64>, AppError> {
    match value {
        None | Some("") => Ok(None),
        Some(s) => s
            .parse::<i64>()
            .map(Some)
            .map_err(|_| AppError::Validation(format!("{field} must be an integer"))),
    }
}

pub fn parse_i64_param_required(value: Option<&str>, field: &str) -> Result<i64, AppError> {
    parse_i64_param(value, field)?
        .ok_or_else(|| AppError::Validation(format!("{field} is required")))
}

/// Format integer cents as a two-decimal string. All money crosses the API as strings.
/// Negative amounts format as "-1.50" (sign from the original value, magnitude from abs).
pub fn cents_to_string(cents: i64) -> String {
    let abs = cents.unsigned_abs();
    let sign = if cents < 0 { "-" } else { "" };
    format!("{}{}.{:02}", sign, abs / 100, abs % 100)
}

/// Parse a decimal money string into integer cents. Rejects floats' precision issues.
pub fn parse_money_cents(value: &str, field: &str) -> Result<i64, AppError> {
    let s = value.trim();
    if s.is_empty() {
        return Err(AppError::Validation(format!("{field} must be a decimal string")));
    }
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() > 2 {
        return Err(AppError::Validation(format!("{field} must be a decimal string")));
    }
    let int_part = parts[0];
    if int_part.is_empty() || !int_part.chars().all(|c| c.is_ascii_digit()) {
        return Err(AppError::Validation(format!("{field} must be a decimal string")));
    }
    let mut cents: i64 = int_part
        .parse::<i64>()
        .map_err(|_| AppError::Validation(format!("{field} is out of range")))?
        .checked_mul(100)
        .ok_or_else(|| AppError::Validation(format!("{field} is out of range")))?;
    if parts.len() == 2 {
        let frac = parts[1];
        if frac.len() > 2 || !frac.chars().all(|c| c.is_ascii_digit()) {
            return Err(AppError::Validation(format!("{field} must be a decimal string")));
        }
        let mut frac_cents: i64 = 0;
        if !frac.is_empty() {
            frac_cents = frac.parse::<i64>().unwrap_or(0);
            if frac.len() == 1 {
                frac_cents *= 10;
            }
        }
        cents = cents.checked_add(frac_cents).ok_or_else(|| {
            AppError::Validation(format!("{field} is out of range"))
        })?;
    }
    Ok(cents)
}

/// Current unix timestamp (seconds).
pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cents_to_string_formats_positive_amounts() {
        assert_eq!(cents_to_string(0), "0.00");
        assert_eq!(cents_to_string(5), "0.05");
        assert_eq!(cents_to_string(50), "0.50");
        assert_eq!(cents_to_string(4990), "49.90");
        assert_eq!(cents_to_string(10780), "107.80");
    }

    #[test]
    fn cents_to_string_formats_negative_amounts() {
        assert_eq!(cents_to_string(-5), "-0.05");
        assert_eq!(cents_to_string(-50), "-0.50");
        assert_eq!(cents_to_string(-150), "-1.50");
        assert_eq!(cents_to_string(-10780), "-107.80");
    }

    #[test]
    fn cents_to_string_handles_i64_min_without_overflow() {
        // unsigned_abs() is total, so i64::MIN does not panic or wrap.
        // i64::MIN = -9223372036854775808 => magnitude ends in 8 cents => "-...08".
        let s = cents_to_string(i64::MIN);
        assert!(s.starts_with('-'));
        assert!(s.ends_with(".08"));
    }

    #[test]
    fn parse_money_cents_accepts_decimal_strings_only() {
        assert_eq!(parse_money_cents("49.90", "a").unwrap(), 4990);
        assert_eq!(parse_money_cents("5", "a").unwrap(), 500);
        assert_eq!(parse_money_cents("0.05", "a").unwrap(), 5);
        assert_eq!(parse_money_cents("12.3", "a").unwrap(), 1230);
        assert!(parse_money_cents("1.234", "a").is_err());
        assert!(parse_money_cents("-1.00", "a").is_err());
        assert!(parse_money_cents("abc", "a").is_err());
        assert!(parse_money_cents("", "a").is_err());
    }
}
