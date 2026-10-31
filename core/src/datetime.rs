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
    pub repeat: Option<Repeat>,
    /// True when the time could mean two things, such as "at 2" or "الساعة 11".
    /// The caller should ask the user instead of trusting `time`.
    pub ambiguous: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repeat {
    Daily,
    Weekly,
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
            _ if hour == 0 => 0,
            DayPart::Morning | DayPart::Evening | DayPart::Night if hour == 12 => 0,
            DayPart::Morning => hour,
            DayPart::Night if hour < 6 => hour,
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

/// The weekday a word names, from MSA, Egyptian and Gulf spellings, with and
/// without ال. Forms without ال only count after يوم or كل: alone, احد means
/// "someone" and سبت means "basket". الحد (the limit) needs it too, and حد
/// (someone) needs يوم, because كل حد is "everyone".
fn weekday_form(word: &str, prev: Option<&str>) -> Option<u32> {
    let named = matches!(prev, Some("يوم") | Some("كل"));
    match word {
        "sunday" | "الاحد" => Some(0),
        "monday" | "الاثنين" | "الاتنين" | "التنين" | "الثنين" => Some(1),
        "tuesday" | "الثلاثاء" | "الثلاثا" | "التلات" | "التلاتا" => Some(2),
        "wednesday" | "الاربعاء" | "الاربعا" | "الاربع" => Some(3),
        "thursday" | "الخميس" => Some(4),
        "friday" | "الجمعه" => Some(5),
        "saturday" | "السبت" => Some(6),
        "احد" | "الحد" if named => Some(0),
        "حد" if prev == Some("يوم") => Some(0),
        "اثنين" | "اتنين" | "تنين" | "ثنين" if named => Some(1),
        "ثلاثاء" | "ثلاثا" | "تلات" | "تلاتا" if named => Some(2),
        "اربعاء" | "اربعا" | "اربع" if named => Some(3),
        "خميس" if named => Some(4),
        "جمعه" if named => Some(5),
        "سبت" if named => Some(6),
        _ => None,
    }
}

/// Units that follow a number, which make تلات and اربع a count ("كل اربع
/// ساعات" is every four hours, not every Wednesday).
const COUNTED: &[&str] = &[
    "ساعه", "ساعات", "ساعتين", "دقيقه", "دقايق", "دقائق", "دقيقتين", "ايام", "يومين", "اسبوع", "اسابيع", "شهر",
    "شهور", "اشهر", "سنه", "سنين", "سنوات", "مره", "مرات",
];

/// Weekday spellings that are also numbers (two, three, four), so a unit after
/// them can make them a count. الخميس and the other names never are.
const NUMBER_FORMS: &[&str] = &["اثنين", "اتنين", "تنين", "ثنين", "ثلاثا", "تلات", "تلاتا", "اربع", "اربعا"];

/// The same with ال, which only counts before a plural unit: "التلات ساعات" is
/// the three hours, but "التلات ساعة 5" is Tuesday at 5.
const DEFINITE_NUMBER_FORMS: &[&str] = &["الاتنين", "التنين", "الثنين", "التلات", "التلاتا", "الاربع", "الاربعا"];

/// The weekday named by `words[i]`, as 0 for Sunday to 6 for Saturday. A
/// number form before a unit is a count ("كل اربع ساعات", "بعد تلات ساعات"),
/// unless يوم comes first: "يوم التلات ساعة 5" names a day.
pub(crate) fn weekday_at(words: &[String], i: usize) -> Option<u32> {
    let word = words.get(i)?.as_str();
    let prev = i.checked_sub(1).map(|p| words[p].as_str());
    let unit = words.get(i + 1).map(String::as_str).filter(|n| COUNTED.contains(n));
    let counted = prev != Some("يوم")
        && unit.is_some()
        && (NUMBER_FORMS.contains(&word) || (DEFINITE_NUMBER_FORMS.contains(&word) && unit != Some("ساعه")));
    weekday_form(word, prev).filter(|_| !counted)
}

/// True after the word for a prayer or its call. The names that follow share
/// words with times of day, but they are not times.
pub(crate) fn names_prayer(prev: Option<&str>) -> bool {
    matches!(prev, Some("صلاه") | Some("اذان"))
}

/// Reads what follows "every" or "كل". "كل شيء" (everything) is not a repeat.
fn repeat_after(words: &[String], i: usize) -> Option<Repeat> {
    let next = words.get(i + 1).map(String::as_str)?;
    if next == "day" || next == "يوم" {
        return Some(if weekday_at(words, i + 2).is_some() { Repeat::Weekly } else { Repeat::Daily });
    }
    weekday_at(words, i + 1).map(|_| Repeat::Weekly)
}

fn day_part_of(word: &str) -> Option<DayPart> {
    match word {
        "morning" | "am" | "صباحا" | "صباح" | "الصباح" | "الصبح" | "الصبحيه" => Some(DayPart::Morning),
        "noon" | "ظهرا" | "ظهر" | "الظهر" | "ضهرا" | "ضهر" | "الضهر" => Some(DayPart::Noon),
        "afternoon" | "pm" | "عصرا" | "عصر" | "العصر" => Some(DayPart::Afternoon),
        "evening" | "مساء" | "المساء" => Some(DayPart::Evening),
        "night" | "tonight" | "ليلا" | "الليل" | "بالليل" | "الليله" => Some(DayPart::Night),
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

/// True when `words[i]` says that the number after it is a time: الساعة, or
/// ساعة with no article ("يوم الخميس ساعة 5"). Without the article it must be
/// followed by an hour, and it is a count when anything makes it one: "بعد
/// ساعة", "كل ساعة", "ساعة واحدة", "اربع ساعة 5", "ساعة 5 دقايق".
pub(crate) fn hour_marker(words: &[String], i: usize) -> bool {
    match words.get(i).map(String::as_str) {
        Some("الساعه") => return true,
        Some("ساعه") => {}
        _ => return false,
    }
    let Some(hour) = words.get(i + 1) else { return false };
    if clock(hour).is_none() && arabic_hour(hour).is_none() {
        return false;
    }
    if words.get(i + 2).is_some_and(|unit| COUNTED.contains(&unit.as_str())) {
        return false;
    }
    let Some(p) = i.checked_sub(1) else { return true };
    let prev = words[p].as_str();
    let counted_number = prev.bytes().all(|b| b.is_ascii_digit())
        || ((NUMBER_FORMS.contains(&prev) || DEFINITE_NUMBER_FORMS.contains(&prev)) && weekday_at(words, p).is_none());
    !(counted_number || matches!(prev, "بعد" | "عقب" | "كل" | "after" | "every" | "نص" | "نصف" | "ربع" | "ثلث"))
}

fn clock(word: &str) -> Option<(u32, u32)> {
    let (h, m) = word.split_once(':').unwrap_or((word, "0"));
    let time = (h.parse().ok()?, m.parse().ok()?);
    (time.0 < 24 && time.1 < 60).then_some(time)
}

/// Finds the first date and time mentioned in `text`, relative to `today`.
/// A bare weekday means its next occurrence, never today. A part of the day
/// alone is not enough, because صباح الخير and "great morning" are not plans.
/// Returns None when the text gives no date or time. Pass only text already
/// known to be about scheduling: "Today I ate lunch" still resolves to today.
pub fn resolve(text: &str, today: Date) -> Option<Resolved> {
    let words = tokens(&normalize(text));
    let mut offset = None;
    let mut time = None;
    let mut part = None;
    let mut repeat = None;

    for (i, word) in words.iter().enumerate() {
        let next = words.get(i + 1).map(String::as_str);
        let prev = i.checked_sub(1).map(|p| words[p].as_str());
        let after = matches!(prev, Some("بعد") | Some("عقب") | Some("after"));

        match word.as_str() {
            "every" | "كل" => repeat = repeat.or(repeat_after(&words, i)),
            "today" | "اليوم" | "النهارده" | "انهارده" | "tonight" | "الليله" if offset.is_none() => {
                offset = Some(0);
            }
            "tomorrow" | "غدا" | "غد" | "بكره" | "بكرا" | "باكر" | "باجر" if offset.is_none() || after => {
                offset = Some(if after { 2 } else { 1 });
            }
            _ => {}
        }
        if let (None, Some(day)) = (offset, weekday_at(&words, i)) {
            let ahead = (i64::from(day) - i64::from(today.weekday())).rem_euclid(7);
            offset = Some(if ahead == 0 { 7 } else { ahead });
        }
        if part.is_none() && next != Some("الخير") && !names_prayer(prev) {
            part = day_part_of(word);
        }
        if time.is_some() {
            continue;
        }
        let after_marker = i > 0 && hour_marker(&words, i - 1);
        let marked = prev == Some("at") || after_marker;
        if after_marker {
            if let Some(hour) = arabic_hour(word) {
                // الثانية عشرة is 12, الحادية عشرة is 11, العاشرة alone is 10.
                time = Some(match (hour, next) {
                    (2, Some("عشره")) => (12, 0),
                    _ => (hour, 0),
                });
                continue;
            }
        }
        if marked || next.is_some_and(|n| day_part_of(n).is_some()) {
            time = clock(word);
        }
    }

    if offset.is_none() && time.is_none() {
        return None;
    }
    // A named day keeps its own date: بكرة الساعة 12 بالليل is 00:00 tomorrow.
    // Tonight or no day at all moves on one day, because that hour has passed.
    let (time, extra_day, ambiguous) = match (time, part) {
        (Some((h, m)), Some(p)) => {
            let small_hours = match p {
                DayPart::Night => h == 12 || (1..6).contains(&h),
                DayPart::Evening => h == 12,
                _ => false,
            };
            let rolls_over = small_hours && offset.unwrap_or(0) == 0;
            (Some((p.to_24h(h), m)), i64::from(rolls_over), false)
        }
        (Some((h, m)), None) => (Some((h, m)), 0, (1..=12).contains(&h)),
        (None, Some(p)) => (Some((p.default_hour(), 0)), 0, false),
        (None, None) => (None, 0, false),
    };
    let date = today.add_days(offset.unwrap_or(0) + extra_day);
    Some(Resolved { date, time, repeat, ambiguous })
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
    fn repeats_are_flagged() {
        let r = resolve("ذكّرني أعمل backup للـ laptop كل جمعة.", TODAY).unwrap();
        assert_eq!(r.repeat, Some(Repeat::Weekly));
        assert_eq!(r.date, Date::new(2026, 11, 6));
        let repeat = |text| resolve(text, TODAY).unwrap().repeat;
        assert_eq!(repeat("back up every Friday"), Some(Repeat::Weekly));
        assert_eq!(repeat("كل يوم جمعة"), Some(Repeat::Weekly));
        assert_eq!(repeat("كل يوم الساعة 9 صباحا"), Some(Repeat::Daily));
        assert_eq!(repeat("every day at 9 am"), Some(Repeat::Daily));
    }

    #[test]
    fn every_without_a_day_is_not_a_repeat() {
        assert_eq!(resolve("كل شيء تمام", TODAY), None);
        let r = resolve("كل شيء تمام يوم الجمعة", TODAY).unwrap();
        assert_eq!(r.repeat, None);
        assert_eq!(r.date, Date::new(2026, 11, 6));
    }

    #[test]
    fn ordinary_sentences_are_not_dates() {
        for text in [
            "I had a great morning",
            "صباح الخير",
            "مساء الخير",
            "هل كلمك احد؟",
            "العصر الحجري",
            "عندي سبت كبير",
            "Find file 5.",
            "open chapter 3",
            "meeting 2 at home",
        ] {
            assert_eq!(resolve(text, TODAY), None, "{text}");
        }
    }

    #[test]
    fn greeting_does_not_set_the_time() {
        assert_eq!(iso("صباح الخير، الاجتماع بكرة"), "2026-10-31");
    }

    #[test]
    fn dialect_words() {
        assert_eq!(iso("النهاردة الساعة 5 مساء"), "2026-10-30T17:00:00");
        assert_eq!(iso("بكرة الساعة 2 الضهر"), "2026-10-31T14:00:00");
        assert_eq!(iso("عقب باكر"), "2026-11-01");
        assert_eq!(iso("باجر"), "2026-10-31");
        assert_eq!(iso("يوم احد"), "2026-11-01");
    }

    #[test]
    fn night_hours_after_midnight() {
        assert_eq!(iso("بكرة الساعة 12 بالليل"), "2026-10-31T00:00:00");
        assert_eq!(iso("غدا الساعة 12 ليلا"), "2026-10-31T00:00:00");
        assert_eq!(iso("بكرة الساعة 2 بالليل"), "2026-10-31T02:00:00");
        assert_eq!(iso("يوم الأحد الساعة 12 بالليل"), "2026-11-01T00:00:00");
        assert_eq!(iso("tomorrow at 11 at night"), "2026-10-31T23:00:00");
        // Said without a later day, the hour has already passed today.
        assert_eq!(iso("الساعة 12 بالليل"), "2026-10-31T00:00:00");
        assert_eq!(iso("اليوم الساعة 2 بالليل"), "2026-10-31T02:00:00");
        // Native speaker: 12 مساء is midnight, not noon.
        assert_eq!(iso("غدا الساعة 12 مساء"), "2026-10-31T00:00:00");
        assert_eq!(iso("الساعة 12 مساء"), "2026-10-31T00:00:00");
        assert!(!resolve("بكرة الساعة 12 بالليل", TODAY).unwrap().ambiguous);
    }

    #[test]
    fn unclear_times_are_flagged() {
        let ambiguous = |text| resolve(text, TODAY).unwrap().ambiguous;
        assert!(ambiguous("الساعة 11"));
        assert!(ambiguous("meet at 2"));
        assert!(ambiguous("موعدنا الساعة الثانية"));
        assert!(!ambiguous("غدا الساعة 12 مساء"));
        assert!(!ambiguous("tomorrow at 14:30"));
        assert!(!ambiguous("tomorrow at 2 pm"));
        assert!(!ambiguous("بكرة الصبح"));
    }

    #[test]
    fn twenty_four_hour_times_survive_a_day_part() {
        assert_eq!(iso("tomorrow at 14:00 pm"), "2026-10-31T14:00:00");
        assert_eq!(iso("غدا الساعة 13:00 ظهرا"), "2026-10-31T13:00:00");
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

    #[test]
    fn egyptian_and_gulf_weekdays_with_al() {
        // TODAY is Friday 2026-10-30.
        for (text, date) in [
            ("يوم الحد", "2026-11-01"),
            ("الاحد", "2026-11-01"),
            ("التنين الساعة 3 العصر", "2026-11-02T15:00:00"),
            ("الاتنين", "2026-11-02"),
            ("الثنين", "2026-11-02"),
            ("الاثنين", "2026-11-02"),
            ("التلات", "2026-11-03"),
            ("التلات الساعة 10 الصبح", "2026-11-03T10:00:00"),
            ("الثلاثا", "2026-11-03"),
            ("الثلاثاء", "2026-11-03"),
            ("الأربع", "2026-11-04"),
            ("الاربعا", "2026-11-04"),
            ("الخميس", "2026-11-05"),
            ("السبت", "2026-10-31"),
        ] {
            assert_eq!(iso(text), date, "{text}");
        }
    }

    #[test]
    fn egyptian_and_gulf_weekdays_without_al_need_a_day_word() {
        for (text, date) in [
            ("يوم حد", "2026-11-01"),
            ("يوم تنين", "2026-11-02"),
            ("يوم اتنين", "2026-11-02"),
            ("يوم تلات", "2026-11-03"),
            ("يوم تلات الساعة 4 العصر", "2026-11-03T16:00:00"),
            ("يوم ثلاثا", "2026-11-03"),
            ("يوم اربع", "2026-11-04"),
            ("يوم اربعا", "2026-11-04"),
            ("يوم خميس", "2026-11-05"),
            ("يوم جمعة", "2026-11-06"),
            ("يوم سبت", "2026-10-31"),
        ] {
            assert_eq!(iso(text), date, "{text}");
        }
        for text in ["تلات", "اربع", "تنين", "حد", "الحد الاقصى", "كل حد عارف", "اربع حاجات"] {
            assert_eq!(resolve(text, TODAY), None, "{text}");
        }
    }

    #[test]
    fn egyptian_weekdays_repeat_and_counts_do_not() {
        let r = resolve("كل تلات الساعة 9 الصبح", TODAY).unwrap();
        assert_eq!((r.repeat, r.date), (Some(Repeat::Weekly), Date::new(2026, 11, 3)));
        assert_eq!(resolve("كل يوم تنين", TODAY).unwrap().repeat, Some(Repeat::Weekly));
        assert_eq!(resolve("كل اربع ساعات", TODAY), None);
        assert_eq!(resolve("كل تلات ايام", TODAY), None);
        assert_eq!(resolve("التلات ساعات", TODAY), None);
    }

    fn date_of(text: &str) -> String {
        let d = resolve(text, TODAY).unwrap_or_else(|| panic!("{text}")).date;
        format!("{:04}-{:02}-{:02}", d.year, d.month, d.day)
    }

    #[test]
    fn a_weekday_before_the_hour_keeps_its_date() {
        // Today is Friday 30 October 2026.
        for (day, date) in [
            ("الاحد", "2026-11-01"),
            ("الاثنين", "2026-11-02"),
            ("الاتنين", "2026-11-02"),
            ("التنين", "2026-11-02"),
            ("الثلاثاء", "2026-11-03"),
            ("التلات", "2026-11-03"),
            ("التلاتا", "2026-11-03"),
            ("الاربعاء", "2026-11-04"),
            ("الاربعا", "2026-11-04"),
            ("الاربع", "2026-11-04"),
            ("الخميس", "2026-11-05"),
            ("الجمعة", "2026-11-06"),
            ("السبت", "2026-10-31"),
            ("تلات", "2026-11-03"),
            ("تلاتا", "2026-11-03"),
            ("ثلاثا", "2026-11-03"),
            ("اربع", "2026-11-04"),
            ("اربعا", "2026-11-04"),
            ("اربعاء", "2026-11-04"),
            ("تنين", "2026-11-02"),
            ("اتنين", "2026-11-02"),
            ("اثنين", "2026-11-02"),
            ("ثنين", "2026-11-02"),
            ("خميس", "2026-11-05"),
            ("جمعة", "2026-11-06"),
            ("سبت", "2026-10-31"),
            ("احد", "2026-11-01"),
        ] {
            for unit in ["ساعة", "ساعات", "ساعتين"] {
                let text = format!("ذكرني يوم {day} {unit} 5");
                assert_eq!(date_of(&text), date, "{text}");
            }
        }
        let thursday = resolve("ذكرني يوم الخميس ساعة 5", TODAY).unwrap();
        assert_eq!(thursday.date, Date::new(2026, 11, 5));
        assert_eq!(thursday.repeat, None);
        for (text, date) in [
            ("remind me Thursday", "2026-11-05"),
            ("الخميس ساعة 5", "2026-11-05"),
            ("الخميس الساعة 5 العصر", "2026-11-05"),
            ("التلات ساعة 5", "2026-11-03"),
            ("الاربع ساعة 5", "2026-11-04"),
            ("كل خميس ساعة 5", "2026-11-05"),
            ("كل يوم تلات ساعة 5", "2026-11-03"),
        ] {
            assert_eq!(date_of(text), date, "{text}");
        }
        assert_eq!(resolve("كل خميس ساعة 5", TODAY).unwrap().repeat, Some(Repeat::Weekly));
        assert_eq!(resolve("كل يوم تلات ساعة 5", TODAY).unwrap().repeat, Some(Repeat::Weekly));
    }

    #[test]
    fn an_hour_word_without_the_article_reads_a_time_like_the_one_with_it() {
        for (plain, with_article) in [
            ("ساعة 5", "الساعة 5"),
            ("ساعة ٥", "الساعة ٥"),
            ("ساعة 5:30", "الساعة 5:30"),
            ("ساعة 5 ونص", "الساعة 5 ونص"),
            ("ساعة الخامسة", "الساعة الخامسة"),
            ("ساعة الثانية عشرة", "الساعة الثانية عشرة"),
            ("بكرة ساعة 5 العصر", "بكرة الساعة 5 العصر"),
            ("بكرة ساعة ٧ مساء", "بكرة الساعة ٧ مساء"),
            ("يوم الخميس ساعة 5", "يوم الخميس الساعة 5"),
            ("التلات ساعة ٥ العصر", "التلات الساعة ٥ العصر"),
            ("كل خميس ساعة 5", "كل خميس الساعة 5"),
            ("بكرة ساعة 12 بالليل", "بكرة الساعة 12 بالليل"),
        ] {
            assert_eq!(resolve(plain, TODAY), resolve(with_article, TODAY), "{plain}");
        }
        assert_eq!(iso("ذكرني يوم الخميس ساعة 5"), "2026-11-05T05:00:00");
        assert_eq!(iso("بكرة ساعة ٥ العصر"), "2026-10-31T17:00:00");
        assert_eq!(iso("بكرة ساعة الخامسة"), "2026-10-31T05:00:00");
        // With no day part it is as unclear as الساعة 5.
        assert!(resolve("ساعة 5", TODAY).unwrap().ambiguous);
        assert!(resolve("يوم الخميس ساعة 5", TODAY).unwrap().ambiguous);
        assert!(!resolve("بكرة ساعة 5 العصر", TODAY).unwrap().ambiguous);
        assert!(!resolve("بكرة ساعة 17:00", TODAY).unwrap().ambiguous);
    }

    #[test]
    fn an_hour_word_that_counts_is_not_a_time() {
        for text in ["بعد ساعة", "بعد 5 ساعات", "ساعة واحدة", "كل ساعة", "كل ساعة 5", "بعد ساعة 5 دقايق", "كل اربع ساعة 5"] {
            assert_eq!(resolve(text, TODAY), None, "{text}");
        }
        assert_eq!(resolve("يوم الخميس ساعة واحدة", TODAY).unwrap().time, None);
    }

    #[test]
    fn number_forms_before_a_unit_stay_counts() {
        for text in [
            "كل اربع ساعات",
            "كل تلات ساعات",
            "كل تلات ساعة",
            "كل اتنين ساعتين",
            "كل تنين دقايق",
            "كل اربعا ايام",
            "بعد تلات ساعات",
            "بعد اربع ساعات",
            "بعد تلاتا ساعات",
            "ذكرني بعد اربع ساعات",
            "التلات ساعات",
            "الاربع ساعات",
        ] {
            assert_eq!(resolve(text, TODAY), None, "{text}");
        }
        // A count never sets the date or the repeat of a day named elsewhere.
        let r = resolve("كل اربع ساعات يوم الخميس", TODAY).unwrap();
        assert_eq!((r.repeat, r.date), (None, Date::new(2026, 11, 5)));
    }

    #[test]
    fn midnight_is_pinned() {
        assert_eq!(iso("بكرة الساعة 12 بالليل"), "2026-10-31T00:00:00");
        assert_eq!(iso("الساعة 12 مساء"), "2026-10-31T00:00:00");
        assert_eq!(iso("بكرة الساعة 12 مساء"), "2026-10-31T00:00:00");
    }

    #[test]
    fn prayer_names_are_not_times_of_day() {
        // A prayer name must not give a time: الساعة 2 stays unclear, and
        // بعد صلاة العصر alone has no hour.
        assert!(resolve("بكرة بعد صلاة العصر", TODAY).unwrap().time.is_none());
        assert!(resolve("بكرة بعد صلاة الظهر", TODAY).unwrap().time.is_none());
        assert!(resolve("بكرة بعد اذان الضهر", TODAY).unwrap().time.is_none());
        assert_eq!(resolve("بعد صلاة العصر", TODAY), None);
        let r = resolve("بكرة الساعة 2 بعد صلاة الظهر", TODAY).unwrap();
        assert_eq!(r.time, Some((2, 0)));
        assert!(r.ambiguous);
        // Prayer names that were never time words stay that way.
        for text in ["بعد صلاة المغرب", "قبل صلاة الفجر", "بعد صلاة العشاء", "after the Maghrib prayer", "before Fajr"] {
            assert_eq!(resolve(text, TODAY), None, "{text}");
        }
        // The same words still count as times of day on their own.
        assert_eq!(iso("بكرة الساعة 2 الضهر"), "2026-10-31T14:00:00");
    }
}
