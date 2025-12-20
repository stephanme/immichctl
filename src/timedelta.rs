use chrono::TimeDelta;
use lazy_static::lazy_static;
use regex::Regex;
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

lazy_static! {
    static ref TIME_DELTA_RE: Regex =
        Regex::new(r"^(?P<sign>[-+])?(?P<days>\d+d)?(?P<hours>\d+h)?(?P<minutes>\d+m)?$").unwrap();
}

/// Wrapper for chrono::TimeDelta to support parsing from string and formatting.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimeDeltaValue(TimeDelta);

impl Deref for TimeDeltaValue {
    type Target = TimeDelta;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for TimeDeltaValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut total_seconds = self.0.num_seconds();

        if total_seconds == 0 {
            return write!(f, "0m");
        }

        let sign = if total_seconds < 0 {
            total_seconds = -total_seconds;
            "-"
        } else {
            ""
        };

        let days = total_seconds / (24 * 3600);
        let hours = (total_seconds % (24 * 3600)) / 3600;
        let minutes = (total_seconds % 3600) / 60;

        let mut result = String::new();
        if days > 0 {
            result.push_str(&format!("{}d", days));
        }
        if hours > 0 {
            result.push_str(&format!("{}h", hours));
        }
        if minutes > 0 {
            result.push_str(&format!("{}m", minutes));
        }

        // If the total delta is less than a minute (but not zero), display as 0m
        if result.is_empty() && total_seconds > 0 {
            return write!(f, "0m");
        }

        write!(f, "{}{}", sign, result)
    }
}

impl FromStr for TimeDeltaValue {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let caps = TIME_DELTA_RE
            .captures(s)
            .ok_or_else(|| anyhow::anyhow!("Invalid time delta format"))?;

        // check that at least one of days, hours, minutes is present
        if caps.name("days").is_none()
            && caps.name("hours").is_none()
            && caps.name("minutes").is_none()
        {
            return Err(anyhow::anyhow!("Invalid time delta format"));
        }

        let sign = if caps.name("sign").map_or("+", |m| m.as_str()) == "-" {
            -1
        } else {
            1
        };
        let days = caps
            .name("days")
            .map_or(0, |m| m.as_str().trim_end_matches('d').parse().unwrap_or(0));
        let hours = caps
            .name("hours")
            .map_or(0, |m| m.as_str().trim_end_matches('h').parse().unwrap_or(0));
        let minutes = caps
            .name("minutes")
            .map_or(0, |m| m.as_str().trim_end_matches('m').parse().unwrap_or(0));

        Ok(TimeDeltaValue(
            (TimeDelta::days(days) + TimeDelta::hours(hours) + TimeDelta::minutes(minutes)) * sign,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_str() {
        let td = TimeDeltaValue::from_str("1d2h3m").unwrap();
        assert_eq!(
            *td,
            TimeDelta::days(1) + TimeDelta::hours(2) + TimeDelta::minutes(3)
        );

        let td = TimeDeltaValue::from_str("-1d2h3m").unwrap();
        assert_eq!(
            *td,
            (TimeDelta::days(1) + TimeDelta::hours(2) + TimeDelta::minutes(3)) * -1
        );

        let td = TimeDeltaValue::from_str("+1d").unwrap();
        assert_eq!(*td, TimeDelta::days(1));

        let td = TimeDeltaValue::from_str("2h").unwrap();
        assert_eq!(*td, TimeDelta::hours(2));

        let td = TimeDeltaValue::from_str("-30m").unwrap();
        assert_eq!(*td, TimeDelta::minutes(-30));
    }

    #[test]
    fn test_from_str_invalid() {
        assert!(TimeDeltaValue::from_str("").is_err());
        assert!(TimeDeltaValue::from_str("-").is_err());
        assert!(TimeDeltaValue::from_str("+").is_err());
        assert!(TimeDeltaValue::from_str("0").is_err());
        assert!(TimeDeltaValue::from_str("1d 2h").is_err());
        assert!(TimeDeltaValue::from_str("1h1d").is_err());
        assert!(TimeDeltaValue::from_str("foo").is_err());
    }

    #[test]
    fn test_display() {
        let td = TimeDeltaValue(TimeDelta::days(1) + TimeDelta::hours(2) + TimeDelta::minutes(3));
        assert_eq!(td.to_string(), "1d2h3m");

        let td = TimeDeltaValue((TimeDelta::days(1) + TimeDelta::minutes(3)) * -1);
        assert_eq!(td.to_string(), "-1d3m");

        let td = TimeDeltaValue(TimeDelta::hours(5));
        assert_eq!(td.to_string(), "5h");

        let td = TimeDeltaValue(TimeDelta::zero());
        assert_eq!(td.to_string(), "0m");

        let td = TimeDeltaValue(TimeDelta::seconds(30));
        assert_eq!(td.to_string(), "0m");
    }
}
