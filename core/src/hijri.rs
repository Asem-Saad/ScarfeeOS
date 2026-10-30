//! Converts Gregorian dates to the tabular (arithmetic) Islamic calendar and back.
//! Models state Hijri dates wrongly, so the arithmetic stays in code.
//!
//! The results are approximate. The tabular calendar follows a fixed 30-year
//! cycle and never looks at the moon. Official calendars (Umm al-Qura in Saudi
//! Arabia) and moon sighting can differ from it by a day or two, and the start
//! of Ramadan, Shawwal and Dhu al-Hijjah must come from the local authority.

use crate::datetime::Date;

/// A date in the tabular Islamic calendar. Build it with [`to_hijri_approx`];
/// treat it as approximate, never as an official date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HijriDate {
    pub year: i32,
    /// 1 is Muharram, 12 is Dhu al-Hijjah.
    pub month: u32,
    pub day: u32,
}

const MONTHS_EN: [&str; 12] = [
    "Muharram",
    "Safar",
    "Rabi' al-awwal",
    "Rabi' al-thani",
    "Jumada al-awwal",
    "Jumada al-thani",
    "Rajab",
    "Sha'ban",
    "Ramadan",
    "Shawwal",
    "Dhu al-Qi'dah",
    "Dhu al-Hijjah",
];

const MONTHS_AR: [&str; 12] = [
    "محرم",
    "صفر",
    "ربيع الأول",
    "ربيع الآخر",
    "جمادى الأولى",
    "جمادى الآخرة",
    "رجب",
    "شعبان",
    "رمضان",
    "شوال",
    "ذو القعدة",
    "ذو الحجة",
];

/// Days from 1970-01-01 back to 1 Muharram 1 AH (civil epoch, Friday 19 July 622
/// Gregorian): Julian day number 1948440 minus 2440588 for 1970-01-01.
const EPOCH_DAYS: i64 = 1_948_440 - 2_440_588;

/// Bounds on the Hijri year accepted by [`from_hijri_approx`], so the result
/// always fits the Gregorian `Date` year.
const MAX_YEAR: i32 = 1_000_000;

impl HijriDate {
    pub fn new(year: i32, month: u32, day: u32) -> HijriDate {
        HijriDate { year, month, day }
    }

    pub fn month_name_en(self) -> &'static str {
        MONTHS_EN[(self.month as usize).clamp(1, 12) - 1]
    }

    pub fn month_name_ar(self) -> &'static str {
        MONTHS_AR[(self.month as usize).clamp(1, 12) - 1]
    }
}

/// Years 2, 5, 7, 10, 13, 16, 18, 21, 24, 26 and 29 of each 30-year cycle have 355 days.
pub fn is_leap_hijri_year(year: i32) -> bool {
    (11 * i64::from(year) + 14).rem_euclid(30) < 11
}

/// Odd months have 30 days, even months 29, and Dhu al-Hijjah gets 30 in a leap year.
/// None when the month is not 1 to 12.
pub fn hijri_month_length(year: i32, month: u32) -> Option<u32> {
    match month {
        1..=11 => Some(if month % 2 == 1 { 30 } else { 29 }),
        12 => Some(if is_leap_hijri_year(year) { 30 } else { 29 }),
        _ => None,
    }
}

fn days_before_year(year: i64) -> i64 {
    354 * (year - 1) + (3 + 11 * year).div_euclid(30)
}

fn days_before_month(month: u32) -> i64 {
    (1..month).map(|m| i64::from(hijri_month_length(0, m).unwrap_or(0))).sum()
}

/// Days since 1970-01-01 in the proleptic Gregorian calendar.
fn days_from_civil(date: Date) -> i64 {
    let y = i64::from(date.year) - i64::from(date.month <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (i64::from(date.month) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(date.day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Approximate Hijri date for a Gregorian date, by the tabular calendar.
///
/// May differ by a day or two from Umm al-Qura and from moon sighting. Do not
/// use it for religious observances or official dates; say "about" or
/// "approximately" when showing it to users. `date` must be a valid Gregorian
/// date. Dates before 1 Muharram 1 AH (19 July 622) extend the same rules
/// backwards and have no historical meaning.
pub fn to_hijri_approx(date: Date) -> HijriDate {
    let n = days_from_civil(date) - EPOCH_DAYS;
    // A 30-year cycle is 10631 days, so this estimate is off by at most one year.
    let mut year = (30 * n).div_euclid(10631) + 1;
    while days_before_year(year + 1) <= n {
        year += 1;
    }
    while days_before_year(year) > n {
        year -= 1;
    }
    let mut rest = n - days_before_year(year);
    let mut month = 1;
    loop {
        let len = i64::from(hijri_month_length(year as i32, month).unwrap_or(30));
        if rest < len || month == 12 {
            break;
        }
        rest -= len;
        month += 1;
    }
    HijriDate { year: year as i32, month, day: rest as u32 + 1 }
}

/// Approximate Gregorian date for a Hijri date, by the tabular calendar. The
/// same accuracy limits as [`to_hijri_approx`] apply.
///
/// None when the month is not 1 to 12, the day is not within the month's
/// length, or the year is outside 1 to 1,000,000.
pub fn from_hijri_approx(date: HijriDate) -> Option<Date> {
    if !(1..=MAX_YEAR).contains(&date.year) {
        return None;
    }
    let len = hijri_month_length(date.year, date.month)?;
    if date.day < 1 || date.day > len {
        return None;
    }
    let n = days_before_year(i64::from(date.year)) + days_before_month(date.month) + i64::from(date.day) - 1;
    Some(Date::new(1970, 1, 1).add_days(n + EPOCH_DAYS))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(year: i32, month: u32, day: u32) -> HijriDate {
        HijriDate::new(year, month, day)
    }

    #[test]
    fn epoch_is_friday_19_july_622() {
        // Julian day 1948440 is 1 Muharram 1 AH. 1970-01-01 is Julian day 2440588,
        // so the epoch is 492148 days before 1970-01-01 (the constant), which
        // lands on 19 July 622 Gregorian (16 July 622 Julian), a Friday.
        let epoch = from_hijri_approx(h(1, 1, 1)).unwrap();
        assert_eq!(epoch, Date::new(622, 7, 19));
        assert_eq!(epoch.weekday(), 5);
        assert_eq!(to_hijri_approx(Date::new(622, 7, 19)), h(1, 1, 1));
    }

    #[test]
    fn new_year_2026_is_12_rajab_1447() {
        // 1970-01-01 to 2026-01-01: 56 years * 365 = 20440, plus 14 leap days
        // (1972, 76, ..., 2024) = 20454. Days since the epoch: 20454 + 492148 = 512602.
        // Days before 1447: 354 * 1446 + floor((3 + 11 * 1447) / 30) = 511884 + 530 = 512414.
        // 512602 - 512414 = 188 days into 1447. Rajab starts after 6 months of
        // 30 + 29 + 30 + 29 + 30 + 29 = 177 days, so 188 - 177 = 11 and the day is 12.
        assert_eq!(to_hijri_approx(Date::new(2026, 1, 1)), h(1447, 7, 12));
        assert_eq!(from_hijri_approx(h(1447, 7, 12)), Some(Date::new(2026, 1, 1)));
    }

    #[test]
    fn ramadan_1445_starts_11_march_2024() {
        // 2024-01-01 is 54 * 365 + 13 leap days = 19723 days after 1970-01-01.
        // Days before 1445: 354 * 1444 + floor((3 + 11 * 1445) / 30) = 511176 + 529 = 511705.
        // Ramadan is month 9, after 8 months of 236 days: 511941 days since the epoch,
        // so 511941 - 492148 = 19793 days after 1970, which is 70 days after
        // 2024-01-01. January has 31 and February 29 (60), so that is 11 March.
        assert_eq!(from_hijri_approx(h(1445, 9, 1)), Some(Date::new(2024, 3, 11)));
        assert_eq!(to_hijri_approx(Date::new(2024, 3, 11)), h(1445, 9, 1));
        assert_eq!(to_hijri_approx(Date::new(2024, 3, 10)), h(1445, 8, 29));
    }

    #[test]
    fn muharram_1447_starts_the_day_after_umm_al_qura() {
        // Days before 1447 are 512414, so the day count from 1970 is 512414 - 492148 = 20266,
        // which is 20454 - 20266 = 188 days before 2026-01-01: 27 June 2025.
        // Umm al-Qura began 1447 on 26 June 2025, so the tabular result is a day later.
        assert_eq!(from_hijri_approx(h(1447, 1, 1)), Some(Date::new(2025, 6, 27)));
    }

    #[test]
    fn leap_years_follow_the_30_year_cycle() {
        let leap: Vec<i32> = (1..=30).filter(|&y| is_leap_hijri_year(y)).collect();
        assert_eq!(leap, [2, 5, 7, 10, 13, 16, 18, 21, 24, 26, 29]);
        assert!(is_leap_hijri_year(1447));
        assert!(!is_leap_hijri_year(1446));
        assert!(!is_leap_hijri_year(1448));
        // A full cycle is 19 * 354 + 11 * 355 = 10631 days.
        let cycle: i64 = (1..=30).map(|y| if is_leap_hijri_year(y) { 355 } else { 354 }).sum();
        assert_eq!(cycle, 10631);
        assert_eq!(days_before_year(31), 10631);
    }

    #[test]
    fn month_lengths() {
        for month in 1..=11 {
            assert_eq!(hijri_month_length(1446, month), Some(if month % 2 == 1 { 30 } else { 29 }));
        }
        assert_eq!(hijri_month_length(1446, 12), Some(29));
        assert_eq!(hijri_month_length(1447, 12), Some(30));
        assert_eq!(hijri_month_length(1447, 0), None);
        assert_eq!(hijri_month_length(1447, 13), None);
    }

    #[test]
    fn leap_day_of_dhu_al_hijjah() {
        // 1447 is leap, so 30 Dhu al-Hijjah exists and 1 Muharram 1448 follows it.
        let last = from_hijri_approx(h(1447, 12, 30)).unwrap();
        assert_eq!(to_hijri_approx(last), h(1447, 12, 30));
        assert_eq!(to_hijri_approx(last.add_days(1)), h(1448, 1, 1));
        // 1446 is not leap: day 29 is the last one.
        let last = from_hijri_approx(h(1446, 12, 29)).unwrap();
        assert_eq!(to_hijri_approx(last.add_days(1)), h(1447, 1, 1));
    }

    #[test]
    fn rejects_invalid_input() {
        assert_eq!(from_hijri_approx(h(1446, 12, 30)), None);
        assert_eq!(from_hijri_approx(h(1447, 2, 30)), None);
        assert_eq!(from_hijri_approx(h(1447, 1, 31)), None);
        assert_eq!(from_hijri_approx(h(1447, 1, 0)), None);
        assert_eq!(from_hijri_approx(h(1447, 0, 1)), None);
        assert_eq!(from_hijri_approx(h(1447, 13, 1)), None);
        assert_eq!(from_hijri_approx(h(0, 1, 1)), None);
        assert_eq!(from_hijri_approx(h(-5, 1, 1)), None);
        assert_eq!(from_hijri_approx(h(MAX_YEAR + 1, 1, 1)), None);
        assert!(from_hijri_approx(h(MAX_YEAR, 1, 1)).is_some());
    }

    #[test]
    fn every_day_round_trips_and_advances_by_one() {
        // From 1900 to 2100, which has several Gregorian and Hijri leap years.
        let mut date = Date::new(1900, 1, 1);
        let mut previous = to_hijri_approx(date);
        assert_eq!(from_hijri_approx(previous), Some(date));
        for _ in 0..(200 * 366) {
            date = date.add_days(1);
            let hijri = to_hijri_approx(date);
            assert_eq!(from_hijri_approx(hijri), Some(date), "{date:?}");
            let len = hijri_month_length(previous.year, previous.month).unwrap();
            let expected = if previous.day < len {
                h(previous.year, previous.month, previous.day + 1)
            } else if previous.month < 12 {
                h(previous.year, previous.month + 1, 1)
            } else {
                h(previous.year + 1, 1, 1)
            };
            assert_eq!(hijri, expected, "{date:?}");
            previous = hijri;
        }
    }

    #[test]
    fn every_valid_hijri_date_round_trips() {
        for year in 1..=1500 {
            for month in 1..=12 {
                for day in 1..=hijri_month_length(year, month).unwrap() {
                    let hijri = h(year, month, day);
                    let date = from_hijri_approx(hijri).expect("valid date");
                    assert_eq!(to_hijri_approx(date), hijri);
                }
            }
        }
    }

    #[test]
    fn gregorian_leap_days_round_trip() {
        for date in [Date::new(2000, 2, 29), Date::new(2024, 2, 29), Date::new(2028, 2, 29)] {
            assert_eq!(from_hijri_approx(to_hijri_approx(date)), Some(date));
        }
    }

    #[test]
    fn dates_before_the_epoch_still_convert_consistently() {
        let hijri = to_hijri_approx(Date::new(600, 1, 1));
        assert!(hijri.year <= 0);
        assert!((1..=12).contains(&hijri.month));
    }

    #[test]
    fn month_names() {
        assert_eq!(h(1447, 7, 12).month_name_en(), "Rajab");
        assert_eq!(h(1447, 7, 12).month_name_ar(), "رجب");
        assert_eq!(h(1445, 9, 1).month_name_en(), "Ramadan");
        assert_eq!(h(1445, 9, 1).month_name_ar(), "رمضان");
        assert_eq!(h(1445, 12, 1).month_name_en(), "Dhu al-Hijjah");
        assert_eq!(h(1445, 12, 1).month_name_ar(), "ذو الحجة");
        assert_eq!(h(1445, 1, 1).month_name_en(), "Muharram");
    }
}
