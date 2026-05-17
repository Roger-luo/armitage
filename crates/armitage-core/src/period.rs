use chrono::{Datelike, Local, NaiveDate};

use crate::error::{Error, Result};
use crate::node::Timeline;

/// A calendar period — either a quarter (`2026-Q2`) or a full year (`2026`).
#[derive(Debug, Clone)]
pub struct Period {
    pub label: String,
    pub start: NaiveDate,
    pub end: NaiveDate,
}

impl Period {
    /// Derive the current quarter from today's date.
    pub fn current() -> Self {
        let today = Local::now().date_naive();
        let q = (today.month0() / 3) + 1;
        Self::quarter(today.year(), q)
    }

    /// Parse a period string: `"2026-Q2"`, `"2026"`, or `"current"`.
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.trim();
        if s.eq_ignore_ascii_case("current") {
            return Ok(Self::current());
        }
        if let Some((year_s, q_s)) = s.split_once('-')
            && let Ok(year) = year_s.parse::<i32>()
        {
            let q_upper = q_s.to_ascii_uppercase();
            if let Some(rest) = q_upper.strip_prefix('Q')
                && let Ok(q) = rest.parse::<u32>()
                && (1..=4).contains(&q)
            {
                return Ok(Self::quarter(year, q));
            }
        }
        if let Ok(year) = s.parse::<i32>() {
            return Ok(Self {
                label: year.to_string(),
                start: NaiveDate::from_ymd_opt(year, 1, 1)
                    .ok_or_else(|| Error::Other(format!("invalid year: {year}")))?,
                end: NaiveDate::from_ymd_opt(year, 12, 31)
                    .ok_or_else(|| Error::Other(format!("invalid year: {year}")))?,
            });
        }
        Err(Error::Other(format!(
            "invalid period '{s}': expected 'YYYY-QN' (e.g. 2026-Q2), 'YYYY', or 'current'"
        )))
    }

    /// Return the (start, end) NaiveDates for a quarter label like "2026-Q2".
    /// Returns `None` if the label is not a valid quarter.
    pub fn quarter_bounds(label: &str) -> Option<(NaiveDate, NaiveDate)> {
        let p = Period::parse(label).ok()?;
        // Re-validate that it actually was a quarter (parse can also accept years).
        p.quarter_label()?;
        Some((p.start, p.end))
    }

    fn quarter(year: i32, q: u32) -> Self {
        let (sm, em) = match q {
            1 => (1u32, 3u32),
            2 => (4, 6),
            3 => (7, 9),
            _ => (10, 12),
        };
        Self {
            label: format!("{year}-Q{q}"),
            start: NaiveDate::from_ymd_opt(year, sm, 1).unwrap(),
            end: NaiveDate::from_ymd_opt(year, em, days_in_month(year, em)).unwrap(),
        }
    }

    /// Return `Some("YYYY-Q[1-4]")` if this period is exactly one quarter, else `None`.
    /// Yearly periods (e.g. `"2026"`) return `None`.
    pub fn quarter_label(&self) -> Option<String> {
        // The label set in `quarter()` is already YYYY-QN; yearly is just YYYY.
        let bytes = self.label.as_bytes();
        if bytes.len() == 7
            && bytes[0..4].iter().all(|b| b.is_ascii_digit())
            && bytes[4] == b'-'
            && bytes[5] == b'Q'
            && (b'1'..=b'4').contains(&bytes[6])
        {
            Some(self.label.clone())
        } else {
            None
        }
    }

    /// True if this period overlaps with the given node timeline.
    pub fn overlaps_timeline(&self, tl: &Timeline) -> bool {
        tl.start <= self.end && tl.end >= self.start
    }

    /// True if `date` falls within this period (inclusive).
    pub fn contains_date(&self, date: NaiveDate) -> bool {
        date >= self.start && date <= self.end
    }

    /// Human-readable date range string, e.g. `"Apr 1 – Jun 30, 2026"`.
    pub fn display_range(&self) -> String {
        let months = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        let sm = months[(self.start.month() - 1) as usize];
        let em = months[(self.end.month() - 1) as usize];
        if self.start.year() == self.end.year() {
            format!(
                "{} {} – {} {}, {}",
                sm,
                self.start.day(),
                em,
                self.end.day(),
                self.end.year()
            )
        } else {
            format!(
                "{} {}, {} – {} {}, {}",
                sm,
                self.start.day(),
                self.start.year(),
                em,
                self.end.day(),
                self.end.year()
            )
        }
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_quarter() {
        let p = Period::parse("2026-Q2").unwrap();
        assert_eq!(p.label, "2026-Q2");
        assert_eq!(p.start, NaiveDate::from_ymd_opt(2026, 4, 1).unwrap());
        assert_eq!(p.end, NaiveDate::from_ymd_opt(2026, 6, 30).unwrap());
    }

    #[test]
    fn parse_quarter_case_insensitive() {
        let p = Period::parse("2026-q1").unwrap();
        assert_eq!(p.start, NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());
        assert_eq!(p.end, NaiveDate::from_ymd_opt(2026, 3, 31).unwrap());
    }

    #[test]
    fn parse_annual() {
        let p = Period::parse("2025").unwrap();
        assert_eq!(p.start, NaiveDate::from_ymd_opt(2025, 1, 1).unwrap());
        assert_eq!(p.end, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap());
    }

    #[test]
    fn parse_invalid() {
        assert!(Period::parse("Q2-2026").is_err());
        assert!(Period::parse("2026-Q5").is_err());
        assert!(Period::parse("foobar").is_err());
    }

    #[test]
    fn overlaps_timeline() {
        let p = Period::parse("2026-Q2").unwrap();
        let tl_inside = Timeline {
            start: NaiveDate::from_ymd_opt(2026, 4, 15).unwrap(),
            end: NaiveDate::from_ymd_opt(2026, 6, 15).unwrap(),
        };
        assert!(p.overlaps_timeline(&tl_inside));

        let tl_before = Timeline {
            start: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            end: NaiveDate::from_ymd_opt(2026, 3, 31).unwrap(),
        };
        assert!(!p.overlaps_timeline(&tl_before));

        let tl_straddling = Timeline {
            start: NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            end: NaiveDate::from_ymd_opt(2026, 5, 31).unwrap(),
        };
        assert!(p.overlaps_timeline(&tl_straddling));
    }

    #[test]
    fn q4_end_date() {
        let p = Period::parse("2026-Q4").unwrap();
        assert_eq!(p.end, NaiveDate::from_ymd_opt(2026, 12, 31).unwrap());
    }

    #[test]
    fn display_range() {
        let p = Period::parse("2026-Q2").unwrap();
        assert_eq!(p.display_range(), "Apr 1 – Jun 30, 2026");
    }
}
