/// Languages ScarfeeOS supports as main languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
    Arabic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Ltr,
    Rtl,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::English, Language::Arabic];

    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Arabic => "ar",
        }
    }

    pub fn native_name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Arabic => "العربية",
        }
    }

    pub fn direction(self) -> Direction {
        match self {
            Language::English => Direction::Ltr,
            Language::Arabic => Direction::Rtl,
        }
    }

    /// Parses a code such as "en", "ar" or a locale like "ar_SA.UTF-8".
    pub fn from_code(s: &str) -> Option<Language> {
        let base = s.split(['_', '-', '.']).next()?;
        match base.to_ascii_lowercase().as_str() {
            "en" => Some(Language::English),
            "ar" => Some(Language::Arabic),
            _ => None,
        }
    }
}

fn is_arabic_letter(c: char) -> bool {
    c.is_alphabetic()
        && matches!(c, '\u{0600}'..='\u{06FF}' | '\u{0750}'..='\u{077F}' | '\u{08A0}'..='\u{08FF}' | '\u{FB50}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}')
}

fn is_latin_letter(c: char) -> bool {
    c.is_alphabetic() && (c.is_ascii() || ('\u{00C0}'..='\u{024F}').contains(&c))
}

/// True when `text` has at least one letter in the script of `language`.
pub fn has_letters(text: &str, language: Language) -> bool {
    match language {
        Language::English => text.chars().any(is_latin_letter),
        Language::Arabic => text.chars().any(is_arabic_letter),
    }
}

/// The language of a text written only in Arabic letters or only in Latin
/// letters. `None` when it has both, or neither. Digits and punctuation do not count.
pub fn pure_script(text: &str) -> Option<Language> {
    match (has_letters(text, Language::English), has_letters(text, Language::Arabic)) {
        (true, false) => Some(Language::English),
        (false, true) => Some(Language::Arabic),
        _ => None,
    }
}

/// Words said before the command itself ("please", "ممكن").
const POLITE: &[&str] = &["please", "hey", "can", "could", "would", "you", "ممكن", "طيب", "لو", "سمحت", "فضلك"];

/// Commands that ask for a reminder or a meeting, as normalized words.
const COMMANDS: &[&[&str]] = &[
    &["remind", "me"],
    &["set", "a", "reminder"],
    &["create", "a", "reminder"],
    &["schedule", "a", "meeting"],
    &["schedule", "meeting"],
    &["set", "up", "a", "meeting"],
    &["create", "a", "meeting"],
    &["book", "a", "meeting"],
    &["ذكرني"],
    &["ذكريني"],
    &["فكرني"],
    &["عايز", "افتكر"],
    &["عاوز", "افتكر"],
    &["ابغي", "افتكر"],
    &["ابي", "افتكر"],
    &["حدد", "اجتماعا"],
    &["حدد", "اجتماع"],
    &["جدول", "اجتماعا"],
    &["اعمل", "اجتماع"],
];

/// Words that may follow a command and are not part of the text ("remind me to ...").
const GLUE: &[&str] = &["to", "about", "called", "named", "titled", "ان", "باسم", "بعنوان"];

/// The word without punctuation around it, folded so spelling variants match.
fn core_word(token: &str) -> String {
    let word = token.trim_matches(|c: char| !c.is_alphanumeric());
    crate::guard::normalize(word)
}

fn tidy(words: &[&str]) -> String {
    words.join(" ").trim_matches(|c: char| c.is_whitespace() || ",.;:!?،؛؟".contains(c)).to_string()
}

/// `text` without the command at its start ("Remind me to", "ذكرني", "Schedule
/// a meeting called"). Other words are kept as they are.
pub fn without_command_words(text: &str) -> String {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let words: Vec<String> = tokens.iter().map(|t| core_word(t)).collect();
    let mut start = 0;
    while words.get(start).is_some_and(|w| POLITE.contains(&w.as_str())) {
        start += 1;
    }
    let rest = &words[start..];
    let command = COMMANDS
        .iter()
        .filter(|c| c.len() <= rest.len() && c.iter().zip(rest).all(|(a, b)| a == b))
        .map(|c| c.len())
        .max();
    start += command.unwrap_or(0);
    if command.is_some() && words.get(start).is_some_and(|w| GLUE.contains(&w.as_str())) {
        start += 1;
    }
    tidy(&tokens[start..])
}

const DAY_WORDS: &[&str] = &[
    "today", "tomorrow", "tonight", "اليوم", "النهارده", "انهارده", "الليله", "غدا", "غد", "بكره", "بكرا", "باكر", "باجر",
    "sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "الاحد", "الاثنين", "الثلاثاء",
    "الاربعاء", "الخميس", "الجمعه", "السبت",
];

/// Weekdays without ال, which only count after يوم or كل.
const BARE_WEEKDAYS: &[&str] = &["احد", "اثنين", "ثلاثاء", "اربعاء", "خميس", "جمعه", "سبت"];

const PART_WORDS: &[&str] = &[
    "morning", "am", "noon", "afternoon", "pm", "evening", "night", "صباحا", "صباح", "الصباح", "الصبح", "الصبحيه", "ظهرا",
    "ظهر", "الظهر", "ضهرا", "ضهر", "الضهر", "عصرا", "عصر", "العصر", "مساء", "المساء", "ليلا", "الليل", "بالليل",
];

const HOUR_WORDS: &[&str] = &[
    "الواحده", "الثانيه", "الثالثه", "الرابعه", "الخامسه", "السادسه", "السابعه", "الثامنه", "التاسعه", "العاشره", "الحاديه",
];

/// Words that join a time to the text and only count next to a time.
const LINKS: &[&str] = &["at", "on", "in", "the", "this", "next", "every", "after", "الساعه", "يوم", "كل", "بعد", "في"];

fn is_clock(word: &str) -> bool {
    let digits = word.strip_suffix("am").or_else(|| word.strip_suffix("pm")).unwrap_or(word);
    let is_clock_time = digits.split_once(':').is_some_and(|(h, m)| {
        !h.is_empty() && !m.is_empty() && h.bytes().chain(m.bytes()).all(|b| b.is_ascii_digit())
    });
    is_clock_time || (digits.len() < word.len() && !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

/// Whether the word at `i` is a time on its own, without its links.
fn is_time_word(words: &[String], i: usize) -> bool {
    let word = words[i].as_str();
    let prev = i.checked_sub(1).map(|p| words[p].as_str());
    let next = words.get(i + 1).map(String::as_str);
    let before_part = next.is_some_and(|n| PART_WORDS.contains(&n));
    let after_marker = matches!(prev, Some("at") | Some("الساعه"));
    DAY_WORDS.contains(&word)
        || PART_WORDS.contains(&word)
        || (BARE_WEEKDAYS.contains(&word) && matches!(prev, Some("يوم") | Some("كل")))
        || is_clock(word)
        || (!word.is_empty() && word.bytes().all(|b| b.is_ascii_digit()) && (after_marker || before_part))
        || (HOUR_WORDS.contains(&word) && (prev == Some("الساعه") || before_part))
        || (word == "عشره" && i > 0 && HOUR_WORDS.contains(&words[i - 1].as_str()) && is_time_word(words, i - 1))
        || (word == "day" && prev == Some("every"))
        || (word == "يوم" && prev == Some("كل"))
}

/// `text` without the words that say when ("tomorrow at 10 am", "بكرة الصبح",
/// "كل جمعة"). With `trailing_only`, only a time at the end is removed, so
/// "call the bank tomorrow" keeps its middle. Other words, names included,
/// are kept as they are.
pub fn without_time_words(text: &str, trailing_only: bool) -> String {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let words: Vec<String> = tokens.iter().map(|t| core_word(t)).collect();
    let mut time = vec![false; words.len()];
    for i in (0..words.len()).rev() {
        time[i] = is_time_word(&words, i)
            || (LINKS.contains(&words[i].as_str()) && time.get(i + 1).copied().unwrap_or(false));
    }
    if trailing_only {
        let keep = time.iter().rposition(|t| !t).map_or(0, |last| last + 1);
        return tidy(&tokens[..keep]);
    }
    let kept: Vec<&str> = tokens.iter().zip(&time).filter(|(_, t)| !**t).map(|(w, _)| *w).collect();
    tidy(&kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_locales() {
        assert_eq!(Language::from_code("ar_SA.UTF-8"), Some(Language::Arabic));
        assert_eq!(Language::from_code("en-US"), Some(Language::English));
        assert_eq!(Language::from_code("fr"), None);
    }

    #[test]
    fn arabic_is_rtl() {
        assert_eq!(Language::Arabic.direction(), Direction::Rtl);
        assert_eq!(Language::English.direction(), Direction::Ltr);
    }

    #[test]
    fn pure_script_ignores_digits_and_needs_one_script() {
        assert_eq!(pure_script("ذكرني 10 بكرة"), Some(Language::Arabic));
        assert_eq!(pure_script("call Sam at 10"), Some(Language::English));
        assert_eq!(pure_script("call المهندس"), None);
        assert_eq!(pure_script("10:30"), None);
    }

    #[test]
    fn command_words_are_removed_only_at_the_start() {
        assert_eq!(without_command_words("Please remind me to remind Sam."), "remind Sam");
        assert_eq!(without_command_words("ذكّرني بالاتصال بالبنك"), "بالاتصال بالبنك");
        assert_eq!(without_command_words("Call Sam"), "Call Sam");
    }

    #[test]
    fn time_words_go_anywhere_or_only_at_the_end() {
        assert_eq!(without_time_words("tomorrow morning call Sam on Sunday at 2 pm", false), "call Sam");
        assert_eq!(without_time_words("tomorrow call Sam", true), "tomorrow call Sam");
        assert_eq!(without_time_words("اتصل بالبنك يوم الأحد الساعة الثانية عشرة ظهرا", true), "اتصل بالبنك");
        assert_eq!(without_time_words("الساعة 11", true), "");
    }
}
