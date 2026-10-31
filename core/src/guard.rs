//! Checks on what the model says and on what the user asks. Small local models
//! mix foreign scripts into Arabic replies and make up the time and the news,
//! so both are caught in code.

use crate::lang::{self, Language};

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
/// Chinese punctuation (，。（）) and full-width letters count as CJK: small
/// models write them around Arabic words, and they are not Arabic or Latin.
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
        (0x2E80, 0x2FDF), (0x3001, 0x303F), (0x3100, 0x312F), (0x31A0, 0x31BF), (0x3400, 0x4DBF),
        (0x4E00, 0x9FFF), (0xF900, 0xFAFF), (0xFF01, 0xFF0F), (0xFF1A, 0xFF65), (0x20000, 0x323AF),
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
/// Digits, Western punctuation, emoji and symbols are never reported.
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

/// Scripts the user wrote in or asked for ("translate to Chinese", "بالروسية"),
/// which may then appear in the reply even though they do not belong in it.
fn permitted_scripts(user_text: &str) -> Vec<Script> {
    const ASKED: &[(&str, &[Script])] = &[
        ("chinese", &[Script::Cjk]), ("mandarin", &[Script::Cjk]), ("cantonese", &[Script::Cjk]),
        ("japanese", &[Script::Cjk, Script::Hiragana, Script::Katakana]), ("korean", &[Script::Hangul]),
        ("russian", &[Script::Cyrillic]), ("cyrillic", &[Script::Cyrillic]), ("ukrainian", &[Script::Cyrillic]),
        ("bulgarian", &[Script::Cyrillic]), ("serbian", &[Script::Cyrillic]),
        ("صيني", &[Script::Cjk]), ("ياباني", &[Script::Cjk, Script::Hiragana, Script::Katakana]),
        ("كوري", &[Script::Hangul]), ("روسي", &[Script::Cyrillic]), ("اوكراني", &[Script::Cyrillic]),
    ];
    let normalized = normalize(user_text);
    let mut permitted: Vec<Script> = user_text.chars().filter_map(script_of).collect();
    for (name, scripts) in ASKED {
        if normalized.contains(name) {
            permitted.extend_from_slice(scripts);
        }
    }
    permitted
}

/// [`check_reply`] without the scripts the user wrote in or asked for.
pub fn leaked_words(reply: &str, language: Language, user_text: &str) -> Vec<Issue> {
    let permitted = permitted_scripts(user_text);
    let mut issues = check_reply(reply, language);
    issues.retain(|i| !permitted.contains(&i.script));
    issues
}

/// `reply` with every run of leaked characters removed, when what is left is
/// still a reply: at least two words, and at least two words kept for each
/// removed run. Otherwise `None`, and the reply should not be shown.
pub fn without_leaked_characters(reply: &str, language: Language, user_text: &str) -> Option<String> {
    let permitted = permitted_scripts(user_text);
    let allowed = allowed(language);
    let mut runs = 0usize;
    let mut run_has_letters = false;
    let mut out = String::new();
    let mut gap = false;
    for c in reply.chars() {
        if script_of(c).is_some_and(|s| !allowed.contains(&s) && !permitted.contains(&s)) {
            gap = true;
            if c.is_alphanumeric() && !run_has_letters {
                run_has_letters = true;
                runs += 1;
            }
            continue;
        }
        run_has_letters = false;
        if gap && !c.is_whitespace() && !out.ends_with(char::is_whitespace) && !")]}»”،,.;:!?؟؛".contains(c) {
            out.push(' ');
        }
        gap = false;
        out.push(c);
    }
    for pair in ["()", "[]", "«»", "“”"] {
        out = out.replace(pair, "");
    }
    for p in [',', '.', ';', ':', '!', '?', '،', '؛', '؟'] {
        out = out.replace(&format!(" {p}"), &p.to_string());
    }
    let lines: Vec<String> = out.lines().map(|l| l.split(' ').filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")).collect();
    let out = lines.join("\n").trim().to_string();
    let kept = out.split_whitespace().filter(|w| w.chars().any(char::is_alphanumeric)).count();
    (kept >= 2 && kept >= runs * 2).then_some(out)
}

/// The assistant's tools and the tools small models make up. The user is never
/// told about tools, so a reply that contains one of these names is wrong.
const TOOL_NAMES: &[&str] = &[
    "open_app", "list_files", "search_files", "create_reminder", "schedule_meeting", "set_language", "current_time",
    "delete_file", "move_file", "copy_file", "read_file", "write_file",
];

/// True when the reply names an internal tool, such as `delete_file`.
pub fn names_a_tool(reply: &str) -> bool {
    reply
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .any(|word| TOOL_NAMES.iter().any(|name| word.eq_ignore_ascii_case(name)))
}

/// Words that can sit between "I can" and the verb ("I can also help you delete").
/// Any other word, such as "show you how to", makes the sentence an explanation.
const CLAIM_FILLER_EN: &[&str] = &[
    "also", "easily", "quickly", "now", "then", "gladly", "happily", "certainly", "definitely", "help", "you", "to",
    "go", "ahead", "and", "just", "even", "still", "always", "simply", "please", "only", "with", "me", "for", "have",
    "already", "successfully", "gone",
];

/// How a reply starts to say "I can do this": ability, future, offer.
const CLAIM_EN: &[&[&str]] = &[
    &["i", "can"], &["i", "could"], &["i", "will"], &["ill"], &["i", "am", "able", "to"], &["im", "able", "to"],
    &["i", "am", "going", "to"], &["im", "going", "to"], &["let", "me"], &["want", "me", "to"],
    &["like", "me", "to"], &["ask", "me", "to"], &["shall", "i"],
];

/// How a reply starts to say "I did this". Only a past verb may follow.
const DONE_EN: &[&[&str]] = &[&["i"], &["ive"]];

/// Words that name files and what holds them. A verb on one of these is about
/// files, not about text ("sort the list").
const FILE_NOUNS_EN: &[&str] = &[
    "file", "files", "folder", "folders", "directory", "directories", "document", "documents", "download", "downloads",
    "photo", "photos", "picture", "pictures", "image", "images", "video", "videos", "desktop", "disk", "drive",
    "storage",
];

/// Pronouns that count as files after a verb that changes them ("organize them").
const PRONOUNS_EN: &[&str] = &["them", "these", "those", "everything"];

/// One kind of thing a reply can claim. `tool` is the tool that makes the claim
/// true, or `None` when no tool does.
struct Ability {
    tool: Option<&'static str>,
    /// English verbs as (base, -ing, past).
    verbs_en: &'static [[&'static str; 3]],
    /// Arabic verbs in the first person, present: "I delete".
    verbs_ar: &'static [&'static str],
    /// The same actions as Arabic nouns: "deleting".
    nouns_ar: &'static [&'static str],
}

// Arabic words are in normalized form (see `normalize`).
const ABILITIES: &[Ability] = &[
    Ability {
        tool: None,
        verbs_en: &[
            ["delete", "deleting", "deleted"], ["remove", "removing", "removed"], ["erase", "erasing", "erased"],
            ["trash", "trashing", "trashed"], ["move", "moving", "moved"], ["copy", "copying", "copied"],
            ["rename", "renaming", "renamed"], ["organize", "organizing", "organized"],
            ["organise", "organising", "organised"], ["reorganize", "reorganizing", "reorganized"],
            ["arrange", "arranging", "arranged"], ["rearrange", "rearranging", "rearranged"],
            ["sort", "sorting", "sorted"], ["clean", "cleaning", "cleaned"], ["tidy", "tidying", "tidied"],
            ["declutter", "decluttering", "decluttered"], ["compress", "compressing", "compressed"],
            ["zip", "zipping", "zipped"], ["archive", "archiving", "archived"],
            ["categorize", "categorizing", "categorized"], ["merge", "merging", "merged"],
        ],
        verbs_ar: &["احذف", "امسح", "ازيل", "انقل", "انسخ", "ارتب", "انظم", "افرز", "اصنف", "انظف", "اضغط", "احرك"],
        nouns_ar: &["حذف", "مسح", "ازاله", "نقل", "نسخ", "ترتيب", "تنظيم", "فرز", "تصنيف", "تنظيف", "ضغط", "تحريك"],
    },
    Ability {
        tool: Some("search_files"),
        verbs_en: &[["search", "searching", "searched"], ["find", "finding", "found"], ["locate", "locating", "located"]],
        verbs_ar: &["ابحث", "ادور"],
        nouns_ar: &["بحث"],
    },
    Ability {
        tool: Some("list_files"),
        verbs_en: &[["list", "listing", "listed"], ["show", "showing", "showed"], ["display", "displaying", "displayed"]],
        verbs_ar: &["اعرض"],
        nouns_ar: &["عرض"],
    },
];

/// Words between a claim opener and the verb in Arabic ("اقدر اساعدك في ترتيب").
const CLAIM_FILLER_AR: &[&str] = &[
    "ان", "ايضا", "كذلك", "بسهوله", "بسرعه", "في", "على", "اساعدك", "اساعدكم", "ساعدك", "مساعدتك", "مساعدتكم", "ل",
    "بكل", "سرور", "هنا", "الان", "فورا", "تلقائيا",
];

/// Words that open an Arabic claim: I can, I am able, will, let me, and "مني"
/// as in "تريد مني" (do you want me to).
const CLAIM_AR: &[&str] = &[
    "اقدر", "استطيع", "يمكنني", "بامكاني", "باستطاعتي", "سوف", "رح", "راح", "خلني", "خليني", "دعني", "مني", "تريدني",
    "تحبني", "تبيني", "تبغاني",
];

/// Words that turn an Arabic claim into a refusal: "لا أستطيع".
const NEGATION_AR: &[&str] = &["لا", "لن", "ما", "مش", "مو", "غير"];

const FILE_NOUNS_AR: &[&str] = &[
    "ملف", "ملفات", "مجلد", "مجلدات", "فولدر", "فولدرات", "مستند", "مستندات", "وثيقه", "وثائق", "صوره", "صور",
    "فيديو", "فيديوهات", "تنزيلات", "جهاز", "قرص", "كمبيوتر", "لابتوب", "تخزين",
];

/// Endings that put an object on a verb or noun: احذفها, حذفه.
const OBJECT_ENDING_AR: &[&str] = &["ها", "هم", "هن", "ه"];

/// Endings that say "for you": احذفلك.
const FOR_YOU_ENDING_AR: &[&str] = &["لك", "لكم"];

/// When a reply is a text the user asked for, its "I" is not the assistant.
const WRITING_CUES: &[&str] = &[
    "write", "draft", "compose", "rewrite", "email", "letter", "story", "poem", "essay", "paragraph", "message",
    "اكتب", "اكتبي", "ايميل", "رساله", "قصه", "قصيده", "خطاب", "مقال", "صياغه", "صيغ",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ending {
    Bare,
    Object,
    ForYou,
}

/// How `word` is one of `list`, if it is: alone, with an object ending, or with
/// "for you". A prefix such as و or ب in front is allowed.
fn ending(word: &str, list: &[&str]) -> Option<Ending> {
    forms(word).iter().find_map(|stem| {
        list.iter().find_map(|form| {
            let rest = stem.strip_prefix(form)?;
            if rest.is_empty() {
                Some(Ending::Bare)
            } else if OBJECT_ENDING_AR.contains(&rest) {
                Some(Ending::Object)
            } else if FOR_YOU_ENDING_AR.contains(&rest) {
                Some(Ending::ForYou)
            } else {
                None
            }
        })
    })
}

/// An Arabic file noun, with a prefix (بالملفات) or a possessive ending (ملفاتك).
fn arabic_file_noun(word: &str) -> bool {
    forms(word).iter().any(|stem| {
        FILE_NOUNS_AR.contains(stem)
            || ["ك", "ي", "ه", "ها", "هم", "كم", "نا"]
                .iter()
                .any(|end| stem.strip_suffix(end).is_some_and(|s| FILE_NOUNS_AR.contains(&s)))
    })
}

fn english_claim(words: &[&str], tools: &[&str]) -> Option<String> {
    for i in 0..words.len() {
        let (after, done) = match (starts_with_any(words, i, CLAIM_EN), starts_with_any(words, i, DONE_EN)) {
            (Some(n), _) => (i + n, false),
            (None, Some(n)) => (i + n, true),
            (None, None) => continue,
        };
        let verb_at = after + words[after..].iter().take(3).take_while(|w| CLAIM_FILLER_EN.contains(w)).count();
        let Some(word) = words.get(verb_at) else { continue };
        let next = &words[verb_at + 1..];
        for ability in ABILITIES.iter().filter(|a| !a.tool.is_some_and(|t| tools.contains(&t))) {
            let is_verb = ability
                .verbs_en
                .iter()
                .any(|[base, ing, past]| if done { word == past } else { word == base || word == ing });
            let object = next.iter().take(3).any(|w| {
                FILE_NOUNS_EN.contains(w) || (ability.tool.is_none() && PRONOUNS_EN.contains(w))
            });
            if is_verb && object {
                return Some(words[i..(verb_at + 4).min(words.len())].join(" "));
            }
        }
    }
    None
}

fn starts_with_any(words: &[&str], i: usize, starts: &[&[&str]]) -> Option<usize> {
    starts.iter().find(|s| words[i..].starts_with(s)).map(|s| s.len())
}

fn arabic_claim(words: &[&str], tools: &[&str]) -> Option<String> {
    let missing = || ABILITIES.iter().filter(|a| !a.tool.is_some_and(|t| tools.contains(&t)));
    let acts_on_files = |at: usize, ending: Ending| {
        ending == Ending::Object || words[at + 1..].iter().take(3).any(|w| arabic_file_noun(w))
    };
    for i in 0..words.len() {
        let word = words[i];
        // سأحذف and هحذف (Egyptian) are claims by themselves.
        let fused = word.strip_prefix('س').map(str::to_string).or_else(|| word.strip_prefix('ه').map(|v| format!("ا{v}")));
        if let Some(verb) = fused {
            let hit = missing().any(|a| ending(&verb, a.verbs_ar).is_some_and(|e| acts_on_files(i, e)));
            if hit {
                return Some(words[i..(i + 4).min(words.len())].join(" "));
            }
        }
        // "تم" at the start of the clause says it was done.
        let done = i < 2 && word == "تم";
        if !done && !CLAIM_AR.contains(&word) || i > 0 && NEGATION_AR.contains(&words[i - 1]) {
            continue;
        }
        let verb_at = i + 1 + words[i + 1..].iter().take(3).take_while(|w| CLAIM_FILLER_AR.contains(w)).count();
        let Some(verb) = words.get(verb_at) else { continue };
        let hit = missing().any(|a| {
            let by_verb = if done { None } else { ending(verb, a.verbs_ar) };
            by_verb.or_else(|| ending(verb, a.nouns_ar)).is_some_and(|e| acts_on_files(verb_at, e))
        });
        if hit {
            return Some(words[i..(verb_at + 4).min(words.len())].join(" "));
        }
    }
    None
}

/// Finds the first place where a reply says or offers that the assistant can
/// change, search or list files, when no tool in `tools` does it. `tools` are
/// the names of the tools offered in this conversation. Advice to the user
/// ("you can delete old files", "I recommend organizing them") and
/// explanations ("I can show you how to delete files") are not claims: a claim
/// is "I" with can, will, let me, want me to or a past verb, then the verb and
/// a file word within three words. A request to write or translate is not
/// checked, because the "I" in the text is not the assistant. Returns the words
/// of the claim.
pub fn claims_missing_ability(user_text: &str, reply: &str, tools: &[&str]) -> Option<String> {
    let user = normalize(user_text);
    if asks_to_translate(user_text) || has_term(&split_words(&user), WRITING_CUES) {
        return None;
    }
    let text = normalize(&strip_quoted(reply));
    text.split(['.', '!', '?', '؟', '؛', ';', '\n', ':', ',', '،']).find_map(|clause| {
        let words = split_words(clause);
        english_claim(&words, tools).or_else(|| arabic_claim(&words, tools))
    })
}

/// True when the reply is in the wrong one of the two scripts for the request.
/// Arabic request: the reply is a sentence (three words or more) only in Latin
/// letters; a short "OK" or a name is not wrong, and not when the user asked
/// for a translation or for English. Latin request: the reply is only in Arabic
/// letters, even one word; not when the user asked for a translation or named
/// Arabic. A request with both scripts is never checked.
pub fn replied_in_wrong_language(user_text: &str, reply: &str) -> bool {
    match (lang::pure_script(user_text), lang::pure_script(reply)) {
        (Some(Language::Arabic), Some(Language::English)) => {
            reply.split_whitespace().count() >= 3
                && !asks_to_translate(user_text)
                && !normalize(user_text).contains("انجليز")
        }
        (Some(Language::English), Some(Language::Arabic)) => {
            !asks_to_translate(user_text) && !normalize(user_text).contains("arab")
        }
        _ => false,
    }
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

// Words that may stand around a time in a message that is only a time. Normalized form.
const TIME_MARKERS: &[&str] = &["at", "oclock", "am", "pm", "الساعه", "عند", "ص", "م"];
const TIME_FILLER: &[&str] = &[
    "around", "about", "by", "the", "its", "time", "please", "hey", "hi", "ok", "okay", "morning", "evening",
    "night", "noon", "afternoon", "حوالي", "تقريبا", "في", "ب", "صباحا", "مساء", "ظهرا", "عصرا", "ليلا", "فجرا",
    "الصباح", "المساء", "الظهر", "الليل", "من", "فضلك", "لو", "سمحت", "طيب", "يا", "بعد",
];

/// A time of day written as digits: `11`, `3:30`, `3pm`, `3:30pm`. Returns it
/// with `true` when it has a minute part or am/pm, which only a time has.
fn time_token(word: &str) -> Option<(String, bool)> {
    let (clock, suffix) = match word.find(|c: char| c.is_alphabetic()) {
        Some(i) => (&word[..i], &word[i..]),
        None => (word, ""),
    };
    if !matches!(suffix, "" | "am" | "pm") {
        return None;
    }
    let (hour, minute) = match clock.split_once(':') {
        Some((h, m)) => (h, Some(m)),
        None => (clock, None),
    };
    let digits = |t: &str, len: std::ops::RangeInclusive<usize>| -> Option<u32> {
        if len.contains(&t.len()) && t.bytes().all(|b| b.is_ascii_digit()) { t.parse().ok() } else { None }
    };
    let hour_n = digits(hour, 1..=2)?;
    let minute_n = match minute {
        Some(m) => Some(digits(m, 2..=2)?),
        None => None,
    };
    let valid = if suffix.is_empty() { hour_n <= 23 } else { (1..=12).contains(&hour_n) };
    (valid && minute_n.is_none_or(|m| m <= 59)).then(|| (word.to_string(), minute.is_some() || !suffix.is_empty()))
}

/// When the whole message is only a time of day ("الساعة 11", "at 3:30 pm",
/// "11:15") and asks for nothing, returns that time as written. A model asked
/// about such a message tends to answer it as the current time, which it does
/// not know, so the caller asks the user what they want instead. A message with
/// any other word in it, such as "what time is it" or "remind me at 11", is not
/// a bare time.
pub fn bare_time(user_text: &str) -> Option<String> {
    let text = normalize(user_text).replace("a.m", "am").replace("p.m", "pm");
    let mut time = None;
    let mut marked = false;
    for word in text.split(|c: char| !c.is_alphanumeric() && c != ':').map(|w| w.trim_matches(':')) {
        if word.is_empty() {
            continue;
        }
        if let Some((token, own_marker)) = time_token(word) {
            if time.replace(token).is_some() {
                return None;
            }
            marked |= own_marker;
        } else if TIME_MARKERS.contains(&word) {
            marked = true;
        } else if !TIME_FILLER.contains(&word) {
            return None;
        }
    }
    time.filter(|_| marked)
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

    #[test]
    fn tool_names_in_a_reply_are_found() {
        for reply in ["Use delete_file.", "I called `open_app` for you", "SET_LANGUAGE failed", "the move_file tool"] {
            assert!(names_a_tool(reply), "{reply}");
        }
        for reply in ["Opening the app now", "Please open the file", "تم فتح الملف", "my_file.txt is here"] {
            assert!(!names_a_tool(reply), "{reply}");
        }
    }

    #[test]
    fn only_a_purely_latin_reply_to_a_purely_arabic_request_is_wrong() {
        assert!(replied_in_wrong_language("شغلي أغنية", "Sorry, I can't play music."));
        assert!(!replied_in_wrong_language("شغلي أغنية", "عذرًا، لا أستطيع"));
        assert!(!replied_in_wrong_language("شغلي أغنية", "عذرًا, I can't (sorry)"));
        assert!(!replied_in_wrong_language("شغلي أغنية", "12:30"));
        assert!(!replied_in_wrong_language("شغلي أغنية", "ok"));
        assert!(!replied_in_wrong_language("شغلي أغنية", "Sorry, no."));
        assert!(!replied_in_wrong_language("Play a song", "Sorry"));
        assert!(!replied_in_wrong_language("ترجم صباح الخير إلى الإنجليزية", "Good morning"));
        assert!(!replied_in_wrong_language("اكتب رسالة بالانجليزي", "Hello"));
        assert!(!replied_in_wrong_language("اكتب رسالة بالإنجليزية", "Hello"));
    }

    #[test]
    fn only_a_purely_arabic_reply_to_a_purely_latin_request_is_wrong() {
        assert!(replied_in_wrong_language("What is the capital of Egypt? One word.", "القاهرة"));
        assert!(replied_in_wrong_language("Play a song", "عذرًا، لا أستطيع تشغيل الموسيقى"));
        // Both scripts in the reply or in the request: not checked.
        assert!(!replied_in_wrong_language("What is the capital of Egypt?", "القاهرة (Cairo)"));
        assert!(!replied_in_wrong_language("What is القاهرة in English?", "Cairo"));
        assert!(!replied_in_wrong_language("What is القاهرة?", "عاصمة مصر"));
        // No letters in the reply, or the reply is in the request's language.
        assert!(!replied_in_wrong_language("What is 2+2?", "٤"));
        assert!(!replied_in_wrong_language("What is the capital of Egypt?", "Cairo"));
        // Translation and Arabic requests.
        for user in ["Translate to Arabic: good morning", "translate good morning", "Say good morning in Arabic", "Write it in arabic"] {
            assert!(!replied_in_wrong_language(user, "صباح الخير"), "{user}");
        }
    }

    #[test]
    fn chinese_punctuation_and_rare_ideographs_in_arabic_are_reported() {
        for reply in [
            "الواي فاي，هو شبكة لاسلكية",
            "الفرق（بين）الاثنين",
            "الفرق بين الاثنين。",
            "الواي فاي是一种شبكة",
            "هذا ＷｉＦｉ جيد",
            "شبكة \u{2F800}",
            "شبكة \u{30000}",
        ] {
            let issues = check_reply(reply, Language::Arabic);
            assert!(!issues.is_empty() && issues.iter().all(|i| i.script == Script::Cjk), "{reply}: {issues:?}");
            assert!(!check_reply(reply, Language::English).is_empty(), "{reply}");
        }
        // Western punctuation and full-width digits are not reported.
        assert!(check_reply("الواي فاي، هو (شبكة): نعم؟ «تمام» ٢٠٢٦", Language::Arabic).is_empty());
        assert!(check_reply("الكود ２０٢٦", Language::Arabic).is_empty());
    }

    const ALL_TOOLS: &[&str] =
        &["open_app", "list_files", "search_files", "create_reminder", "schedule_meeting", "set_language"];

    fn claim(user: &str, reply: &str) -> Option<String> {
        claims_missing_ability(user, reply, ALL_TOOLS)
    }

    #[test]
    fn claims_to_change_files_are_found_in_english() {
        for reply in [
            "I can delete the old files for you.",
            "Sure! I can also organize your downloads folder.",
            "I can help you organize your files into folders.",
            "I'll move those documents to a new folder.",
            "Let me clean up your desktop.",
            "I'm able to rename the files in bulk.",
            "I can help you with sorting the photos by date.",
            "Do you want me to delete them?",
            "Would you like me to organize your documents?",
            "You can ask me to remove duplicate files.",
            "I have deleted the files you listed.",
            "I just moved your folder to the desktop.",
            "I can't open that app, but I can delete your files.",
            "I will definitely compress the folder.",
            "Shall I archive the videos?",
        ] {
            assert!(claim("my disk is full", reply).is_some(), "{reply}");
        }
    }

    #[test]
    fn advice_and_explanations_are_not_claims_in_english() {
        for reply in [
            "You can delete old files to free up space.",
            "I recommend organizing your files into folders by project.",
            "I suggest you delete duplicates and empty the trash.",
            "To organize your files, create a folder for each project.",
            "I can't delete files, but you can do it from the file manager.",
            "I cannot move your files. I can only explain how.",
            "I can show you how to delete files safely.",
            "I can explain how to organize your documents.",
            "I can help you decide which files to delete.",
            "Let me know if you want tips on sorting your photos.",
            "Let me explain how to copy files to a USB drive.",
            "Do you want me to explain how to delete a file?",
            "I would back up your documents before cleaning the disk.",
            "I can help you organize your thoughts.",
            "I can sort the list alphabetically.",
            "I can remove the second paragraph.",
            "I can copy this text into a table.",
            "Deleting files you do not need is a good habit.",
            "You should move your photos to an external drive.",
            "I moved on to the next question.",
            "I can open the music app.",
            "I can set a reminder.",
            "To free up space, I'd delete large files you no longer need.",
            "I can help you plan a cleanup: first sort your files by size.",
            "I can tell you which files are safe to delete.",
            "I can help you write a script that deletes the files.",
            "I can't see your files, so you should sort them yourself.",
            "I can summarize the document for you.",
        ] {
            assert_eq!(claim("my disk is full", reply), None, "{reply}");
        }
    }

    #[test]
    fn claims_to_change_files_are_found_in_arabic() {
        for reply in [
            "أقدر أحذف الملفات القديمة لك.",
            "يمكنني ترتيب ملفاتك في مجلدات.",
            "أستطيع نقل المستندات إلى مجلد جديد.",
            "سأحذف الملفات المكررة الآن.",
            "ساحذفها لك.",
            "هرتب الفولدرات بتاعتك.",
            "هحذفها دلوقتي.",
            "أقدر أساعدك في تنظيف الجهاز وحذف الملفات.",
            "تقدر تطلب مني حذف الملفات.",
            "تريد مني أن أنظم المجلدات؟",
            "خلني أرتب الصور.",
            "راح انقل الملفات لك.",
            "تم حذف الملفات القديمة.",
            "أقدر أحذفلك الملفات.",
            "رح أنسخها للفلاشة.",
            "ممكن اقدر امسح ملفاتك القديمة.",
        ] {
            assert!(claim("جهازي ممتلئ", reply).is_some(), "{reply}");
        }
    }

    #[test]
    fn advice_and_explanations_are_not_claims_in_arabic() {
        for reply in [
            "يمكنك حذف الملفات القديمة لتوفير مساحة.",
            "أنصحك بترتيب ملفاتك في مجلدات حسب المشروع.",
            "احذف الملفات المكررة وأفرغ سلة المحذوفات.",
            "تقدر تنقل الصور إلى قرص خارجي.",
            "لا أستطيع حذف الملفات، لكن يمكنك فعل ذلك من مدير الملفات.",
            "لا أقدر أنقل ملفاتك.",
            "أقدر أشرح لك كيف تحذف الملفات.",
            "أقدر أساعدك في اختيار الملفات التي تريد حذفها.",
            "أقدر أرتب لك الأفكار.",
            "يمكنني ترتيب القائمة أبجديًا.",
            "سأشرح لك طريقة ترتيب الملفات.",
            "يفضل حذف الملفات المؤقتة كل شهر.",
            "حذف الملفات القديمة عادة جيدة.",
            "إذا حذفت ملفًا فسيذهب إلى سلة المحذوفات.",
            "سأفتح تطبيق الموسيقى.",
            "ساعدك في كتابة الرسالة.",
            "أقدر أعطيك نصائح لحذف الملفات القديمة.",
            "أقدر أكتب لك سكربت يحذف الملفات.",
            "راح أشرح لك كيف تنقل الملفات.",
            "أنا ما أقدر أحذف الملفات بس تقدر تحذفها انت.",
            "مش هقدر احذف ملفاتك.",
            "يمكنني تلخيص المستند لك.",
            "هنا بعض النصائح لتنظيم الملفات.",
        ] {
            assert_eq!(claim("جهازي ممتلئ", reply), None, "{reply}");
        }
    }

    #[test]
    fn a_claim_is_only_missing_when_no_tool_does_it() {
        let reply = "I can search your files for it.";
        assert_eq!(claim("find my report", reply), None);
        assert!(claims_missing_ability("find my report", reply, &["open_app", "list_files"]).is_some());
        let reply = "أقدر أبحث عن الملف لك.";
        assert_eq!(claim("دور على التقرير", reply), None);
        assert!(claims_missing_ability("دور على التقرير", reply, &[]).is_some());
        // The writer model is offered no tools, so it may not claim to list files either.
        assert!(claims_missing_ability("hi", "I can show you the files in Documents.", &[]).is_some());
        assert_eq!(claim("hi", "I can show you the files in Documents."), None);
        // Nothing offers a way to delete, whatever else is offered.
        assert!(claim("hi", "I can delete your files.").is_some());
    }

    #[test]
    fn text_the_user_asked_for_is_not_checked() {
        let reply = "Hi team, I have moved the files to the shared folder.";
        assert_eq!(claim("write an email to my team about the files", reply), None);
        assert_eq!(claim("اكتب رسالة لفريقي", "مرحبا، لقد نقلت الملفات، تم حذف الملفات القديمة."), None);
        assert_eq!(claim("translate to French", "I can delete the files."), None);
        assert!(claim("what should I do?", reply).is_some());
        // A quoted sentence is not the assistant speaking.
        assert_eq!(claim("hi", "A scammer might say \"I can delete your files\" to scare you."), None);
    }

    #[test]
    fn leaked_runs_are_removed_only_when_a_reply_is_left() {
        let ar = |t: &str| without_leaked_characters(t, Language::Arabic, "سؤال");
        assert_eq!(ar("الواي فاي，هو اتصال 移动 لاسلكي (移动)."), Some("الواي فاي هو اتصال لاسلكي.".into()));
        assert_eq!(ar("الواي فاي是一种شبكة لاسلكية"), Some("الواي فاي شبكة لاسلكية".into()));
        assert_eq!(ar("移动"), None);
        assert_eq!(ar("شبكة 移动网络是一种无线连接"), None);
        // Scripts the user wrote or asked for are not leaks.
        let kept = "It says 你好 and means hello.";
        assert_eq!(without_leaked_characters(kept, Language::English, "say hello in Chinese"), Some(kept.into()));
        assert!(leaked_words("Привет", Language::Arabic, "ترجم إلى الروسية").is_empty());
        assert!(!leaked_words("Привет", Language::Arabic, "ترجم إلى الإنجليزية").is_empty());
    }

    #[test]
    fn the_claim_is_returned_for_review() {
        let english = claim("hi", "Sure! I can also organize your downloads folder.");
        assert_eq!(english.as_deref(), Some("i can also organize your downloads folder"));
        let arabic = claim("hi", "يمكنني ترتيب ملفاتك في مجلدات.");
        assert_eq!(arabic.as_deref(), Some("يمكنني ترتيب ملفاتك في مجلدات"));
    }
}
