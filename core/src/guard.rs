//! Checks on what the model says and on what the user asks. Small local models
//! mix foreign scripts into Arabic replies and make up the time and the news,
//! so both are caught in code.

use crate::lang::Language;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    Latin,
    Arabic,
    Cyrillic,
    Cjk,
    Hiragana,
    Katakana,
    Hangul,
    Hebrew,
    Thai,
    Devanagari,
}

/// One word in a reply that is written in a script that does not belong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub script: Script,
    pub word: String,
}

/// Latin and Arabic are allowed in both languages: Arabic replies carry
/// English terms, URLs and file names, and English replies carry Arabic names.
fn allowed(language: Language) -> &'static [Script] {
    match language {
        Language::English => &[Script::Latin, Script::Arabic],
        Language::Arabic => &[Script::Arabic, Script::Latin],
    }
}

/// Greek is left out on purpose: π and Δ are normal in technical answers.
fn script_of(c: char) -> Option<Script> {
    let n = c as u32;
    let in_any = |ranges: &[(u32, u32)]| ranges.iter().any(|&(lo, hi)| (lo..=hi).contains(&n));
    if c.is_ascii_alphabetic() || (c.is_alphabetic() && in_any(&[(0xC0, 0x24F), (0x1E00, 0x1EFF)]) && n != 0xD7 && n != 0xF7) {
        Some(Script::Latin)
    } else if in_any(&[(0x620, 0x65F), (0x66E, 0x6D3), (0x6D5, 0x6FF), (0x750, 0x77F), (0x8A0, 0x8FF), (0xFB50, 0xFDFB), (0xFE70, 0xFEFC)]) {
        Some(Script::Arabic)
    } else if in_any(&[(0x400, 0x52F), (0x1C80, 0x1C8F), (0x2DE0, 0x2DFF), (0xA640, 0xA69F)]) {
        Some(Script::Cyrillic)
    } else if in_any(&[(0x3040, 0x309F)]) {
        Some(Script::Hiragana)
    } else if in_any(&[(0x30A0, 0x30FF), (0x31F0, 0x31FF), (0xFF66, 0xFF9F)]) {
        Some(Script::Katakana)
    } else if in_any(&[(0x1100, 0x11FF), (0x3130, 0x318F), (0xA960, 0xA97F), (0xAC00, 0xD7FF), (0xFFA0, 0xFFDC)]) {
        Some(Script::Hangul)
    } else if in_any(&[
        (0x2E80, 0x2FDF), (0x3007, 0x3007), (0x3100, 0x312F), (0x31A0, 0x31BF), (0x3400, 0x4DBF),
        (0x4E00, 0x9FFF), (0xF900, 0xFAFF), (0x20000, 0x2EBEF),
    ]) {
        Some(Script::Cjk)
    } else if in_any(&[(0x590, 0x5FF), (0xFB1D, 0xFB4F)]) {
        Some(Script::Hebrew)
    } else if in_any(&[(0xE00, 0xE7F)]) {
        Some(Script::Thai)
    } else if in_any(&[(0x900, 0x97F)]) {
        Some(Script::Devanagari)
    } else {
        None
    }
}

/// Reports every word written in a script that does not belong in `language`.
/// Digits, punctuation, emoji and symbols are never reported.
pub fn check_reply(reply: &str, language: Language) -> Vec<Issue> {
    let allowed = allowed(language);
    let mut issues = Vec::new();
    let mut word = String::new();
    let mut foreign: Option<Script> = None;
    let mut flush = |word: &mut String, foreign: &mut Option<Script>| {
        if let Some(script) = foreign.take() {
            issues.push(Issue { script, word: word.clone() });
        }
        word.clear();
    };
    for c in reply.chars() {
        let script = script_of(c);
        if c.is_alphanumeric() || script.is_some() {
            word.push(c);
            if foreign.is_none() {
                foreign = script.filter(|s| !allowed.contains(s));
            }
        } else {
            flush(&mut word, &mut foreign);
        }
    }
    flush(&mut word, &mut foreign);
    issues
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveData {
    Time,
    News,
    Weather,
}

/// Lowercases, drops apostrophes and Arabic diacritics and tatweel, and folds
/// Arabic spelling variants, so that الأخبار matches الاخبار and What's matches whats.
pub(crate) fn normalize(text: &str) -> String {
    text.chars()
        .filter(|c| !('\u{064B}'..='\u{0652}').contains(c) && !matches!(c, '\u{0640}' | '\'' | '’'))
        .map(|c| match c {
            'أ' | 'إ' | 'آ' => 'ا',
            'ة' => 'ه',
            'ى' => 'ي',
            '٠'..='٩' => char::from_u32(c as u32 - 0x0660 + 0x30).unwrap_or(c),
            '۰'..='۹' => char::from_u32(c as u32 - 0x06F0 + 0x30).unwrap_or(c),
            _ => c,
        })
        .flat_map(char::to_lowercase)
        .collect()
}

/// Words that may come before a phrase ("please tell me ...").
const LEAD: &[&str] = &[
    "please", "hey", "hi", "hello", "ok", "okay", "so", "and", "can", "could", "would", "you", "tell", "show", "give",
    "get", "me", "the", "i", "need", "want", "to", "know", "just", "quick", "now",
    "لو", "سمحت", "من", "فضلك", "طيب", "يا", "ما", "هو", "ممكن", "تقدر", "تقول", "لي", "قل", "اخبرني", "اعطني",
    "ايه", "شو", "ايش", "هات", "وريني", "ورني", "اعرض", "اعرف", "اريد", "عايز", "ابغى", "ابي", "بدي", "ان",
];

/// Words that may come after a phrase ("... right now, please").
const TRAIL: &[&str] = &[
    "now", "today", "tonight", "tomorrow", "right", "currently", "outside", "like", "please", "this", "morning",
    "evening", "week", "there", "here", "in", "at", "for", "on", "about", "from", "near",
    "الان", "اليوم", "حاليا", "الحين", "دلوقتي", "النهارده", "بكره", "غدا", "لو", "سمحت", "من", "فضلك", "عندكم",
    "عندنا", "هنا", "الليله", "في", "عن", "ب",
];

/// Prepositions that open a place or topic of up to three words.
const OPENERS: &[&str] = &["in", "at", "for", "on", "about", "from", "near", "في", "عن"];

const MAX_WORDS: usize = 14;

// Phrases are written in normalized form: no diacritics, ا for أإآ, ه for ة, ي for ى.
const TIME: &[&[&str]] = &[
    &["what", "time", "is", "it"],
    &["whats", "the", "time"],
    &["what", "is", "the", "time"],
    &["whats", "the", "current", "time"],
    &["what", "is", "the", "current", "time"],
    &["current", "time"],
    &["the", "time"],
    &["time", "now"],
    &["كم", "الساعه"],
    &["الساعه", "كام"],
    &["الساعه", "كم"],
    &["كام", "الساعه"],
    &["وش", "الساعه"],
    &["ايش", "الساعه"],
    &["كم", "الوقت"],
    &["الساعه", "الان"],
    &["الساعه", "الحين"],
    &["الساعه", "دلوقتي"],
    &["الوقت", "الان"],
    &["الوقت", "الحين"],
    &["الوقت", "دلوقتي"],
    &["الوقت", "الحالي"],
];

const NEWS: &[&[&str]] = &[
    &["whats", "the", "news"],
    &["what", "is", "the", "news"],
    &["the", "news"],
    &["latest", "news"],
    &["todays", "news"],
    &["news", "today"],
    &["top", "headlines"],
    &["headlines"],
    &["اخر", "الاخبار"],
    &["اخر", "اخبار"],
    &["اهم", "الاخبار"],
    &["اخبار", "اليوم"],
    &["الاخبار", "اليوم"],
    &["الاخبار", "الان"],
    &["الاخبار", "الحين"],
    &["الاخبار", "دلوقتي"],
    &["الاخبار", "النهارده"],
    &["عناوين", "الاخبار"],
    &["نشره", "الاخبار"],
    &["اخبار", "العالم"],
    &["وش", "الاخبار"],
];

const WEATHER: &[&[&str]] = &[
    &["whats", "the", "weather"],
    &["what", "is", "the", "weather"],
    &["hows", "the", "weather"],
    &["how", "is", "the", "weather"],
    &["the", "weather"],
    &["weather", "today"],
    &["weather", "now"],
    &["weather", "tomorrow"],
    &["weather", "forecast"],
    &["will", "it", "rain"],
    &["is", "it", "raining"],
    &["كيف", "الطقس"],
    &["كيف", "حاله", "الطقس"],
    &["ما", "حاله", "الطقس"],
    &["وش", "الطقس"],
    &["ايش", "الطقس"],
    &["شو", "الطقس"],
    &["ايه", "الطقس"],
    &["الطقس", "اليوم"],
    &["طقس", "اليوم"],
    &["الطقس", "الان"],
    &["الطقس", "حاليا"],
    &["الطقس", "غدا"],
    &["الطقس", "بكره"],
    &["الطقس", "النهارده"],
    &["الطقس", "دلوقتي"],
    &["الطقس", "عامل", "ايه"],
    &["الجو", "عامل", "ايه"],
    &["كيف", "الجو"],
    &["شلون", "الجو"],
    &["وش", "الجو"],
    &["توقعات", "الطقس"],
    &["درجه", "الحراره", "اليوم"],
    &["درجه", "الحراره", "الان"],
    &["هل", "ستمطر"],
    &["هل", "سيمطر"],
];

/// A request is accepted only when it is a phrase from the lists above, with
/// nothing before it but polite filler and nothing after it but a time word or
/// a short place or topic. "اشرح لي الطقس والمناخ" and "Explain what a VPN
/// does" do not qualify. When in doubt this says no.
fn matches_any(words: &[&str], phrases: &[&[&str]]) -> bool {
    phrases.iter().any(|phrase| {
        (0..words.len()).any(|i| {
            words[i..].starts_with(phrase)
                && words[..i].iter().all(|w| LEAD.contains(w))
                && trail_ok(&words[i + phrase.len()..])
        })
    })
}

fn trail_ok(tail: &[&str]) -> bool {
    let mut i = 0;
    let mut opened = false;
    while i < tail.len() {
        if OPENERS.contains(&tail[i]) && !opened {
            opened = true;
            i = (i + 4).min(tail.len());
        } else if TRAIL.contains(&tail[i]) {
            i += 1;
        } else {
            return false;
        }
    }
    true
}

fn split_words(normalized: &str) -> Vec<&str> {
    normalized.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect()
}

/// Tells whether the user asks for the current time, news or weather, which a
/// local model cannot know and must not invent. Each sentence is checked on its
/// own, so a second sentence cannot hide the request in the first.
pub fn asks_for_live_data(user_text: &str) -> Option<LiveData> {
    user_text
        .split(['.', '؟', '?', '!', '\n'])
        .find_map(live_data_in_sentence)
}

fn live_data_in_sentence(sentence: &str) -> Option<LiveData> {
    let text = normalize(sentence);
    let words = split_words(&text);
    if words.len() > MAX_WORDS {
        return None;
    }
    if matches_any(&words, TIME) {
        Some(LiveData::Time)
    } else if matches_any(&words, NEWS) {
        Some(LiveData::News)
    } else if matches_any(&words, WEATHER) {
        Some(LiveData::Weather)
    } else {
        None
    }
}

/// Tells whether the user asks to translate. A translation request is not a
/// request to change the system language.
pub fn asks_to_translate(user_text: &str) -> bool {
    let text = normalize(user_text);
    text.contains("translat") || text.contains("ترجم")
}

// Hijri words, in normalized form. "هجري" is matched inside a word, so it also
// covers الهجري, بالهجري and الهجرية.
const HIJRI_CUES: &[&str] = &[
    "date", "dates", "convert", "conversion", "corresponding", "equivalent", "gregorian",
    "تاريخ", "التاريخ", "تحويل", "حول", "يقابل", "يوافق", "يعادل", "ميلادي", "الميلادي", "ميلاديه", "الموافق", "مقابل",
];

/// Tells whether the user asks to convert a date to the Hijri calendar: a
/// Hijri word together with a number or a word like "date" or "يقابل".
pub fn asks_for_hijri(user_text: &str) -> bool {
    let text = normalize(user_text);
    let words = split_words(&text);
    let hijri = words.iter().any(|w| *w == "hijri" || w.contains("هجري"))
        || (words.contains(&"islamic") && words.iter().any(|w| matches!(*w, "date" | "dates" | "calendar")));
    hijri && words.iter().any(|w| w.chars().any(|c| c.is_ascii_digit()) || HIJRI_CUES.contains(w))
}

// Religious words, in normalized form (see `normalize`). A word also matches
// without a final "s" and, in Arabic, without و ف ب ل ك and ال in front, so
// "Muslims" and "للإسلام" match. This is a list of words, not a classifier: it
// prefers a false alarm to an answer, and it will miss some questions.
const RELIGIOUS_EN: &[&str] = &[
    "islam", "islamic", "islamist", "islamophobia", "muslim", "muslimah", "quran", "quranic", "koran", "hadith",
    "sunnah", "sunni", "shia", "shiite", "sufi", "sufism", "fatwa", "fatwah", "sharia", "shariah", "allah", "imam",
    "mosque", "masjid", "hajj", "umrah", "zakat", "salah", "salat", "wudu", "halal", "haram", "tarawih", "taraweeh",
    "mufti", "jihad", "christian", "christianity", "christ", "jesus", "bible", "biblical", "gospel", "church",
    "cathedral", "catholic", "catholicism", "protestant", "pope", "vatican", "jewish", "judaism", "torah", "talmud",
    "rabbi", "synagogue", "hindu", "hinduism", "buddhist", "buddhism", "buddha", "sikh", "sikhism", "bahai",
    "zoroastrian", "religion", "religious", "theology", "theological", "scripture", "scriptural", "sermon",
    "blasphemy",
];

const RELIGIOUS_AR: &[&str] = &[
    "اسلام", "اسلامي", "اسلاميه", "مسلم", "مسلمه", "مسلمين", "مسلمون", "مسلمات", "قران", "قراني", "قرانيه", "فتوي",
    "فتاوي", "افتاء", "مفتي", "شريعه", "فقه", "عقيده", "وضوء", "تيمم", "صيام", "صوم", "زكاه", "حج", "تراويح",
    "تهجد", "نبوي", "نبويه", "شيعه", "مسيحي", "مسيحيه", "مسيحيين", "نصراني", "نصرانيه", "انجيل", "يسوع", "مسيح",
    "كنيسه", "كنائس", "كاثوليك", "كاثوليكي", "بروتستانت", "فاتيكان", "يهودي", "يهوديه", "يهود", "توراه", "تلمود",
    "حاخام", "هندوسيه", "هندوسي", "بوذيه", "بوذي", "بوذا", "بهائيه", "اديان", "ديني", "دينيه", "دينيا", "لاهوت",
    "مسجد", "مساجد",
];

// Words that are religious only outside a quoted title, such as a song called "Prayer".
const RELIGIOUS_WEAK_EN: &[&str] = &["pray", "prayer", "praying", "prayed", "worship", "worshipping", "god"];

const RELIGIOUS_WEAK_AR: &[&str] = &["صلاه", "صلوات", "ركعه", "ركعات"];

const RELIGIOUS_PHRASES: &[&[&str]] = &[
    &["prophet", "muhammad"],
    &["prophet", "mohammed"],
    &["prophet", "muhammed"],
    &["prophet", "mohammad"],
    &["peace", "be", "upon", "him"],
    &["holy", "book"],
    &["holy", "spirit"],
    &["old", "testament"],
    &["new", "testament"],
    &["ten", "commandments"],
    &["day", "of", "judgment"],
    &["judgment", "day"],
    &["الكتاب", "المقدس"],
    &["ليله", "القدر"],
    &["حديث", "نبوي"],
    &["حديث", "شريف"],
    &["احاديث", "نبويه"],
    &["هل", "حرام"],
    &["هل", "حلال"],
    &["العمره"],
];

// Ramadan is also a calendar month, so it counts only next to one of these words.
const RAMADAN: &[&str] = &["ramadan", "ramadhan", "ramazan", "رمضان"];

const RAMADAN_CONTEXT: &[&str] = &[
    "fast", "fasting", "rule", "ruling", "obligatory", "mandatory", "permissible", "allowed", "forbidden", "meaning",
    "significance", "importance", "virtue", "reward", "tarawih", "taraweeh", "prayer", "pray",
    "صيام", "صوم", "حكم", "احكام", "فضل", "فضائل", "تراويح", "تهجد", "ثواب", "واجب", "يجوز", "حرام", "حلال", "معني",
    "اهميه", "قيام",
];

/// The word as written and the forms it may be listed under.
fn forms(word: &str) -> Vec<&str> {
    let mut out = vec![word];
    if let Some(rest) = word.strip_suffix('s') {
        out.push(rest);
    }
    for prefixes in [&["لل", "و", "ف", "ب", "ل", "ك"][..], &["لل", "و", "ف", "ب", "ل", "ك"][..], &["ال"][..]] {
        for form in out.clone() {
            for prefix in prefixes {
                if let Some(rest) = form.strip_prefix(prefix) {
                    if rest.chars().count() >= 2 && !out.contains(&rest) {
                        out.push(rest);
                    }
                }
            }
        }
    }
    out
}

fn has_term(words: &[&str], terms: &[&str]) -> bool {
    words.iter().any(|w| forms(w).iter().any(|f| terms.contains(f)))
}

fn has_phrase(words: &[&str], phrases: &[&[&str]]) -> bool {
    phrases.iter().any(|phrase| words.windows(phrase.len()).any(|w| w == *phrase))
}

/// "Islamic date" and "التقويم الإسلامي" name the Hijri calendar, not the faith.
fn without_calendar_uses<'a>(words: &[&'a str]) -> Vec<&'a str> {
    (0..words.len())
        .filter(|&i| {
            let next = words.get(i + 1).copied();
            let prev = i.checked_sub(1).map(|j| words[j]);
            let calendar = (words[i] == "islamic" && matches!(next, Some("date" | "dates" | "calendar")))
                || (forms(words[i]).iter().any(|f| matches!(*f, "اسلامي" | "اسلاميه"))
                    && matches!(prev, Some("تقويم" | "التقويم")));
            !calendar
        })
        .map(|i| words[i])
        .collect()
}

/// Cuts out text in double quotes, so a quoted title is not read as a question.
fn strip_quoted(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut close = None;
    for (i, &c) in chars.iter().enumerate() {
        if let Some(end) = close {
            if c == end {
                close = None;
                out.push(' ');
            }
            continue;
        }
        let end = match c {
            '"' => '"',
            '“' => '”',
            '«' => '»',
            _ => {
                out.push(c);
                continue;
            }
        };
        if chars[i + 1..].contains(&end) {
            close = Some(end);
        } else {
            out.push(c);
        }
    }
    out
}

/// Tells whether the text is about religion, in English or Arabic. ScarfeeOS
/// gives no religious content, so the assistant declines these. Hijri dates,
/// "Ramadan" as a month and names like Islamabad are not religious content.
pub fn asks_religious(user_text: &str) -> bool {
    let text = normalize(user_text);
    let all = without_calendar_uses(&split_words(&text));
    let unquoted = normalize(&strip_quoted(user_text));
    let unquoted = without_calendar_uses(&split_words(&unquoted));
    has_term(&all, RELIGIOUS_EN)
        || has_term(&all, RELIGIOUS_AR)
        || has_phrase(&all, RELIGIOUS_PHRASES)
        || has_term(&unquoted, RELIGIOUS_WEAK_EN)
        || has_term(&unquoted, RELIGIOUS_WEAK_AR)
        || (has_term(&all, RAMADAN) && has_term(&all, RAMADAN_CONTEXT))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(reply: &str, lang: Language) -> Vec<String> {
        check_reply(reply, lang).into_iter().map(|i| i.word).collect()
    }

    #[test]
    fn reports_russian_and_chinese_in_arabic() {
        let issues = check_reply("افتح الملف بعد ذلك привет ثم 你好世界 الآن", Language::Arabic);
        assert_eq!(
            issues,
            vec![
                Issue { script: Script::Cyrillic, word: "привет".into() },
                Issue { script: Script::Cjk, word: "你好世界".into() },
            ]
        );
    }

    #[test]
    fn reports_other_scripts() {
        assert_eq!(check_reply("こんにちは", Language::Arabic)[0].script, Script::Hiragana);
        assert_eq!(check_reply("コンピュータ", Language::Arabic)[0].script, Script::Katakana);
        assert_eq!(check_reply("안녕하세요", Language::Arabic)[0].script, Script::Hangul);
        assert_eq!(check_reply("שלום", Language::Arabic)[0].script, Script::Hebrew);
        assert_eq!(check_reply("สวัสดี", Language::Arabic)[0].script, Script::Thai);
        assert_eq!(check_reply("नमस्ते", Language::Arabic)[0].script, Script::Devanagari);
        assert_eq!(words("नमस्ते", Language::Arabic), vec!["नमस्ते"]);
    }

    #[test]
    fn latin_digits_urls_and_files_are_fine_in_arabic() {
        let reply = "افتح report_final.docx من https://example.com/a?b=1 عبر VPN، الإصدار ٣ و 4.2 😀";
        assert!(check_reply(reply, Language::Arabic).is_empty());
    }

    #[test]
    fn arabic_is_fine_in_english() {
        assert!(check_reply("The user Ahmed (أحمد) opened the file.", Language::English).is_empty());
    }

    #[test]
    fn foreign_scripts_are_reported_in_english_too() {
        assert_eq!(words("Open the файл now", Language::English), vec!["файл"]);
    }

    #[test]
    fn accents_math_and_punctuation_are_fine() {
        assert!(check_reply("Café résumé: π ≈ 3.14, Δx — «ok» ×÷", Language::English).is_empty());
    }

    #[test]
    fn foreign_word_glued_to_arabic_is_reported_whole() {
        assert_eq!(words("الملفпрограмма،", Language::Arabic), vec!["الملفпрограмма"]);
    }

    #[test]
    fn required_live_data_requests() {
        assert_eq!(asks_for_live_data("كم الساعة الآن في طوكيو؟"), Some(LiveData::Time));
        assert_eq!(asks_for_live_data("وش الأخبار اليوم؟"), Some(LiveData::News));
        assert_eq!(asks_for_live_data("What's the weather like?"), Some(LiveData::Weather));
        assert_eq!(asks_for_live_data("Explain what a VPN does"), None);
        assert_eq!(asks_for_live_data("اشرح لي الطقس والمناخ"), None);
    }

    #[test]
    fn time_requests() {
        for text in [
            "What time is it?",
            "what time is it in Tokyo",
            "What's the time right now?",
            "Tell me the current time, please",
            "الساعة كام؟",
            "الساعة كام دلوقتي",
            "كم الساعة؟",
            "كم الوقت الآن",
            "طيب الساعة كام",
        ] {
            assert_eq!(asks_for_live_data(text), Some(LiveData::Time), "{text}");
        }
    }

    #[test]
    fn news_requests() {
        for text in [
            "What's the news?",
            "Give me the latest news about Egypt",
            "headlines",
            "وش الأخبار",
            "آخر الأخبار",
            "ايه الأخبار النهارده",
            "اهم الاخبار الان",
        ] {
            assert_eq!(asks_for_live_data(text), Some(LiveData::News), "{text}");
        }
    }

    #[test]
    fn weather_requests() {
        for text in [
            "What's the weather in Cairo today?",
            "How's the weather",
            "weather forecast for tomorrow",
            "Will it rain tomorrow?",
            "كيف الطقس اليوم",
            "الطقس النهارده",
            "الجو عامل ايه",
            "وش الطقس عندكم",
            "هل ستمطر غدا؟",
        ] {
            assert_eq!(asks_for_live_data(text), Some(LiveData::Weather), "{text}");
        }
    }

    #[test]
    fn other_questions_are_not_live_data() {
        for text in [
            "What is the time complexity of quicksort?",
            "What time is the meeting?",
            "Is the time in Tokyo different from Cairo?",
            "Explain the news cycle",
            "Summarize the news article below",
            "Why does weather change with altitude?",
            "الاجتماع الساعة كام",
            "ايه الأخبار",
            "شو الأخبار",
            "اشرح لي الأخبار الكاذبة",
            "ما هو الطقس",
            "كم سعر الساعة",
            "Set a reminder at 5pm",
            "",
        ] {
            assert_eq!(asks_for_live_data(text), None, "{text}");
        }
    }

    #[test]
    fn long_text_is_not_a_request() {
        let text = "Here is an article. The latest news says that nothing happened in the city at all today and people went home.";
        assert_eq!(asks_for_live_data(text), None);
    }

    #[test]
    fn religious_questions_are_caught() {
        for text in [
            "What are the five pillars of Islam?",
            "ما هي أركان الإسلام الخمسة؟",
            "Explain Surah Al-Fatiha from the Quran",
            "Who wrote the Bible?",
            "ما تفسير هذه الآية من القرآن الكريم",
            "ما هو الإنجيل؟",
            "Is it a sin to skip the midday prayer?",
            "What is the ruling on praying while traveling?",
            "ما حكم الصلاة في السفر",
            "Is this fatwa valid?",
            "هل هذه الفتوى صحيحة",
            "Tell me about Buddhism and Hinduism",
            "What do Jewish people believe?",
            "ما هي اليهودية",
            "Can Muslims eat this? Is it halal?",
            "هل يجوز للمسلمين ذلك",
            "What does the Prophet Muhammad say about trade?",
            "Is fasting in Ramadan obligatory?",
            "ما حكم صيام رمضان",
            "Does God exist?",
            "what does my religion say",
            "ما هو الإسلام",
            "اشرح لي أحكام الصلاة والزكاة",
        ] {
            assert!(asks_religious(text), "{text}");
        }
    }

    #[test]
    fn ordinary_questions_are_not_religious() {
        for text in [
            "Open Islamabad on the map",
            "What is the population of Islamabad?",
            "Play \"Livin' on a Prayer\" by Bon Jovi",
            "شغل أغنية «صلاة» لفيروز",
            "When does Ramadan start in 2026?",
            "كم يوم باقي على رمضان",
            "What is the Hijri date for 1 January 2026?",
            "ما التاريخ الهجري الذي يقابل 1 يناير 2026 ميلادي تقريبًا؟",
            "Convert 1 January 2026 to the Islamic calendar",
            "Convert to the Islamic date for tomorrow",
            "حول التاريخ إلى التقويم الإسلامي",
            "ما هو شهر ذو الحجة في التقويم الهجري",
            "Explain what a VPN does",
            "اشرح لي الطقس والمناخ",
            "Remind me to call the bank tomorrow",
            "ذكرني بالاجتماع غدا إن شاء الله",
            "سداد الدين الشهري",
            "Schedule a meeting with Sara on Sunday",
            "",
        ] {
            assert!(!asks_religious(text), "{text}");
        }
    }

    #[test]
    fn religious_words_match_with_endings_and_prefixes() {
        assert!(asks_religious("Islam's history"));
        assert!(asks_religious("What do Christians believe?"));
        assert!(asks_religious("ما رأيك في الاسلام"));
        assert!(asks_religious("تحدث عن للإسلام"));
        assert!(asks_religious("وبالقرآن"));
        assert!(!asks_religious("Islamabadis"));
    }

    #[test]
    fn each_sentence_is_checked_for_live_data() {
        assert_eq!(asks_for_live_data("وش الأخبار اليوم؟ ابي تلخص لي أهم شي."), Some(LiveData::News));
        assert_eq!(asks_for_live_data("What's the weather today? Please be brief."), Some(LiveData::Weather));
        assert_eq!(asks_for_live_data("Hi!\nWhat time is it?"), Some(LiveData::Time));
        assert_eq!(asks_for_live_data("Explain what a VPN does"), None);
        assert_eq!(asks_for_live_data("اشرح لي الطقس والمناخ"), None);
        assert_eq!(asks_for_live_data("Explain what a VPN does. Why is it useful?"), None);
    }

    #[test]
    fn hijri_requests() {
        for text in [
            "ما التاريخ الهجري الذي يقابل 1 يناير 2026 ميلادي تقريبًا؟",
            "What is the Hijri date for 1 January 2026?",
            "Convert January 1, 2026 to Hijri",
            "what is 2026-01-01 in the Islamic calendar",
            "كم يوافق ٢٠٢٦/١/١ بالهجري",
            "تحويل التاريخ الهجري",
        ] {
            assert!(asks_for_hijri(text), "{text}");
        }
        for text in ["Remind me on 1 January 2026", "What is the date today?", "ما هو التاريخ اليوم", "Open the Hijri app", ""] {
            assert!(!asks_for_hijri(text), "{text}");
        }
    }

    #[test]
    fn translation_requests() {
        for text in ["Translate to Arabic: good morning", "ترجم هذه الجملة", "ترجملي الجملة", "Give me a translation"] {
            assert!(asks_to_translate(text), "{text}");
        }
        assert!(!asks_to_translate("Switch the system language to Arabic"));
        assert!(!asks_to_translate("غير لغة النظام إلى العربية"));
    }
}
