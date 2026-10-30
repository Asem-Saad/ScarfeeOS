//! Resolves relative dates and times in English and Arabic text.
//! Models get these wrong, so the calendar arithmetic stays in code.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Date {
    pub fn new(year: i32, month: u32, day: u32) -> Date {
        Date { year, month, day }
    }

    fn to_days(self) -> i64 {
        let y = i64::from(self.year) - i64::from(self.month <= 2);
        let era = y.div_euclid(400);
        let yoe = y.rem_euclid(400);
        let mp = (i64::from(self.month) + 9) % 12;
        let doy = (153 * mp + 2) / 5 + i64::from(self.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146097 + doe - 719468
    }

    fn from_days(days: i64) -> Date {
        let z = days + 719468;
        let era = z.div_euclid(146097);
        let doe = z.rem_euclid(146097);
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
        Date { year, month, day }
    }

    pub fn add_days(self, n: i64) -> Date {
        Date::from_days(self.to_days() + n)
    }

    /// 0 is Sunday, 6 is Saturday.
    pub fn weekday(self) -> u32 {
        (self.to_days() + 4).rem_euclid(7) as u32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved {
    pub date: Date,
    /// Hour and minute in 24-hour time, when the text gives one.
    pub time: Option<(u32, u32)>,
    /// True for "every Friday" or "كل جمعة".
    pub weekly: bool,
}

impl Resolved {
    /// Local ISO 8601, without a time when the text gave none.
    pub fn to_iso(&self) -> String {
        let d = self.date;
        match self.time {
            Some((h, m)) => format!("{:04}-{:02}-{:02}T{:02}:{:02}:00", d.year, d.month, d.day, h, m),
            None => format!("{:04}-{:02}-{:02}", d.year, d.month, d.day),
        }
    }
}

#[derive(Clone, Copy)]
enum DayPart {
    Morning,
    Noon,
    Afternoon,
    Evening,
    Night,
}

impl DayPart {
    fn default_hour(self) -> u32 {
        match self {
            DayPart::Morning => 9,
            DayPart::Noon => 12,
            DayPart::Afternoon => 15,
            DayPart::Evening => 18,
            DayPart::Night => 21,
        }
    }

    fn to_24h(self, hour: u32) -> u32 {
        match self {
            DayPart::Morning => hour,
            DayPart::Noon if (1..=6).contains(&hour) => hour + 12,
            DayPart::Noon => hour,
            _ if hour < 12 => hour + 12,
            _ => hour,
        }
    }
}

/// Lowercases and folds Arabic spelling variants, so that غدًا matches غدا
/// and ٢ matches 2.
fn normalize(text: &str) -> String {
    text.chars()
        .filter(|c| !('\u{064B}'..='\u{0652}').contains(c))
        .map(|c| match c {
            'أ' | 'إ' | 'آ' => 'ا',
            'ة' => 'ه',
            'ى' => 'ي',
            '٠'..='٩' => char::from_u32(c as u32 - 0x0660 + 0x30).unwrap_or(c),
            _ => c,
        })
        .flat_map(char::to_lowercase)
        .collect()
}

fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for word in text.split(|c: char| !(c.is_alphanumeric() || c == ':')) {
        let word = word.trim_matches(':');
        if word.is_empty() {
            continue;
        }
        // "10am" is treated the same as "10 am".
        match word.strip_suffix("am").or_else(|| word.strip_suffix("pm")) {
            Some(n) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit() || c == ':') => {
                out.push(n.to_string());
                out.push(word[n.len()..].to_string());
            }
            _ => out.push(word.to_string()),
        }
    }
    out
}

fn weekday_of(word: &str) -> Option<u32> {
    match word {
        "sunday" | "الاحد" | "احد" => Some(0),
        "monday" | "الاثنين" | "اثنين" => Some(1),
        "tuesday" | "الثلاثاء" | "ثلاثاء" => Some(2),
        "wednesday" | "الاربعاء" | "اربعاء" => Some(3),
        "thursday" | "الخميس" | "خميس" => Some(4),
        "friday" | "الجمعه" | "جمعه" => Some(5),
        "saturday" | "السبت" | "سبت" => Some(6),
        _ => None,
    }
}

fn day_part_of(word: &str) -> Option<DayPart> {
    match word {
        "morning" | "am" | "صباحا" | "صباح" | "الصباح" | "الصبح" | "الصبحيه" => Some(DayPart::Morning),
        "noon" | "ظهرا" | "ظهر" | "الظهر" => Some(DayPart::Noon),
        "afternoon" | "pm" | "عصرا" | "عصر" | "العصر" => Some(DayPart::Afternoon),
        "evening" | "مساء" | "المساء" => Some(DayPart::Evening),
        "night" | "tonight" | "ليلا" | "الليل" | "الليله" => Some(DayPart::Night),
        _ => None,
    }
}

fn arabic_hour(word: &str) -> Option<u32> {
    match word {
        "الواحده" => Some(1),
        "الثانيه" => Some(2),
        "الثالثه" => Some(3),
        "الرابعه" => Some(4),
        "الخامسه" => Some(5),
        "السادسه" => Some(6),
        "السابعه" => Some(7),
        "الثامنه" => Some(8),
        "التاسعه" => Some(9),
        "العاشره" => Some(10),
        "الحاديه" => Some(11),
        _ => None,
    }
}

fn clock(word: &str) -> Option<(u32, u32)> {
    let (h, m) = word.split_once(':').unwrap_or((word, "0"));
    let time = (h.parse().ok()?, m.parse().ok()?);
    (time.0 < 24 && time.1 < 60).then_some(time)
}

/// Finds the first date and time mentioned in `text`, relative to `today`.
/// A bare weekday means its next occurrence, never today. Returns None when
/// the text mentions neither.
pub fn resolve(text: &str, today: Date) -> Option<Resolved> {
    let words = tokens(&normalize(text));
    let mut offset = None;
    let mut time = None;
    let mut part = None;
    let mut weekly = false;

    for (i, word) in words.iter().enumerate() {
        let next = words.get(i + 1).map(String::as_str);
        let prev = i.checked_sub(1).map(|p| words[p].as_str());
        let after = prev == Some("بعد") || prev == Some("after");

        match word.as_str() {
            "every" | "كل" => weekly = true,
            "today" | "اليوم" | "tonight" | "الليله" if offset.is_none() => offset = Some(0),
            "tomorrow" | "غدا" | "غد" | "بكره" | "باكر" if offset.is_none() || after => {
                offset = Some(if after { 2 } else { 1 });
            }
            _ => {}
        }
        if let (None, Some(day)) = (offset, weekday_of(word)) {
            let ahead = (i64::from(day) - i64::from(today.weekday())).rem_euclid(7);
            offset = Some(if ahead == 0 { 7 } else { ahead });
        }
        if part.is_none() {
            part = day_part_of(word);
        }
        if time.is_some() {
            continue;
        }
        let marked = matches!(prev, Some("at") | Some("الساعه"));
        if prev == Some("الساعه") {
            if let Some(hour) = arabic_hour(word) {
                // الثانية عشرة is 12, الحادية عشرة is 11, العاشرة alone is 10.
                time = Some(match (hour, next) {
                    (2, Some("عشره")) => (12, 0),
                    _ => (hour, 0),
                });
                continue;
            }
        }
        if marked || next.map_or(false, |n| day_part_of(n).is_some()) {
            time = clock(word);
        }
    }

    if offset.is_none() && time.is_none() && part.is_none() {
        return None;
    }
    let time = match (time, part) {
        (Some((h, m)), Some(p)) => {
            let twelve_am = matches!(p, DayPart::Morning) && h == 12;
            Some((if twelve_am { 0 } else { p.to_24h(h) }, m))
        }
        (Some(t), None) => Some(t),
        (None, Some(p)) => Some((p.default_hour(), 0)),
        (None, None) => None,
    };
    Some(Resolved { date: today.add_days(offset.unwrap_or(0)), time, weekly })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Friday, the day of the baseline run where the model got all of these wrong.
    const TODAY: Date = Date { year: 2026, month: 10, day: 30 };

    fn iso(text: &str) -> String {
        resolve(text, TODAY).expect("should resolve").to_iso()
    }

    #[test]
    fn weekday_is_correct() {
        assert_eq!(TODAY.weekday(), 5);
        assert_eq!(Date::new(1970, 1, 1).weekday(), 4);
    }

    #[test]
    fn crosses_month_year_and_leap_day() {
        assert_eq!(Date::new(2026, 10, 31).add_days(1), Date::new(2026, 11, 1));
        assert_eq!(Date::new(2026, 12, 31).add_days(1), Date::new(2027, 1, 1));
        assert_eq!(Date::new(2028, 2, 28).add_days(1), Date::new(2028, 2, 29));
        assert_eq!(Date::new(2027, 2, 28).add_days(1), Date::new(2027, 3, 1));
    }

    #[test]
    fn baseline_failures_english() {
        assert_eq!(iso("Remind me to call the bank tomorrow at 10 am."), "2026-10-31T10:00:00");
        assert_eq!(
            iso("Schedule a meeting called Project Review with Sara and Omar on Sunday at 2 pm."),
            "2026-11-01T14:00:00"
        );
    }

    #[test]
    fn baseline_failures_arabic() {
        assert_eq!(iso("ذكّرني بالاتصال بالبنك غدًا الساعة العاشرة صباحًا."), "2026-10-31T10:00:00");
        assert_eq!(
            iso("حدّد اجتماعًا باسم مراجعة المشروع مع سارة وعمر يوم الأحد الساعة الثانية ظهرًا."),
            "2026-11-01T14:00:00"
        );
    }

    #[test]
    fn baseline_failures_mixed_and_dialect() {
        assert_eq!(iso("ممكن تعمل schedule لـ meeting مع الـ team يوم الخميس الساعة 11؟"), "2026-11-05T11:00:00");
        assert_eq!(iso("عايز أفتكر أكلم المهندس بكرة الصبح."), "2026-10-31T09:00:00");
    }

    #[test]
    fn weekly_is_flagged() {
        let r = resolve("ذكّرني أعمل backup للـ laptop كل جمعة.", TODAY).unwrap();
        assert!(r.weekly);
        assert_eq!(r.date, Date::new(2026, 11, 6));
        assert!(resolve("back up every Friday", TODAY).unwrap().weekly);
    }

    #[test]
    fn same_weekday_means_next_week() {
        assert_eq!(iso("on Friday"), "2026-11-06");
    }

    #[test]
    fn day_after_tomorrow() {
        assert_eq!(iso("the day after tomorrow"), "2026-11-01");
        assert_eq!(iso("بعد غد"), "2026-11-01");
        assert_eq!(iso("بعد بكرة"), "2026-11-01");
    }

    #[test]
    fn clock_forms() {
        assert_eq!(iso("tomorrow at 14:30"), "2026-10-31T14:30:00");
        assert_eq!(iso("tomorrow 3pm"), "2026-10-31T15:00:00");
        assert_eq!(iso("tomorrow at 12 am"), "2026-10-31T00:00:00");
        assert_eq!(iso("tomorrow at 12 pm"), "2026-10-31T12:00:00");
        assert_eq!(iso("غدا الساعة ١١ صباحا"), "2026-10-31T11:00:00");
        assert_eq!(iso("غدا الساعة الثانية عشرة ظهرا"), "2026-10-31T12:00:00");
        assert_eq!(iso("اليوم الساعة 5 مساء"), "2026-10-30T17:00:00");
    }

    #[test]
    fn plain_numbers_are_not_times() {
        assert_eq!(resolve("Find the file called budget 2026.", TODAY), None);
        assert_eq!(resolve("ابحث عن الملف الذي اسمه ميزانية 2026.", TODAY), None);
    }
}
