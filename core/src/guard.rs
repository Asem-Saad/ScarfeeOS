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
fn normalize(text: &str) -> String {
    text.chars()
        .filter(|c| !('\u{064B}'..='\u{0652}').contains(c) && !matches!(c, '\u{0640}' | '\'' | '’'))
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

/// Tells whether the user asks for the current time, news or weather, which a
/// local model cannot know and must not invent.
pub fn asks_for_live_data(user_text: &str) -> Option<LiveData> {
    let text = normalize(user_text);
    let words: Vec<&str> = text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
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
}
