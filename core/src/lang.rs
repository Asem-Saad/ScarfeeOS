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
    // English
    &["remind", "me"],
    &["set", "a", "reminder"],
    &["set", "reminder"],
    &["set", "me", "a", "reminder"],
    &["create", "a", "reminder"],
    &["create", "reminder"],
    &["add", "a", "reminder"],
    &["add", "reminder"],
    &["make", "a", "reminder"],
    &["i", "need", "to", "remember"],
    &["i", "want", "to", "remember"],
    &["dont", "let", "me", "forget"],
    &["schedule"],
    &["schedule", "a", "meeting"],
    &["schedule", "meeting"],
    &["set", "up", "a", "meeting"],
    &["set", "up", "meeting"],
    &["set", "a", "meeting"],
    &["create", "a", "meeting"],
    &["create", "meeting"],
    &["book", "a", "meeting"],
    &["book", "meeting"],
    &["add", "a", "meeting"],
    &["make", "a", "meeting"],
    &["arrange", "a", "meeting"],
    &["arrange", "meeting"],
    &["organize", "a", "meeting"],
    &["plan", "a", "meeting"],
    &["i", "need", "to", "schedule", "a", "meeting"],
    &["i", "want", "to", "schedule", "a", "meeting"],
    // Reminders: Modern Standard, then Egyptian, Gulf and Levantine
    &["ذكرني"],
    &["ذكريني"],
    &["نبهني"],
    &["نبهيني"],
    &["اضف", "تذكير"],
    &["اضف", "تذكيرا"],
    &["انشئ", "تذكير"],
    &["انشئ", "تذكيرا"],
    &["ضع", "تذكير"],
    &["ضع", "تذكيرا"],
    &["فكرني"],
    &["فكريني"],
    &["تفكرني"],
    &["تفكريني"],
    &["عايز", "افتكر"],
    &["عاوز", "افتكر"],
    &["عايزه", "افتكر"],
    &["عاوزه", "افتكر"],
    &["خليني", "افتكر"],
    &["اعمل", "تذكير"],
    &["اعملي", "تذكير"],
    &["اعمل", "لي", "تذكير"],
    &["ضيف", "تذكير"],
    &["سجل", "تذكير"],
    &["حط", "تذكير"],
    &["حطلي", "تذكير"],
    &["ابغي", "افتكر"],
    &["ابي", "افتكر"],
    &["ابغا", "افتكر"],
    &["ابي", "تذكرني"],
    &["ابغي", "تذكرني"],
    &["ابغا", "تذكرني"],
    &["ابيك", "تذكرني"],
    &["ابغاك", "تذكرني"],
    &["ابي", "تذكير"],
    &["ابغي", "تذكير"],
    &["سوي", "تذكير"],
    &["سويلي", "تذكير"],
    &["سوي", "لي", "تذكير"],
    &["بدي", "افتكر"],
    &["بدي", "تذكرني"],
    &["بدي", "تفكرني"],
    &["بدي", "تذكير"],
    // Meetings: Modern Standard, then Egyptian, Gulf, Levantine and mixed with English
    &["حدد", "اجتماعا"],
    &["حدد", "اجتماع"],
    &["جدول", "اجتماعا"],
    &["جدول", "اجتماع"],
    &["نظم", "اجتماعا"],
    &["نظم", "اجتماع"],
    &["انشئ", "اجتماعا"],
    &["انشئ", "اجتماع"],
    &["اضف", "اجتماعا"],
    &["اضف", "اجتماع"],
    &["اعمل", "اجتماع"],
    &["اعملي", "اجتماع"],
    &["اعمل", "لي", "اجتماع"],
    &["اعمل", "ميتنج"],
    &["اعمل", "ميتينج"],
    &["حددلي", "اجتماع"],
    &["ظبط", "اجتماع"],
    &["ظبطلي", "اجتماع"],
    &["نظملي", "اجتماع"],
    &["ضيف", "اجتماع"],
    &["عايز", "اعمل", "اجتماع"],
    &["عاوز", "اعمل", "اجتماع"],
    &["عايز", "احدد", "اجتماع"],
    &["عاوز", "احدد", "اجتماع"],
    &["عايز", "اعمل", "ميتنج"],
    &["سوي", "اجتماع"],
    &["سويلي", "اجتماع"],
    &["سوي", "لي", "اجتماع"],
    &["رتب", "اجتماع"],
    &["رتبلي", "اجتماع"],
    &["ابي", "اسوي", "اجتماع"],
    &["ابغي", "اسوي", "اجتماع"],
    &["ابغا", "اسوي", "اجتماع"],
    &["بدي", "اعمل", "اجتماع"],
    &["بدي", "احدد", "اجتماع"],
    &["حط", "اجتماع"],
    &["حطلي", "اجتماع"],
    &["حدد", "meeting"],
    &["اعمل", "meeting"],
    &["تعمل", "schedule", "ل", "meeting"],
    &["اعمل", "schedule", "ل", "meeting"],
];

/// Words that may follow a command and are not part of the text ("remind me to ...").
const GLUE: &[&str] = &["to", "about", "that", "called", "named", "titled", "ان", "اني", "انو", "انه", "باسم", "بعنوان"];

/// The word without punctuation around it, folded so spelling variants match.
fn core_word(token: &str) -> String {
    let word = token.trim_matches(|c: char| !c.is_alphanumeric());
    crate::guard::normalize(word)
}

fn tidy(words: &[&str]) -> String {
    words.join(" ").trim_matches(|c: char| c.is_whitespace() || ",.;:!?،؛؟".contains(c)).to_string()
}

/// The index of the first word after the polite words, the command and the
/// glue word at the start of `words`.
fn command_end(words: &[String]) -> usize {
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
    start
}

/// `text` without the command at its start ("Remind me to", "ذكرني", "Schedule
/// a meeting called"). Other words are kept as they are.
pub fn without_command_words(text: &str) -> String {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let words: Vec<String> = tokens.iter().map(|t| core_word(t)).collect();
    tidy(&tokens[command_end(&words)..])
}

const DAY_WORDS: &[&str] = &[
    "today", "tomorrow", "tonight", "اليوم", "النهارده", "انهارده", "الليله", "غدا", "غد", "بكره", "بكرا", "باكر", "باجر",
    "sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "الاحد", "الاثنين", "الثلاثاء",
    "الاربعاء", "الخميس", "الجمعه", "السبت",
    // Egyptian weekdays, and "the day after tomorrow" in Modern Standard
    "التنين", "التلات", "الاربع", "الغد",
];

/// Weekdays without ال, which only count after يوم or كل.
const BARE_WEEKDAYS: &[&str] = &["احد", "اثنين", "ثلاثاء", "اربعاء", "خميس", "جمعه", "سبت", "الحد", "تنين", "تلات", "اربع"];

const PART_WORDS: &[&str] = &[
    "morning", "am", "noon", "afternoon", "pm", "evening", "night", "صباحا", "صباح", "الصباح", "الصبح", "الصبحيه", "ظهرا",
    "ظهر", "الظهر", "ضهرا", "ضهر", "الضهر", "عصرا", "عصر", "العصر", "مساء", "المساء", "ليلا", "الليل", "بالليل",
    "midnight", "midday", "oclock", "a.m", "p.m", "النهار", "بالنهار", "بالصبح", "بليل", "الظهيره", "العصريه",
];

const HOUR_WORDS: &[&str] = &[
    "الواحده", "الثانيه", "الثالثه", "الرابعه", "الخامسه", "السادسه", "السابعه", "الثامنه", "التاسعه", "العاشره", "الحاديه",
];

/// Words that join a time to the text and only count next to a time.
const LINKS: &[&str] =
    &["at", "on", "in", "the", "this", "next", "every", "after", "الساعه", "يوم", "كل", "بعد", "قبل", "في"];

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
        || (word == "day" && next == Some("after") && words.get(i + 2).is_some_and(|w| w == "tomorrow"))
        || (word == "يوم" && prev == Some("كل"))
}

/// Which words are part of a time: the time words, and the links before them.
fn time_mask(words: &[String]) -> Vec<bool> {
    let mut time = vec![false; words.len()];
    for i in (0..words.len()).rev() {
        time[i] = is_time_word(words, i)
            || (LINKS.contains(&words[i].as_str()) && time.get(i + 1).copied().unwrap_or(false));
    }
    time
}

/// `text` without the words that say when ("tomorrow at 10 am", "بكرة الصبح",
/// "كل جمعة"). With `trailing_only`, only a time at the end is removed, so
/// "call the bank tomorrow" keeps its middle. Other words, names included,
/// are kept as they are.
pub fn without_time_words(text: &str, trailing_only: bool) -> String {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let words: Vec<String> = tokens.iter().map(|t| core_word(t)).collect();
    let time = time_mask(&words);
    if trailing_only {
        let keep = time.iter().rposition(|t| !t).map_or(0, |last| last + 1);
        return tidy(&tokens[..keep]);
    }
    let kept: Vec<&str> = tokens.iter().zip(&time).filter(|(_, t)| !**t).map(|(w, _)| *w).collect();
    tidy(&kept)
}

/// What is left of the user's own words once the command and the time are
/// taken out, exactly as they were written. Empty when nothing is left.
pub fn without_command_and_time(text: &str) -> String {
    without_command_words(&without_time_words(text, false))
}

/// Words that say a title follows. A title the user names ends at the time or
/// at "with" / "مع" before an attendee, so a title with those inside needs quotes.
const TITLE_MARKERS: &[&str] = &["called", "named", "titled", "باسم", "بعنوان", "اسمه", "اسمها", "عنوانه", "عنوانها"];

const WITH_WORDS: &[&str] = &["with", "مع"];

/// The longest default title, in words.
const DEFAULT_TITLE_WORDS: usize = 6;

/// Words a cut-short default title must not end on.
const DANGLING: &[&str] = &["with", "and", "of", "the", "a", "an", "to", "for", "in", "on", "at", "about", "مع", "و", "في", "عن", "من"];

/// The first `DEFAULT_TITLE_WORDS` words of `tokens`, without a dangling word at the end.
fn short_title(tokens: &[&str]) -> String {
    let mut end = tokens.len().min(DEFAULT_TITLE_WORDS);
    while end > 1 && end < tokens.len() && DANGLING.contains(&core_word(tokens[end - 1]).as_str()) {
        end -= 1;
    }
    tidy(&tokens[..end])
}

/// A person named in the user's text: the words where the name starts, and
/// the name as the user spelled it.
struct Mention {
    proposed: usize,
    start: usize,
    name: String,
}

/// Whether `wanted` starts at `words[start]`, and how many joined letters come
/// before it. Arabic joins "and", "to" and "with" to the next word (وسارة, لسارة,
/// بسارة, ولسارة), so up to two of those letters before the name count.
fn name_at(words: &[String], start: usize, wanted: &[String]) -> Option<usize> {
    let span = words.get(start..start + wanted.len())?;
    let (first, rest) = span.split_first()?;
    if rest != &wanted[1..] {
        return None;
    }
    if *first == wanted[0] {
        return Some(0);
    }
    let once = first.strip_prefix(['و', 'ل', 'ب'])?;
    if once == wanted[0] {
        return Some(1);
    }
    (once.strip_prefix(['ل', 'ب'])? == wanted[0]).then_some(2)
}

fn trim_token(token: &str) -> &str {
    token.trim_matches(|c: char| !c.is_alphanumeric())
}

/// Every place where one of the `proposed` names is in the text.
fn mentions(tokens: &[&str], words: &[String], proposed: &[String]) -> Vec<Mention> {
    let mut found = Vec::new();
    for (index, name) in proposed.iter().enumerate() {
        let wanted: Vec<String> = name.split_whitespace().map(core_word).filter(|w| !w.is_empty()).collect();
        if wanted.is_empty() {
            continue;
        }
        for start in 0..words.len() {
            let Some(joined) = name_at(words, start, &wanted) else { continue };
            let mut parts: Vec<&str> = tokens[start..start + wanted.len()].iter().map(|t| trim_token(t)).collect();
            // Drop the joined letters and the marks written on them.
            let mut rest = parts[0];
            for _ in 0..joined {
                let mut chars = rest.chars();
                chars.next();
                rest = chars.as_str().trim_start_matches(|c: char| ('\u{064B}'..='\u{0652}').contains(&c));
            }
            parts[0] = rest;
            found.push(Mention { proposed: index, start, name: parts.join(" ") });
        }
    }
    found
}

/// A title in quotes at the start of `tokens`, exactly as written.
fn quoted_title(tokens: &[&str]) -> Option<String> {
    let joined = tokens.join(" ");
    let inner = joined.strip_prefix(['"', '“', '«'])?;
    let end = inner.find(['"', '”', '“', '»'])?;
    let title = inner[..end].trim();
    (!title.is_empty()).then(|| title.to_string())
}

/// The title the user named after a marker such as "called" or "باسم".
fn named_title(tokens: &[&str], words: &[String], time: &[bool], mentions: &[Mention]) -> Option<String> {
    let starts_name = |i: usize| mentions.iter().any(|m| m.start == i);
    for marker in (0..words.len()).filter(|&i| TITLE_MARKERS.contains(&words[i].as_str())) {
        let first = marker + 1;
        // "a guy named Bob" names a person, not the meeting.
        if first >= words.len() || starts_name(first) {
            continue;
        }
        if let Some(title) = quoted_title(&tokens[first..]) {
            return Some(title);
        }
        let end = (first..words.len())
            .find(|&i| time[i] || (WITH_WORDS.contains(&words[i].as_str()) && starts_name(i + 1)))
            .unwrap_or(words.len());
        let title = tidy(&tokens[first..end]);
        if !title.is_empty() {
            return Some(title);
        }
    }
    None
}

/// A short title from the user's words when no title was named: what is left
/// after the command and the time, up to the "with" / "مع" that opens the
/// attendees. When nothing is left before it, "Meeting" / "اجتماع" with what
/// follows ("Meeting with Sara").
fn default_title(text: &str, proposed: &[String]) -> String {
    let own = without_command_and_time(text);
    let tokens: Vec<&str> = own.split_whitespace().collect();
    let words: Vec<String> = tokens.iter().map(|t| core_word(t)).collect();
    let names: Vec<Vec<String>> = proposed
        .iter()
        .map(|n| n.split_whitespace().map(core_word).filter(|w| !w.is_empty()).collect::<Vec<_>>())
        .filter(|n| !n.is_empty())
        .collect();
    let own_mentions = |i: usize| names.iter().any(|n| name_at(&words, i, n).is_some());
    let cut = (0..words.len())
        .find(|&i| WITH_WORDS.contains(&words[i].as_str()) && (i == 0 || own_mentions(i + 1)))
        .unwrap_or(words.len());
    let mut head = &tokens[..cut];
    if head.len() > 1 && matches!(words[0].as_str(), "a" | "an" | "the") {
        head = &head[1..];
    }
    if !head.is_empty() {
        return short_title(head);
    }
    let meeting = if has_letters(text, Language::Arabic) { "اجتماع" } else { "Meeting" };
    let clause = &tokens[cut..];
    if clause.is_empty() {
        meeting.to_string()
    } else {
        format!("{meeting} {}", short_title(clause))
    }
}

/// The title and attendees of a meeting, from the user's own words. The title is
/// the one the user named, used exactly, or a short default. An attendee the
/// model proposed is kept, spelled as the user wrote it, only when the user's
/// text has it; the others are dropped.
pub fn meeting_from_user_text(text: &str, proposed_attendees: &[String]) -> (String, Vec<String>) {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let words: Vec<String> = tokens.iter().map(|t| core_word(t)).collect();
    let found = mentions(&tokens, &words, proposed_attendees);
    let title = named_title(&tokens, &words, &time_mask(&words), &found).unwrap_or_else(|| default_title(text, proposed_attendees));
    let mut attendees: Vec<String> = Vec::new();
    for index in 0..proposed_attendees.len() {
        if let Some(m) = found.iter().find(|m| m.proposed == index) {
            if !attendees.contains(&m.name) {
                attendees.push(m.name.clone());
            }
        }
    }
    (title, attendees)
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

    #[test]
    fn dialect_commands_are_removed() {
        for (text, expected) in [
            ("فكرني اتصل بالبنك", "اتصل بالبنك"),
            ("ممكن تفكرني اتصل بالبنك", "اتصل بالبنك"),
            ("ابغاك تذكرني اتصل بالبنك", "اتصل بالبنك"),
            ("بدي تفكرني اني اتصل بالبنك", "اتصل بالبنك"),
            ("اعملي تذكير اتصل بالبنك", "اتصل بالبنك"),
            ("عايز اعمل اجتماع مع الفريق", "مع الفريق"),
            ("Please set me a reminder to call the bank", "call the bank"),
            ("Call the bank", "Call the bank"),
        ] {
            assert_eq!(without_command_words(text), expected, "{text}");
        }
    }

    #[test]
    fn dialect_time_words_are_removed() {
        for (text, expected) in [
            ("اتصل بالبنك النهاردة الضهر", "اتصل بالبنك"),
            ("اتصل بالبنك يوم التلات بعد الضهر", "اتصل بالبنك"),
            ("اتصل بالبنك باجر بالصبح", "اتصل بالبنك"),
            ("اتصل بالبنك كل يوم الحد", "اتصل بالبنك"),
            ("call the bank the day after tomorrow", "call the bank"),
            ("call the bank at 9 o'clock tonight", "call the bank"),
            ("call the bank at midnight", "call the bank"),
            ("قبل الحد الأقصى", "قبل الحد الأقصى"),
        ] {
            assert_eq!(without_time_words(text, false), expected, "{text}");
        }
    }
}
