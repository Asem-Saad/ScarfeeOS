//! One turn of the assistant: user text in, a description of what should
//! happen out. Nothing here runs a tool. Time and live data come from code,
//! never from the model, and every reply is checked before it is returned.

use serde_json::{json, Value};

use crate::datetime::Date;
use crate::guard::{self, Issue, LiveData};
use crate::hijri::{self, HijriDate};
use crate::lang::{self, Language};
use crate::provider::{LocalProvider, ModelReply, RawToolCall};
use crate::tools::{self, Clock, ClockError, LocalTime, Resolution, ToolCall};

/// The model behind the assistant. A trait so tests need no server.
pub trait Model {
    fn chat(&self, system: &str, user: &str, tools: &Value) -> Result<ModelReply, String>;
}

impl Model for LocalProvider {
    fn chat(&self, system: &str, user: &str, tools: &Value) -> Result<ModelReply, String> {
        self.complete_with_tools(system, user, tools)
    }
}

/// What the caller should do after one turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The device's local time, read from the clock.
    Time(LocalTime),
    /// A named place or zone was asked for. Only the device's zone is supported.
    UnsupportedTimeZone(String),
    /// News and weather need the network, which the local assistant does not use.
    CannotDoOffline(LiveData),
    /// Religious content is not provided. Nothing was sent to the model.
    Declined,
    /// The Hijri date for a Gregorian date in the text, computed in code. It is
    /// approximate: official calendars and moon sighting can differ by a day or two.
    HijriApprox { gregorian: Date, hijri: HijriDate },
    /// A Hijri conversion was asked for but no Gregorian date could be read
    /// from the text. Ask the user for one in digits.
    MissingDate,
    /// A text answer, with every word that is in a script that does not belong.
    Reply { text: String, issues: Vec<Issue> },
    /// The tool may run as it is.
    Run(ToolCall),
    /// The time could mean two things. Ask the user to pick.
    NeedsClarification(ToolCall),
    /// The tool changes a setting. Run it only after the user says yes.
    NeedsConfirmation(ToolCall),
    /// The text names no date or time. Ask the user. The call has an empty time.
    MissingTime(ToolCall),
    /// The whole message is only a time of day, such as "الساعة 11", with no
    /// request. Ask the user what they want at that time. The model was not
    /// called, so the time is never answered as the current time.
    TimeOnly(String),
    Error(String),
}

pub struct Assistant<'a> {
    model: &'a dyn Model,
    writer: Option<&'a dyn Model>,
    clock: &'a dyn Clock,
}

impl<'a> Assistant<'a> {
    pub fn new(model: &'a dyn Model, clock: &'a dyn Clock) -> Self {
        Assistant { model, writer: None, clock }
    }

    /// Adds a second model that writes the text replies. The first model is
    /// still asked first, with tools, and its tool calls are used as they are.
    pub fn with_writer(mut self, writer: &'a dyn Model) -> Self {
        self.writer = Some(writer);
        self
    }

    pub fn handle(&self, user_text: &str, language: Language) -> Outcome {
        // Islam, Salah and Imam are also people's names, so a matching word alone
        // cannot decline "Remind me to call Islam". Only commands get through
        // here: any text answer is thrown away, so the model's words never show.
        let religious = guard::asks_religious(user_text);
        match guard::asks_for_live_data(user_text) {
            _ if religious => {}
            Some(LiveData::Time) => return self.time_outcome(requested_place(user_text).as_deref()),
            Some(live) => return Outcome::CannotDoOffline(live),
            None => {}
        }
        if !religious {
            if let Some(time) = guard::bare_time(user_text) {
                return Outcome::TimeOnly(time);
            }
        }
        if !religious && guard::asks_for_hijri(user_text) {
            return match gregorian_date_in(user_text) {
                Some(date) => Outcome::HijriApprox { gregorian: date, hijri: hijri::to_hijri_approx(date) },
                None => Outcome::MissingDate,
            };
        }
        let now = match self.clock.now() {
            Ok(now) => now,
            Err(e) => return Outcome::Error(e.to_string()),
        };
        let system = system_prompt(now, true);
        let translating = guard::asks_to_translate(user_text);
        let tools = tool_schema_for(!translating);
        match self.model.chat(&system, user_text, &tools) {
            Ok(ModelReply::Text(_)) | Err(_) if religious => Outcome::Declined,
            Err(e) => Outcome::Error(e),
            Ok(ModelReply::Text(text)) => match self.written_by_writer(user_text, language, now) {
                Some(outcome) => outcome,
                None => self.checked_reply(self.model, &system, user_text, language, &tools, text),
            },
            Ok(ModelReply::ToolCalls(calls)) => self.tool_outcome(user_text, now, translating, calls),
        }
    }

    /// The writer's reply to the same text, without tools. `None` when there is
    /// no writer or it failed, so the command model's own text is used instead.
    fn written_by_writer(&self, user_text: &str, language: Language, now: LocalTime) -> Option<Outcome> {
        let writer = self.writer?;
        let system = system_prompt(now, false);
        let no_tools = json!([]);
        match writer.chat(&system, user_text, &no_tools) {
            Ok(ModelReply::Text(text)) => Some(self.checked_reply(writer, &system, user_text, language, &no_tools, text)),
            Ok(ModelReply::ToolCalls(_)) | Err(_) => None,
        }
    }

    /// Asks `model` once more when the reply has words in a foreign script,
    /// is in the other language than the request (English for Arabic, Arabic for English), names an internal tool, or says it can
    /// change or search files with no tool for it. Keeps whichever reply has fewer of these problems.
    /// Leaked characters never reach the user: when the kept reply still has some,
    /// they are removed if a sensible reply is left, otherwise a fixed apology is returned.
    fn checked_reply(
        &self,
        model: &dyn Model,
        system: &str,
        user_text: &str,
        language: Language,
        tools: &Value,
        text: String,
    ) -> Outcome {
        let offered: Vec<&str> =
            tools.as_array().into_iter().flatten().filter_map(|t| t["function"]["name"].as_str()).collect();
        let count = |reply: &str| {
            guard::leaked_words(reply, language, user_text).len()
                + usize::from(guard::replied_in_wrong_language(user_text, reply))
                + usize::from(guard::names_a_tool(reply))
                + usize::from(guard::claims_missing_ability(user_text, reply, &offered).is_some())
        };
        let mut text = text;
        let problems = count(&text);
        if problems > 0 {
            let mut retry = system.to_string();
            if !guard::leaked_words(&text, language, user_text).is_empty() {
                let name = match language {
                    Language::English => "English",
                    Language::Arabic => "Arabic",
                };
                retry.push_str(&format!(
                    " Your last reply had words in a foreign script. Reply again in {name}, with no words in other scripts."
                ));
            }
            if guard::replied_in_wrong_language(user_text, &text) {
                retry.push_str(if lang::pure_script(user_text) == Some(Language::English) {
                    " Your last reply was in Arabic, but the user wrote in English. Reply again in English only."
                } else {
                    " Your last reply was in English, but the user wrote in Arabic. Reply again in Arabic only, in the user's dialect."
                });
            }
            if guard::names_a_tool(&text) {
                retry.push_str(
                    " Your last reply named a tool or function. Reply again in plain words without naming any tool or function.",
                );
            }
            if guard::claims_missing_ability(user_text, &text, &offered).is_some() {
                retry.push_str(
                    " Your last reply said or offered something you cannot do here. Reply again without saying or offering that you can do it; give general advice only.",
                );
            }
            if let Ok(ModelReply::Text(again)) = model.chat(&retry, user_text, tools) {
                if count(&again) < problems {
                    text = again;
                }
            }
        }
        if guard::leaked_words(&text, language, user_text).is_empty() {
            return Outcome::Reply { text, issues: Vec::new() };
        }
        let text = guard::without_leaked_characters(&text, language, user_text)
            .unwrap_or_else(|| (if language == Language::Arabic { APOLOGY_AR } else { APOLOGY_EN }).to_string());
        Outcome::Reply { text, issues: Vec::new() }
    }

    fn time_outcome(&self, timezone: Option<&str>) -> Outcome {
        match tools::current_time(self.clock, timezone) {
            Ok(now) => Outcome::Time(now),
            Err(ClockError::UnsupportedTimeZone(zone)) => Outcome::UnsupportedTimeZone(zone),
            Err(e) => Outcome::Error(e.to_string()),
        }
    }

    fn tool_outcome(&self, user_text: &str, now: LocalTime, translating: bool, mut calls: Vec<RawToolCall>) -> Outcome {
        if translating && calls.iter().any(|c| c.name == "set_language") {
            return Outcome::Error("set_language: a translation request does not change the system language".into());
        }
        calls.dedup();
        let [raw] = calls.as_slice() else {
            return Outcome::Error(format!("model proposed {} tool calls, expected one", calls.len()));
        };
        let call = match parse_call(raw) {
            Ok(call) => call,
            Err(e) => return Outcome::Error(e),
        };
        let call = match call {
            ToolCall::CreateReminder { time, repeat, .. } => {
                ToolCall::CreateReminder { text: reminder_text(user_text), time, repeat }
            }
            ToolCall::ScheduleMeeting { time, attendees, repeat, .. } => {
                let (title, attendees) = lang::meeting_from_user_text(user_text, &attendees);
                let title = subject_title(user_text, &title).unwrap_or(title);
                ToolCall::ScheduleMeeting { title, time, attendees, repeat }
            }
            other => other,
        };
        match call {
            ToolCall::CurrentTime { timezone } => self.time_outcome(timezone.as_deref()),
            ToolCall::SetLanguage { .. } => Outcome::NeedsConfirmation(call),
            call => match tools::resolve_call(user_text, now.date, call.clone()) {
                Resolution::Ready(call) => Outcome::Run(call),
                Resolution::NeedsClarification(call) => Outcome::NeedsClarification(call),
                Resolution::MissingTime => Outcome::MissingTime(call),
            },
        }
    }
}

/// Words that open a request for a meeting ("ابي اجتماع", "I want a meeting"). They are
/// the user's command and never a title.
const WANT_PHRASES: &[&[&str]] = &[
    &["ابي"], &["ابغي"], &["ابغا"], &["ابيك"], &["ابغاك"], &["بدي"], &["عايز"], &["عاوز"], &["عايزه"], &["عاوزه"],
    &["محتاج"], &["محتاجه"], &["احتاج"], &["i", "want"], &["i", "need"], &["id", "like"], &["i", "would", "like"],
    &["we", "want"], &["we", "need"],
];

/// Words that open the subject of a meeting ("... عن المشروع الجديد").
const SUBJECT_MARKERS: &[&str] = &["عن", "بخصوص", "حول", "about", "regarding"];

fn core_word(token: &str) -> String {
    guard::normalize(token.trim_matches(|c: char| !c.is_alphanumeric()))
}

/// When the default `title` of a meeting begins with the user's command phrase
/// ("ابي اجتماع"), the subject the user named after "عن" / "about" if there is
/// one, else the title without the command phrase. `None` when the title is
/// not a command phrase.
fn subject_title(user_text: &str, title: &str) -> Option<String> {
    let title_tokens: Vec<&str> = title.split_whitespace().collect();
    let title_words: Vec<String> = title_tokens.iter().map(|t| core_word(t)).collect();
    let want = WANT_PHRASES.iter().find(|p| title_words.len() >= p.len() && p.iter().zip(&title_words).all(|(a, b)| a == b))?;
    let own = lang::without_command_and_time(user_text);
    let tokens: Vec<&str> = own.split_whitespace().collect();
    let words: Vec<String> = tokens.iter().map(|t| core_word(t)).collect();
    if let Some(marker) = words.iter().position(|w| SUBJECT_MARKERS.contains(&w.as_str())) {
        let subject: Vec<&str> = tokens[marker + 1..]
            .iter()
            .copied()
            .zip(&words[marker + 1..])
            .take_while(|(_, w)| !matches!(w.as_str(), "with" | "مع"))
            .map(|(t, _)| t)
            .collect();
        let subject = subject.join(" ").trim_matches(|c: char| c.is_whitespace() || ",.;:!?،؛؟".contains(c)).to_string();
        if !subject.is_empty() {
            return Some(subject);
        }
    }
    let rest = title_tokens[want.len()..].join(" ");
    Some(if rest.is_empty() { (if lang::has_letters(user_text, Language::Arabic) { "اجتماع" } else { "Meeting" }).to_string() } else { rest })
}

/// Shown instead of a reply that is mostly in a script that does not belong.
const APOLOGY_EN: &str = "Sorry, I could not write a clear answer. Please ask again.";
const APOLOGY_AR: &str = "عذرًا، لم أتمكن من كتابة إجابة واضحة. من فضلك اسأل مرة أخرى.";

/// The reminder text: the user's own words without the command and the time,
/// so the model's wording, which can change the meaning, never reaches the
/// reminder. When no task words are left, a fixed neutral title in the user's
/// language, never the model's text.
fn reminder_text(user_text: &str) -> String {
    let own = lang::without_command_and_time(user_text);
    if own.is_empty() {
        lang::default_reminder_title(user_text).to_string()
    } else {
        own
    }
}

/// Rules for how the model talks to the user. The Arabic lines say the same as
/// the English ones before them, because small models follow an instruction
/// better in the language they are asked to answer in.
const REPLY_RULES: &str = "\
Always reply in the language and dialect of the user's latest message. \
If it is Arabic, reply in Arabic, in the same dialect (Gulf, Egyptian, Levantine, Maghrebi or Modern Standard), and never in English. \
If it is English, reply in English only, and never in Arabic, even when the question is about Egypt, Arabic or anything Arabic. \
If it mixes both languages, reply in the language of most of its words. \
Only a request to translate, or to write in a named language, changes the language of the reply. \
ردّ دائمًا بلغة رسالة المستخدم ولهجتها. إذا كتب المستخدم بالعربية فاكتب بالعربية فقط، ولا تردّ بالإنجليزية أبدًا. \
وإذا كتب بالإنجليزية فاكتب بالإنجليزية فقط، وإذا خلط اللغتين فاتبع اللغة الغالبة على رسالته. \
Never mention tools or functions, or their names, to the user, and never say \"the available tools\". \
لا تذكر الأدوات أو الدوال أو أسماءها للمستخدم أبدًا. \
Answer questions and writing requests yourself, directly: unit conversions, maths, writing emails and other texts, translations, explanations, advice. \
Never refuse because no tool fits. \
When you give advice, keep it general and answer only the question. Do not recommend apps or features of this computer, such as reminders, \
and do not offer to open, delete, move, organize or search anything, unless a tool for it is offered to you in this conversation and the user asked for that action. \
عند تقديم نصيحة اجعلها عامة وأجب عن السؤال فقط، ولا توصِ بتطبيقات أو ميزات في هذا الجهاز، مثل التذكيرات، ولا تعرض فتح شيء أو حذفه أو نقله أو ترتيبه أو البحث فيه، إلا إذا طلب المستخدم ذلك وتوفرت لك أداة له في هذه المحادثة. \
أجب بنفسك مباشرة عن الأسئلة وطلبات الكتابة والتحويل والترجمة، ولا ترفض بحجة عدم وجود أداة. \
If you truly cannot do something on this computer, say so briefly in the user's language, without naming any tool. \
Never invent data.";

/// Added for the model that has tools.
const TOOL_RULES: &str = "\
Tools are only for acting on this computer: opening apps, listing or searching files, reminders, meetings and the system language. \
Call a tool only when one fits the request. \
Playing music, a song or a playlist means opening the music app: call open_app with the name \"music\". \
Never say you have no tool for music. \
تشغيل الموسيقى أو أغنية يعني فتح تطبيق الموسيقى: استدعِ open_app باسم \"music\"، ولا تقل إنه لا توجد أداة للموسيقى. \
Looking for a file, in any dialect, is search_files with the name as spoken, never open_app: \"دوّرلي على ملف\", \"دور لي على ملف\", \"فتّش عن ملف\". \
البحث عن ملف بأي لهجة (دوّر، فتّش، ابحث) هو search_files وليس open_app. \
Changing the system language is set_language: \"خلّ اللغة عربي\", \"خلي اللغة إنجليزي\", \"غيّر اللغة\". \
تغيير لغة النظام (خلّ، خلي، غيّر) هو set_language. \
A polite request or a question about ability, in any dialect, is a command and not a question: \
\"Can you open the calculator?\", \"ممكن تفتح الآلة الحاسبة\", \"فيك تفتحلي الإيميل؟\", \"لو سمحت افتح الإيميل\". \
Do it and do not ask back. \
الطلب المهذب بأي لهجة، مثل فيك وممكن ولو سمحت، هو أمر وليس سؤالًا. نفّذه مباشرة ولا تسأل المستخدم.";

/// The system prompt, with today's date from the clock. `tools` is true for
/// the model that is offered tools, false for the writer.
fn system_prompt(now: LocalTime, tools: bool) -> String {
    format!(
        "You are the assistant built into ScarfeeOS. Today is {}, {:04}-{:02}-{:02}. {REPLY_RULES}{}",
        now.weekday_name(),
        now.date.year,
        now.date.month,
        now.date.day,
        if tools { format!(" {TOOL_RULES}") } else { String::new() },
    )
}

/// Words that end a place name ("... in Tokyo right now").
const PLACE_END: &[&str] = &[
    "now", "right", "today", "tonight", "currently", "please", "الان", "الآن", "حاليا", "حاليًا", "الحين", "دلوقتي",
    "اليوم", "فضلك", "سمحت",
];

/// Words that mean the device's own place.
const PLACE_LOCAL: &[&str] = &["here", "local", "my", "هنا", "محلي"];

/// The place named after "in" / "في" in a time question, if any.
fn requested_place(user_text: &str) -> Option<String> {
    let words: Vec<&str> = user_text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let opener = words.iter().position(|w| w.eq_ignore_ascii_case("in") || *w == "في")?;
    let place: Vec<&str> = words[opener + 1..]
        .iter()
        .copied()
        .take_while(|w| !PLACE_END.contains(&w.to_lowercase().as_str()))
        .take(3)
        .collect();
    let first = place.first()?.to_lowercase();
    (!PLACE_LOCAL.contains(&first.as_str())).then(|| place.join(" "))
}

/// The tools offered to the model, in the OpenAI `tools` format. `current_time`
/// is left out: code answers time questions first, and when it was offered,
/// models called it before reminders and stopped there.
pub fn tool_schema() -> Value {
    tool_schema_for(true)
}

/// The same, without `set_language` when `set_language` is false.
fn tool_schema_for(set_language: bool) -> Value {
    fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
        json!({"type": "function", "function": {
            "name": name,
            "description": description,
            "parameters": {"type": "object", "properties": properties, "required": required},
        }})
    }
    let text = |d: &str| json!({"type": "string", "description": d});
    let mut tools = json!([
        tool("open_app", "Open an application such as email, calculator or music. Not for files.", json!({"name": text("App name")}), &["name"]),
        tool("list_files", "List files in a folder.", json!({"path": text("Folder path")}), &["path"]),
        tool("search_files", "Search for a file by name (دوّر, فتّش).", json!({"query": text("Search text")}), &["query"]),
        tool(
            "create_reminder",
            "Create a reminder.",
            json!({"text": text("Reminder text"), "time": text("ISO 8601 time")}),
            &["text", "time"],
        ),
        tool(
            "schedule_meeting",
            "Schedule a meeting.",
            json!({
                "title": text("Meeting title"),
                "time": text("ISO 8601 time"),
                "attendees": {"type": "array", "items": {"type": "string"}},
            }),
            &["title", "time", "attendees"],
        ),
        tool(
            "set_language",
            "Change the system language.",
            json!({"lang": {"type": "string", "enum": ["en", "ar"]}}),
            &["lang"],
        ),
    ]);
    if !set_language {
        tools.as_array_mut().expect("the tools are a list").retain(|t| t["function"]["name"] != "set_language");
    }
    tools
}

/// Month names in normalized form (see `guard::normalize`), with the number of
/// words they take. كانون and تشرين need the word after them.
fn month_at(words: &[&str], i: usize) -> Option<(u32, usize)> {
    let month = match words[i] {
        "january" | "jan" | "يناير" | "جانفي" => 1,
        "february" | "feb" | "فبراير" | "فيفري" => 2,
        "march" | "mar" | "مارس" => 3,
        "april" | "apr" | "ابريل" | "افريل" => 4,
        "may" | "مايو" | "ماي" => 5,
        "june" | "jun" | "يونيو" | "يونيه" | "جوان" => 6,
        "july" | "jul" | "يوليو" | "يوليه" | "جويليه" => 7,
        "august" | "aug" | "اغسطس" | "اوت" => 8,
        "september" | "sep" | "sept" | "سبتمبر" => 9,
        "october" | "oct" | "اكتوبر" => 10,
        "november" | "nov" | "نوفمبر" => 11,
        "december" | "dec" | "ديسمبر" => 12,
        "شباط" => 2,
        "اذار" => 3,
        "نيسان" => 4,
        "ايار" => 5,
        "حزيران" => 6,
        "تموز" => 7,
        "اب" => 8,
        "ايلول" => 9,
        "كانون" | "تشرين" => {
            let first = words[i] == "كانون";
            return match words.get(i + 1).copied() {
                Some("الاول") => Some((if first { 12 } else { 10 }, 2)),
                Some("الثاني") => Some((if first { 1 } else { 11 }, 2)),
                _ => None,
            };
        }
        _ => return None,
    };
    Some((month, 1))
}

fn is_valid_date(year: i32, month: u32, day: u32) -> bool {
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let length = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    year >= 1000 && (1..=length).contains(&day)
}

/// A year of four digits, which also keeps every date after the Hijri epoch.
fn year_of(word: &str) -> Option<i32> {
    (word.len() == 4 && word.bytes().all(|b| b.is_ascii_digit())).then(|| word.parse().ok()).flatten()
}

fn day_of(word: &str) -> Option<u32> {
    let digits = ["st", "nd", "rd", "th"].iter().find_map(|s| word.strip_suffix(s)).unwrap_or(word);
    (digits.len() <= 2 && !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| digits.parse().ok())
        .flatten()
}

/// Words that may sit between the parts of a date ("1st of January, year 2026").
const DATE_FILLER: &[&str] = &["of", "من", "عام", "سنه", "year"];

/// The Gregorian date written with digits in the text, such as `2026-01-01`,
/// `1/1/2026`, `1 January 2026`, `January 1, 2026` or `1 يناير 2026`. A date
/// like 01/02/2026, where either order is possible, and a date that does not
/// exist are not read.
fn gregorian_date_in(user_text: &str) -> Option<Date> {
    let text = guard::normalize(user_text);
    let numeric = text.split(|c: char| !c.is_alphanumeric() && !matches!(c, '-' | '/' | '.'));
    for token in numeric {
        let parts: Vec<&str> = token.trim_matches(['-', '/', '.']).split(['-', '/', '.']).collect();
        let [a, b, c] = parts.as_slice() else { continue };
        if ![a, b, c].iter().all(|p| !p.is_empty() && p.bytes().all(|x| x.is_ascii_digit())) {
            continue;
        }
        let (a, b, c) = (a.parse::<u32>().ok()?, b.parse::<u32>().ok()?, c.parse::<u32>().ok()?);
        let (year, month, day) = if parts[0].len() == 4 {
            (a, b, c)
        } else if parts[2].len() == 4 && (a > 12 || b > 12 || a == b) {
            (c, if a > 12 { b } else { a }, if a > 12 { a } else { b })
        } else {
            continue;
        };
        if is_valid_date(year as i32, month, day) {
            return Some(Date::new(year as i32, month, day));
        }
    }
    let words: Vec<&str> = text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    for i in 0..words.len() {
        let Some((month, len)) = month_at(&words, i) else { continue };
        let skip_fillers = |mut j: usize, back: bool| -> Option<usize> {
            while words.get(j).is_some_and(|w| DATE_FILLER.contains(w)) {
                j = if back { j.checked_sub(1)? } else { j + 1 };
            }
            Some(j)
        };
        // "1 January 2026"
        let day_before = i.checked_sub(1).and_then(|j| skip_fillers(j, true)).and_then(|j| words.get(j)).and_then(|w| day_of(w));
        let year_after = skip_fillers(i + len, false).and_then(|j| words.get(j)).and_then(|w| year_of(w));
        if let (Some(day), Some(year)) = (day_before, year_after) {
            if is_valid_date(year, month, day) {
                return Some(Date::new(year, month, day));
            }
        }
        // "January 1, 2026"
        let day_after = words.get(i + len).and_then(|w| day_of(w));
        let year_next = skip_fillers(i + len + 1, false).and_then(|j| words.get(j)).and_then(|w| year_of(w));
        if let (Some(day), Some(year)) = (day_after, year_next) {
            if is_valid_date(year, month, day) {
                return Some(Date::new(year, month, day));
            }
        }
    }
    None
}

/// Checks a raw call against the known tools. Times in `create_reminder` and
/// `schedule_meeting` are not read: they come from the user's text later.
fn parse_call(raw: &RawToolCall) -> Result<ToolCall, String> {
    let name = raw.name.as_str();
    let args: Value = serde_json::from_str(&raw.arguments)
        .map_err(|e| format!("{name}: arguments are not valid JSON: {e}"))?;
    let Value::Object(args) = args else {
        return Err(format!("{name}: arguments are not a JSON object"));
    };
    let string = |key: &str| -> Result<String, String> {
        match args.get(key) {
            Some(Value::String(s)) => Ok(s.clone()),
            _ => Err(format!("{name}: argument `{key}` must be a string")),
        }
    };
    match name {
        "open_app" => Ok(ToolCall::OpenApp { name: string("name")? }),
        "list_files" => Ok(ToolCall::ListFiles { path: string("path")? }),
        "search_files" => Ok(ToolCall::SearchFiles { query: string("query")? }),
        "create_reminder" => Ok(ToolCall::CreateReminder { text: string("text")?, time: String::new(), repeat: None }),
        "schedule_meeting" => {
            let attendees = match args.get("attendees") {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::Array(items)) => items
                    .iter()
                    .map(|v| v.as_str().map(str::to_string))
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| format!("{name}: `attendees` must be a list of strings"))?,
                Some(_) => return Err(format!("{name}: `attendees` must be a list of strings")),
            };
            Ok(ToolCall::ScheduleMeeting { title: string("title")?, time: String::new(), attendees, repeat: None })
        }
        "set_language" => {
            let code = string("lang")?;
            Language::from_code(&code)
                .map(|lang| ToolCall::SetLanguage { lang })
                .ok_or_else(|| format!("{name}: unsupported language `{code}`"))
        }
        "current_time" => match args.get("timezone") {
            None | Some(Value::Null) => Ok(ToolCall::CurrentTime { timezone: None }),
            Some(Value::String(zone)) => Ok(ToolCall::CurrentTime { timezone: Some(zone.clone()) }),
            Some(_) => Err(format!("{name}: argument `timezone` must be a string")),
        },
        _ => Err(format!("unknown tool: {name}")),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;
    use crate::datetime::Date;
    use crate::tools::FixedClock;

    const TODAY: Date = Date { year: 2026, month: 10, day: 30 };
    const NOW: LocalTime = LocalTime { date: TODAY, hour: 9, minute: 5, second: 30 };

    /// Answers with `replies` in order, then repeats the last one.
    struct MockModel {
        replies: Vec<Result<ModelReply, String>>,
        calls: Cell<u32>,
        last_system: RefCell<String>,
        last_tools: RefCell<Value>,
    }

    impl Model for MockModel {
        fn chat(&self, system: &str, _user: &str, tools: &Value) -> Result<ModelReply, String> {
            let n = self.calls.get() as usize;
            self.calls.set(self.calls.get() + 1);
            *self.last_system.borrow_mut() = system.to_string();
            *self.last_tools.borrow_mut() = tools.clone();
            self.replies[n.min(self.replies.len() - 1)].clone()
        }
    }

    fn model(reply: Result<ModelReply, String>) -> MockModel {
        model_seq(vec![reply])
    }

    fn model_seq(replies: Vec<Result<ModelReply, String>>) -> MockModel {
        MockModel { replies, calls: Cell::new(0), last_system: RefCell::new(String::new()), last_tools: RefCell::new(Value::Null) }
    }

    fn offered_tools(m: &MockModel) -> Vec<String> {
        m.last_tools.borrow().as_array().unwrap().iter().map(|t| t["function"]["name"].as_str().unwrap().to_string()).collect()
    }

    fn text(s: &str) -> Result<ModelReply, String> {
        Ok(ModelReply::Text(s.into()))
    }

    fn call(name: &str, arguments: &str) -> Result<ModelReply, String> {
        Ok(ModelReply::ToolCalls(vec![RawToolCall { name: name.into(), arguments: arguments.into() }]))
    }

    /// Runs one turn and returns the outcome with the number of model calls.
    fn turn(m: &MockModel, text: &str, language: Language) -> (Outcome, u32) {
        let clock = FixedClock(NOW);
        let outcome = Assistant::new(m, &clock).handle(text, language);
        (outcome, m.calls.get())
    }

    fn error_of(outcome: Outcome) -> String {
        match outcome {
            Outcome::Error(e) => e,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn named_place_gets_no_time_and_no_model_call() {
        let m = model(Ok(ModelReply::Text("It is 3 pm in Tokyo.".into())));
        for (text, place) in [
            ("What time is it in Tokyo?", "Tokyo"),
            ("what's the time in New York right now", "New York"),
            ("كم الساعة الآن في طوكيو؟", "طوكيو"),
        ] {
            let (outcome, calls) = turn(&m, text, Language::English);
            assert_eq!(outcome, Outcome::UnsupportedTimeZone(place.into()), "{text}");
            assert_eq!(calls, 0, "{text}");
        }
    }

    #[test]
    fn local_time_comes_from_the_clock_without_the_model() {
        let m = model(Err("must not be called".into()));
        for text in ["What time is it?", "الساعة كام؟", "What time is it here?"] {
            assert_eq!(turn(&m, text, Language::English), (Outcome::Time(NOW), 0), "{text}");
        }
    }

    #[test]
    fn news_and_weather_say_offline_without_the_model() {
        let m = model(Ok(ModelReply::Text("Big news today!".into())));
        for (text, live) in [
            ("What's the news?", LiveData::News),
            ("وش الأخبار اليوم؟", LiveData::News),
            ("What's the weather in Cairo today?", LiveData::Weather),
            ("الطقس النهارده", LiveData::Weather),
        ] {
            assert_eq!(turn(&m, text, Language::English), (Outcome::CannotDoOffline(live), 0), "{text}");
        }
    }

    #[test]
    fn wrong_model_date_is_overridden() {
        let m = model(call(
            "create_reminder",
            r#"{"text":"call the bank","time":"2020-01-01T00:00:00"}"#,
        ));
        let (outcome, calls) = turn(&m, "Remind me to call the bank tomorrow at 10 am.", Language::English);
        assert_eq!(calls, 1);
        assert_eq!(
            outcome,
            Outcome::Run(ToolCall::CreateReminder {
                text: "call the bank".into(),
                time: "2026-10-31T10:00:00".into(),
                repeat: None,
            })
        );
        // The model is told the real date.
        assert!(m.last_system.borrow().contains("Friday, 2026-10-30"), "{}", m.last_system.borrow());
    }

    #[test]
    fn wrong_meeting_date_is_overridden_and_keeps_attendees() {
        let m = model(call(
            "schedule_meeting",
            r#"{"title":"Project Review","time":"1999-01-01T00:00:00","attendees":["Sara","Omar"]}"#,
        ));
        let (outcome, _) = turn(&m, "Schedule Project Review with Sara and Omar on Sunday at 2 pm.", Language::English);
        assert_eq!(
            outcome,
            Outcome::Run(ToolCall::ScheduleMeeting {
                title: "Project Review".into(),
                time: "2026-11-01T14:00:00".into(),
                attendees: vec!["Sara".into(), "Omar".into()],
                repeat: None,
            })
        );
    }

    #[test]
    fn bare_arabic_hour_needs_clarification() {
        let m = model(call("create_reminder", r#"{"text":"x","time":"2020-01-01T11:00:00"}"#));
        let (outcome, _) = turn(&m, "ذكرني الساعة 11 x", Language::Arabic);
        assert_eq!(
            outcome,
            Outcome::NeedsClarification(ToolCall::CreateReminder {
                text: "x".into(),
                time: "2026-10-30T11:00:00".into(),
                repeat: None,
            })
        );
    }

    #[test]
    fn a_message_that_is_only_a_time_is_not_answered_as_the_current_time() {
        // The model made up "الساعة الآن 11 صباحًا" for the bare request الساعة 11.
        let m = model(Ok(ModelReply::Text("الساعة الآن 11 صباحًا".into())));
        for (user, language, time) in [
            ("الساعة 11", Language::Arabic, "11"),
            ("الساعة ١١", Language::Arabic, "11"),
            ("الساعة 11:30 مساءً", Language::Arabic, "11:30"),
            ("الساعة 11 صباحًا", Language::Arabic, "11"),
            ("عند الساعة 3", Language::Arabic, "3"),
            ("at 11", Language::English, "11"),
            ("At 3pm, please", Language::English, "3pm"),
            ("11:15", Language::English, "11:15"),
            ("3 p.m.", Language::English, "3"),
        ] {
            assert_eq!(turn(&m, user, language), (Outcome::TimeOnly(time.into()), 0), "{user}");
        }
    }

    #[test]
    fn time_questions_and_requests_with_a_time_are_not_bare_times() {
        let m = model(Ok(ModelReply::Text("ok".into())));
        for (user, language) in [
            ("What time is it", Language::English),
            ("كم الساعة", Language::Arabic),
            ("11", Language::English),
            ("الساعة 25", Language::Arabic),
            ("Remind me at 11", Language::English),
            ("ذكرني الساعة 11", Language::Arabic),
            ("at 11 and 12", Language::English),
        ] {
            assert!(!matches!(turn(&m, user, language).0, Outcome::TimeOnly(_)), "{user}");
        }
        assert!(matches!(turn(&m, "What time is it", Language::English).0, Outcome::Time(_)));
        assert!(matches!(turn(&m, "كم الساعة؟", Language::Arabic).0, Outcome::Time(_)));
    }

    #[test]
    fn missing_time_is_returned_without_the_model_time() {
        let m = model(call("create_reminder", r#"{"text":"call the bank","time":"2020-01-01T00:00:00"}"#));
        let (outcome, _) = turn(&m, "Remind me to call the bank", Language::English);
        assert_eq!(
            outcome,
            Outcome::MissingTime(ToolCall::CreateReminder { text: "call the bank".into(), time: String::new(), repeat: None })
        );
    }

    #[test]
    fn set_language_needs_confirmation() {
        let m = model(call("set_language", r#"{"lang":"ar"}"#));
        let (outcome, calls) = turn(&m, "Switch the system language to Arabic", Language::English);
        assert_eq!(outcome, Outcome::NeedsConfirmation(ToolCall::SetLanguage { lang: Language::Arabic }));
        assert_eq!(calls, 1);
        assert!(offered_tools(&m).contains(&"set_language".to_string()));
    }

    #[test]
    fn translation_requests_do_not_offer_set_language() {
        for user in [
            "Translate to Arabic: good morning",
            "ترجم إلى الإنجليزية: صباح الخير",
            "ترجملي هذه الجملة للعربية",
        ] {
            let m = model(text("صباح الخير"));
            let (outcome, calls) = turn(&m, user, Language::Arabic);
            assert_eq!(outcome, Outcome::Reply { text: "صباح الخير".into(), issues: vec![] }, "{user}");
            assert_eq!(calls, 1, "{user}");
            let tools = offered_tools(&m);
            assert!(!tools.contains(&"set_language".to_string()), "{user}: {tools:?}");
            assert_eq!(tools.len(), tool_schema().as_array().unwrap().len() - 1, "{user}");
        }
    }

    #[test]
    fn set_language_call_during_a_translation_is_an_error() {
        for user in ["Translate to Arabic: good morning", "ترجملي هذه الجملة"] {
            let m = model(call("set_language", r#"{"lang":"ar"}"#));
            let (outcome, calls) = turn(&m, user, Language::English);
            assert!(error_of(outcome).starts_with("set_language"), "{user}");
            assert_eq!(calls, 1, "{user}");
        }
    }

    #[test]
    fn other_tools_still_run_during_a_translation() {
        let m = model(call("open_app", r#"{"name":"calculator"}"#));
        let (outcome, _) = turn(&m, "Translate this and open the calculator", Language::English);
        assert_eq!(outcome, Outcome::Run(ToolCall::OpenApp { name: "calculator".into() }));
    }

    #[test]
    fn plain_tools_run() {
        let m = model(call("open_app", r#"{"name":"calculator"}"#));
        let (outcome, _) = turn(&m, "Open the calculator.", Language::English);
        assert_eq!(outcome, Outcome::Run(ToolCall::OpenApp { name: "calculator".into() }));
    }

    /// The reminder the model's `model_text` becomes for `user_text`, with its time.
    fn reminder_for(user_text: &str, model_text: &str, language: Language) -> (String, String) {
        let args = json!({"text": model_text, "time": "2020-01-01T00:00:00"}).to_string();
        match turn(&model(call("create_reminder", &args)), user_text, language).0 {
            Outcome::Run(ToolCall::CreateReminder { text, time, .. }) => (text, time),
            other => panic!("expected a reminder, got {other:?}"),
        }
    }

    #[test]
    fn english_reminder_for_an_arabic_request_uses_the_users_words() {
        let (text, time) = reminder_for(
            "ذكّرني أعمل نسخة احتياطية للابتوب كل جمعة.",
            "perform a backup for the laptop",
            Language::Arabic,
        );
        assert_eq!(text, "أعمل نسخة احتياطية للابتوب");
        assert_eq!(time, "2026-11-06");
        // A model text in another language is ignored too.
        let (text, _) = reminder_for("Remind me to call the bank tomorrow at 10 am.", "اتصل بالبنك", Language::English);
        assert_eq!(text, "call the bank");
    }

    #[test]
    fn time_words_the_model_copied_are_removed() {
        let (text, time) = reminder_for("عايز أفتكر أكلم المهندس بكرة الصبح.", "أكلم المهندس بكرة الصبح", Language::Arabic);
        assert_eq!(text, "أكلم المهندس");
        assert_eq!(time, "2026-10-31T09:00:00");
        let (text, _) = reminder_for("Remind me to call Sam tomorrow at 10 am.", "call Sam tomorrow at 10 am", Language::English);
        assert_eq!(text, "call Sam");
    }

    #[test]
    fn mixed_sentences_keep_the_users_words_not_the_models() {
        for model_text in ["backup the laptop every Friday", "perform a backup for the laptop"] {
            let (text, _) = reminder_for("ذكّرني أعمل backup للـ laptop كل جمعة.", model_text, Language::Arabic);
            assert_eq!(text, "أعمل backup للـ laptop", "{model_text}");
        }
    }

    #[test]
    fn a_wrong_model_text_never_changes_the_reminder() {
        // The model turned "to call the engineer" into "to eat with the engineer".
        let user = "عايز أفتكر أكلم المهندس بكرة الصبح";
        for model_text in ["أكّل مع المهندس", "أكل مع المهندس بكرة الصبح", "buy milk", ""] {
            let (text, time) = reminder_for(user, model_text, Language::Arabic);
            assert_eq!(text, "أكلم المهندس", "{model_text:?}");
            assert_eq!(time, "2026-10-31T09:00:00", "{model_text:?}");
        }
    }

    #[test]
    fn reminders_in_every_dialect_use_only_the_users_words() {
        for (user, language, expected) in [
            ("ذكرني أكلم المهندس بكرة الصبح", Language::Arabic, "أكلم المهندس"),
            ("بكرة الصبح فكرني أكلم المهندس", Language::Arabic, "أكلم المهندس"),
            ("ممكن تفكرني أكلم المهندس بكرة الصبح", Language::Arabic, "أكلم المهندس"),
            ("عاوز افتكر أكلم المهندس انهارده الساعة 5 العصر", Language::Arabic, "أكلم المهندس"),
            ("ابي تذكرني اكلم المهندس باجر الصبح", Language::Arabic, "اكلم المهندس"),
            ("ابغى افتكر اكلم المهندس باكر الصبح", Language::Arabic, "اكلم المهندس"),
            ("بدي افتكر اتصل بالمهندس بكرا الصبح", Language::Arabic, "اتصل بالمهندس"),
            ("نبهني اتصل بالمهندس بكرا الساعة 9 الصبح", Language::Arabic, "اتصل بالمهندس"),
            ("Please remind me to call the engineer tomorrow morning", Language::English, "call the engineer"),
            ("Can you set a reminder to call the engineer tomorrow at 9 am", Language::English, "call the engineer"),
            ("I need to remember to call the engineer the day after tomorrow", Language::English, "call the engineer"),
        ] {
            let (text, _) = reminder_for(user, "unrelated words the user never said", language);
            assert_eq!(text, expected, "{user}");
        }
    }

    #[test]
    fn a_reminder_with_no_task_words_gets_a_neutral_title_not_the_models_text() {
        for model_text in ["اتصل بالبنك بكرة", "buy milk", ""] {
            let (text, time) = reminder_for("ذكرني بكرة الساعة 5 العصر", model_text, Language::Arabic);
            assert_eq!(text, "تذكير", "{model_text:?}");
            assert_eq!(time, "2026-10-31T17:00:00", "{model_text:?}");
            // No part of the day: the hour is unclear, so the user is asked, with the same title.
            let args = json!({"text": model_text, "time": "2020-01-01T00:00:00"}).to_string();
            let outcome = turn(&model(call("create_reminder", &args)), "ذكرني بكرة الساعة 5", Language::Arabic).0;
            let Outcome::NeedsClarification(ToolCall::CreateReminder { text, .. }) = outcome else {
                panic!("expected a question, got {outcome:?}");
            };
            assert_eq!(text, "تذكير", "{model_text:?}");
        }
        let (text, _) = reminder_for("Remind me tomorrow at 5 pm", "call the bank", Language::English);
        assert_eq!(text, "Reminder");
    }

    #[test]
    fn a_reminder_on_an_egyptian_weekday_keeps_its_date_and_task() {
        let (text, time) = reminder_for("ذكرني اتصل بالبنك يوم التلات الساعة 4 العصر", "x", Language::Arabic);
        assert_eq!(text, "اتصل بالبنك");
        assert_eq!(time, "2026-11-03T16:00:00");
    }

    #[test]
    fn english_sentence_with_an_arabic_name_keeps_the_name() {
        let user = "Remind me to call المهندس أحمد tomorrow at 10 am.";
        let (text, time) = reminder_for(user, "call المهندس أحمد tomorrow at 10 am", Language::English);
        assert_eq!(text, "call المهندس أحمد");
        assert_eq!(time, "2026-10-31T10:00:00");
        let (text, _) = reminder_for(user, "call Ahmad", Language::English);
        assert_eq!(text, "call المهندس أحمد");
    }

    #[test]
    fn numbers_and_places_in_the_text_stay() {
        let (text, _) = reminder_for("Remind me to check room 10 tomorrow.", "check room 10 tomorrow", Language::English);
        assert_eq!(text, "check room 10");
        let (text, _) = reminder_for("Remind me to look at the board at 5 pm.", "look at the board at 5 pm", Language::English);
        assert_eq!(text, "look at the board");
    }

    /// The meeting the model's `title` and `attendees` become for `user_text`.
    fn meeting_for(user_text: &str, title: &str, attendees: &[&str]) -> (String, Vec<String>, String) {
        let args = json!({"title": title, "time": "x", "attendees": attendees}).to_string();
        match turn(&model(call("schedule_meeting", &args)), user_text, Language::English).0 {
            Outcome::Run(ToolCall::ScheduleMeeting { title, attendees, time, .. })
            | Outcome::NeedsClarification(ToolCall::ScheduleMeeting { title, attendees, time, .. }) => (title, attendees, time),
            other => panic!("expected a meeting, got {other:?}"),
        }
    }

    #[test]
    fn a_title_the_user_names_is_used_exactly() {
        for (user, expected) in [
            ("Schedule a meeting called Project Review with Sara on Sunday at 2 pm.", "Project Review"),
            ("Set up a meeting named Q4 Budget tomorrow at 10 am", "Q4 Budget"),
            ("Schedule a meeting called \"Review with the board\" on Sunday at 2 pm", "Review with the board"),
            ("حدّد اجتماعًا باسم مراجعة المشروع مع سارة وعمر يوم الأحد الساعة الثانية ظهرًا.", "مراجعة المشروع"),
            ("اعمل اجتماع بعنوان «خطة التسويق» بكرة الساعة 3 العصر", "خطة التسويق"),
        ] {
            let (title, _, _) = meeting_for(user, "Wrong model title", &["Sara", "سارة", "عمر"]);
            assert_eq!(title, expected, "{user}");
        }
        let (title, attendees, time) = meeting_for(
            "حدّد اجتماعًا باسم مراجعة المشروع مع سارة وعمر يوم الأحد الساعة الثانية ظهرًا.",
            "meeting مع الفريق",
            &["سارة", "عمر", "ليلى"],
        );
        assert_eq!((title.as_str(), time.as_str()), ("مراجعة المشروع", "2026-11-01T14:00:00"));
        assert_eq!(attendees, ["سارة", "عمر"]);
    }

    #[test]
    fn a_person_named_in_the_request_is_not_taken_as_the_title() {
        let (title, attendees, _) = meeting_for("Schedule a meeting with a colleague named Bob tomorrow at 10 am", "x", &["Bob"]);
        assert_eq!(title, "Meeting with a colleague named Bob");
        assert_eq!(attendees, ["Bob"]);
    }

    #[test]
    fn without_a_named_title_the_default_is_short_and_built_from_the_users_words() {
        for (user, expected) in [
            ("Schedule a meeting with Sara tomorrow at 3 pm", "Meeting with Sara"),
            ("Schedule a meeting tomorrow at 3 pm", "Meeting"),
            ("Schedule a team meeting with Sara tomorrow at 3 pm", "team meeting with Sara"),
            ("حدد اجتماع مع الفريق بكرة الساعة 3 العصر", "اجتماع مع الفريق"),
            ("اعمل اجتماع بكرة الساعة 3 العصر", "اجتماع"),
            ("ابي اسوي اجتماع مع الفريق باجر الصبح", "اجتماع مع الفريق"),
            ("بدي اعمل اجتماع مع الفريق بكرا الصبح", "اجتماع مع الفريق"),
            ("ممكن تعمل schedule لـ meeting مع الـ team يوم الخميس الساعة 11؟", "اجتماع مع الـ team"),
            (
                "Schedule a meeting to discuss the quarterly budget forecast with the whole finance team tomorrow",
                "discuss the quarterly budget forecast",
            ),
        ] {
            // The mixed title the model made up is never used.
            let (title, _, _) = meeting_for(user, "meeting مع الفريق", &[]);
            assert_eq!(title, expected, "{user}");
        }
    }

    #[test]
    fn a_command_phrase_is_never_the_title() {
        // dialect-gulf-011 took "ابي اجتماع" as the title.
        let (title, attendees, _) = meeting_for(
            "ابي اجتماع مع إسلام يوم الاثنين الساعة 11 الصبح عن المشروع الجديد.",
            "اجتماع",
            &["إسلام"],
        );
        assert_eq!((title.as_str(), attendees.as_slice()), ("المشروع الجديد", ["إسلام".to_string()].as_slice()));
        for (user, expected) in [
            ("ابي اجتماع مع إسلام بكرة الساعة 3 العصر", "اجتماع مع إسلام"),
            ("بدي اجتماع بخصوص الميزانية مع سارة بكرة الساعة 3 العصر", "الميزانية"),
            ("عايز اجتماع مع الفريق عن خطة التسويق مع سارة بكرة الساعة 3 العصر", "خطة التسويق"),
            ("I want a meeting with Sara about the budget tomorrow at 3 pm", "the budget"),
            ("I need a meeting tomorrow at 3 pm", "a meeting"),
        ] {
            let (title, _, _) = meeting_for(user, "x", &[]);
            assert_eq!(title, expected, "{user}");
        }
    }

    #[test]
    fn the_default_title_stops_before_a_named_attendee() {
        let (title, attendees, _) = meeting_for("Schedule a team meeting with Sara tomorrow at 3 pm", "x", &["Sara"]);
        assert_eq!((title.as_str(), attendees.as_slice()), ("team meeting", ["Sara".to_string()].as_slice()));
    }

    #[test]
    fn attendees_must_be_in_the_users_text() {
        let (_, attendees, _) = meeting_for(
            "Schedule a meeting called Review with Sara and Omar on Sunday at 2 pm.",
            "Review",
            &["sara", "Omar Khaled", "Layla", "Omar"],
        );
        assert_eq!(attendees, ["Sara", "Omar"]);
        let (_, attendees, _) = meeting_for("اعمل اجتماع مع المهندس أحمد ولسارة بكرة الساعة 3 العصر", "x", &["أحمد", "سارة", "علي"]);
        assert_eq!(attendees, ["أحمد", "سارة"]);
        let (_, attendees, _) = meeting_for("Schedule a meeting tomorrow at 3 pm", "x", &["Sara"]);
        assert!(attendees.is_empty());
    }

    #[test]
    fn model_current_time_call_is_answered_from_the_clock() {
        let m = model(call("current_time", "{}"));
        assert_eq!(turn(&m, "Tell me the time, thanks a lot", Language::English).0, Outcome::Time(NOW));
        let m = model(call("current_time", r#"{"timezone":"Asia/Tokyo"}"#));
        assert_eq!(
            turn(&m, "Tell me the time over there, thanks a lot", Language::English).0,
            Outcome::UnsupportedTimeZone("Asia/Tokyo".into())
        );
    }

    #[test]
    fn unknown_tool_is_an_error() {
        let m = model(call("delete_everything", "{}"));
        let e = error_of(turn(&m, "Clean up my disk", Language::English).0);
        assert!(e.contains("unknown tool: delete_everything"), "{e}");
    }

    #[test]
    fn malformed_arguments_are_errors() {
        for (name, args) in [
            ("open_app", "{not json"),
            ("open_app", "[]"),
            ("open_app", "{}"),
            ("open_app", r#"{"name":5}"#),
            ("set_language", r#"{"lang":"fr"}"#),
            ("schedule_meeting", r#"{"title":"x","attendees":"Sara"}"#),
            ("schedule_meeting", r#"{"title":"x","attendees":[1]}"#),
            ("current_time", r#"{"timezone":3}"#),
        ] {
            let m = model(call(name, args));
            let e = error_of(turn(&m, "Do the thing", Language::English).0);
            assert!(e.starts_with(name), "{name} {args}: {e}");
        }
    }

    #[test]
    fn several_different_calls_are_an_error_but_repeats_are_not() {
        let open = RawToolCall { name: "open_app".into(), arguments: r#"{"name":"calculator"}"#.into() };
        let files = RawToolCall { name: "list_files".into(), arguments: r#"{"path":"~"}"#.into() };
        let m = model(Ok(ModelReply::ToolCalls(vec![open.clone(), files])));
        assert!(error_of(turn(&m, "Do both", Language::English).0).contains("2 tool calls"));
        let m = model(Ok(ModelReply::ToolCalls(vec![open.clone(), open])));
        assert_eq!(
            turn(&m, "Open the calculator", Language::English).0,
            Outcome::Run(ToolCall::OpenApp { name: "calculator".into() })
        );
    }

    #[test]
    fn russian_in_an_arabic_reply_is_removed_from_the_reply() {
        let m = model(Ok(ModelReply::Text("تم فتح الملف привет بنجاح".into())));
        let (outcome, _) = turn(&m, "اشرح لي ما حدث", Language::Arabic);
        assert_eq!(outcome, reply("تم فتح الملف بنجاح"));
    }

    #[test]
    fn clean_reply_has_no_issues() {
        let m = model(Ok(ModelReply::Text("أهلاً، كيف أساعدك؟".into())));
        assert_eq!(
            turn(&m, "مرحبا", Language::Arabic).0,
            Outcome::Reply { text: "أهلاً، كيف أساعدك؟".into(), issues: vec![] }
        );
    }

    #[test]
    fn model_failure_is_an_error() {
        let m = model(Err("local provider: request failed".into()));
        assert_eq!(error_of(turn(&m, "Hello", Language::English).0), "local provider: request failed");
    }

    #[test]
    fn schema_lists_every_tool() {
        let names: Vec<String> = tool_schema()
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            names,
            ["open_app", "list_files", "search_files", "create_reminder", "schedule_meeting", "set_language"]
        );
    }

    #[test]
    fn places_are_read_from_time_questions() {
        assert_eq!(requested_place("What time is it in Tokyo?").as_deref(), Some("Tokyo"));
        assert_eq!(requested_place("what time is it in Los Angeles right now").as_deref(), Some("Los Angeles"));
        assert_eq!(requested_place("What time is it?"), None);
        assert_eq!(requested_place("what time is it in my time zone"), None);
    }

    #[test]
    fn religious_questions_are_declined_and_the_answer_is_thrown_away() {
        let m = model(text("The five pillars are ..."));
        for (user, language) in [
            ("What are the five pillars of Islam?", Language::English),
            ("ما هي أركان الإسلام الخمسة؟", Language::Arabic),
            ("Who wrote the Bible?", Language::English),
            ("ما حكم الصلاة في السفر", Language::Arabic),
            ("What's the news about Islam?", Language::English),
        ] {
            assert_eq!(turn(&m, user, language).0, Outcome::Declined, "{user}");
        }
        let failed = model(Err("server down".into()));
        assert_eq!(turn(&failed, "Who wrote the Bible?", Language::English).0, Outcome::Declined);
    }

    #[test]
    fn names_that_are_religious_words_do_not_block_commands() {
        let m = model(call("create_reminder", r#"{"text":"call Islam","time":"x"}"#));
        for user in ["Remind me to call Islam tomorrow at 10 am.", "ذكرني أكلم إسلام بكرة الساعة 10 الصبح"] {
            match turn(&m, user, Language::English).0 {
                Outcome::Run(ToolCall::CreateReminder { time, .. }) => assert_eq!(time, "2026-10-31T10:00:00"),
                other => panic!("{user}: {other:?}"),
            }
        }
    }

    #[test]
    fn ordinary_questions_near_religious_words_reach_the_model() {
        let m = model(text("Islamabad is the capital of Pakistan."));
        let (outcome, calls) = turn(&m, "What is the capital of Pakistan, Islamabad?", Language::English);
        assert!(matches!(outcome, Outcome::Reply { .. }), "{outcome:?}");
        assert_eq!(calls, 1);
        let (outcome, calls) = turn(&m, "When does Ramadan start in 2026?", Language::English);
        assert!(matches!(outcome, Outcome::Reply { .. }), "{outcome:?}");
        assert_eq!(calls, 2);
    }

    #[test]
    fn second_sentence_does_not_hide_live_data() {
        let m = model(text("Invented news"));
        assert_eq!(
            turn(&m, "وش الأخبار اليوم؟ ابي تلخص لي أهم شي.", Language::Arabic),
            (Outcome::CannotDoOffline(LiveData::News), 0)
        );
        // Explanations are still the model's job.
        for user in ["Explain what a VPN does", "اشرح لي الطقس والمناخ"] {
            let (outcome, calls) = turn(&m, user, Language::English);
            assert!(matches!(outcome, Outcome::Reply { .. }), "{user}: {outcome:?}");
            assert_eq!(calls, m.calls.get(), "{user}");
        }
        assert_eq!(m.calls.get(), 2);
    }

    #[test]
    fn hijri_date_is_computed_without_the_model() {
        let m = model(text("1 Rajab 1447"));
        let expected = |g: Date| Outcome::HijriApprox { gregorian: g, hijri: hijri::to_hijri_approx(g) };
        let jan1 = Date::new(2026, 1, 1);
        let cases = [
            ("ما التاريخ الهجري الذي يقابل 1 يناير 2026 ميلادي تقريبًا؟", jan1),
            ("What is the Hijri date for 1 January 2026?", jan1),
            ("Convert January 1, 2026 to Hijri", jan1),
            ("what is 2026-01-01 in the Islamic calendar", jan1),
            ("كم يوافق ١/١/٢٠٢٦ بالهجري", jan1),
            ("Hijri date for 15/03/2026", Date::new(2026, 3, 15)),
            ("حول 3 كانون الثاني 2026 إلى التاريخ الهجري", Date::new(2026, 1, 3)),
        ];
        for (user, date) in cases {
            assert_eq!(turn(&m, user, Language::English), (expected(date), 0), "{user}");
        }
        // 1 January 2026 is about 12 Rajab 1447 by the tabular calendar.
        assert_eq!(hijri::to_hijri_approx(jan1), HijriDate::new(1447, 7, 12));
    }

    #[test]
    fn unreadable_hijri_dates_ask_the_user_without_the_model() {
        let m = model(text("1 Rajab 1447"));
        for user in [
            "ما التاريخ الهجري الذي يقابل ميلادي تقريبًا؟",
            "What is the Hijri date for 31 February 2026?",
            "What is the Hijri date for 01/02/2026?",
            "Convert 5 Ramadan 1447 to Hijri",
            "What is the Hijri date?",
        ] {
            assert_eq!(turn(&m, user, Language::English), (Outcome::MissingDate, 0), "{user}");
        }
    }

    #[test]
    fn gregorian_dates_are_read_from_digits() {
        let d = |y, m, day| Some(Date::new(y, m, day));
        assert_eq!(gregorian_date_in("1st of January, 2026"), d(2026, 1, 1));
        assert_eq!(gregorian_date_in("on Feb 29 2028"), d(2028, 2, 29));
        assert_eq!(gregorian_date_in("2026-12-31"), d(2026, 12, 31));
        assert_eq!(gregorian_date_in("25/12/2026"), d(2026, 12, 25));
        assert_eq!(gregorian_date_in("12/25/2026"), d(2026, 12, 25));
        assert_eq!(gregorian_date_in("5/5/2026"), d(2026, 5, 5));
        assert_eq!(gregorian_date_in("١ يناير ٢٠٢٦"), d(2026, 1, 1));
        assert_eq!(gregorian_date_in("1 من تشرين الأول عام 2026"), d(2026, 10, 1));
        assert_eq!(gregorian_date_in("10 أغسطس 2026"), d(2026, 8, 10));
        assert_eq!(gregorian_date_in("Feb 29 2027"), None);
        assert_eq!(gregorian_date_in("04/05/2026"), None);
        assert_eq!(gregorian_date_in("1 يناير"), None);
        assert_eq!(gregorian_date_in("كانون 1 2026"), None);
        assert_eq!(gregorian_date_in("1 Ramadan 1447"), None);
    }

    #[test]
    fn foreign_script_reply_is_retried_once_and_the_better_reply_wins() {
        let bad = "تم فتح الملف привет بنجاح";
        let good = "تم فتح الملف بنجاح";
        // The retry is clean: it is returned.
        let m = model_seq(vec![text(bad), text(good)]);
        assert_eq!(turn(&m, "اشرح لي ما حدث", Language::Arabic), (Outcome::Reply { text: good.into(), issues: vec![] }, 2));
        assert!(m.last_system.borrow().contains("Reply again in Arabic"));
        // The retry is worse: the first reply is kept, and its leaked word is removed.
        let worse = "تم فتح الملف привет مرحبا بنجاح мир";
        let m = model_seq(vec![text(bad), text(worse)]);
        let (outcome, calls) = turn(&m, "اشرح لي ما حدث", Language::Arabic);
        assert_eq!((outcome, calls), (reply(good), 2));
        // The retry is just as bad: the first reply is kept, and there is no third call.
        let m = model_seq(vec![text(bad), text(bad)]);
        assert_eq!(turn(&m, "اشرح لي ما حدث", Language::Arabic).1, 2);
    }

    #[test]
    fn retry_with_fewer_issues_is_returned() {
        let two = "تم فتح الملف привет и мир";
        let one = "تم فتح الملف привет";
        let m = model_seq(vec![text(two), text(one)]);
        let (outcome, calls) = turn(&m, "اشرح", Language::Arabic);
        assert_eq!(calls, 2);
        assert_eq!(outcome, reply("تم فتح الملف"));
    }

    #[test]
    fn failed_or_tool_retry_keeps_the_first_reply_without_its_leaked_word() {
        let bad = "تم فتح الملف привет بنجاح";
        for retry in [Err("local provider: request failed".to_string()), call("open_app", r#"{"name":"x"}"#)] {
            let m = model_seq(vec![text(bad), retry]);
            let (outcome, calls) = turn(&m, "اشرح لي ما حدث", Language::Arabic);
            assert_eq!(calls, 2);
            assert_eq!(outcome, reply("تم فتح الملف بنجاح"));
        }
    }

    #[test]
    fn clean_reply_is_not_retried() {
        let m = model(text("أهلاً"));
        assert_eq!(turn(&m, "مرحبا", Language::Arabic).1, 1);
    }

    /// Runs one turn with a writer and returns the outcome with the call counts
    /// of the command model and the writer.
    fn turn_with_writer(m: &MockModel, w: &MockModel, text: &str, language: Language) -> (Outcome, u32, u32) {
        let clock = FixedClock(NOW);
        let outcome = Assistant::new(m, &clock).with_writer(w).handle(text, language);
        (outcome, m.calls.get(), w.calls.get())
    }

    fn reply(text: &str) -> Outcome {
        Outcome::Reply { text: text.into(), issues: vec![] }
    }

    #[test]
    fn tool_call_never_reaches_the_writer() {
        let m = model(call("open_app", r#"{"name":"Files"}"#));
        let w = model(text("must not be used"));
        let (outcome, cmd, writer) = turn_with_writer(&m, &w, "Open Files", Language::English);
        assert_eq!(outcome, Outcome::Run(ToolCall::OpenApp { name: "Files".into() }));
        assert_eq!((cmd, writer), (1, 0));
    }

    #[test]
    fn text_is_written_by_the_writer_without_tools() {
        let m = model(text("command model text"));
        let w = model(text("اهلا بك"));
        let (outcome, cmd, writer) = turn_with_writer(&m, &w, "مرحبا", Language::Arabic);
        assert_eq!(outcome, reply("اهلا بك"));
        assert_eq!((cmd, writer), (1, 1));
        assert_eq!(*w.last_tools.borrow(), json!([]));
        assert!(w.last_system.borrow().contains("language and dialect"));
        assert!(w.last_system.borrow().contains("Never invent data"));
        assert_eq!(offered_tools(&m).len(), 6);
    }

    #[test]
    fn writer_failure_falls_back_to_the_command_model_text() {
        for failure in [Err("local provider: request failed".to_string()), call("open_app", r#"{"name":"x"}"#)] {
            let m = model(text("command model text"));
            let w = model(failure);
            let (outcome, cmd, writer) = turn_with_writer(&m, &w, "Explain this", Language::English);
            assert_eq!(outcome, reply("command model text"));
            assert_eq!((cmd, writer), (1, 1));
        }
    }

    #[test]
    fn religious_news_and_hijri_requests_call_no_writer() {
        let run = |text: &str| {
            let m = model(text_reply());
            let w = model(text_reply());
            turn_with_writer(&m, &w, text, Language::English)
        };
        fn text_reply() -> Result<ModelReply, String> {
            Ok(ModelReply::Text("some text".into()))
        }
        let (outcome, _, writer) = run("What is the ruling on fasting in Islam?");
        assert_eq!((outcome, writer), (Outcome::Declined, 0));
        assert_eq!(run("What's the news?"), (Outcome::CannotDoOffline(LiveData::News), 0, 0));
        assert_eq!(run("What's the weather in Cairo today?"), (Outcome::CannotDoOffline(LiveData::Weather), 0, 0));
        assert_eq!(run("What time is it?"), (Outcome::Time(NOW), 0, 0));
        let (outcome, cmd, writer) = run("Convert 2026-01-01 to Hijri");
        assert!(matches!(outcome, Outcome::HijriApprox { .. }), "{outcome:?}");
        assert_eq!((cmd, writer), (0, 0));
    }

    #[test]
    fn foreign_script_in_the_writer_reply_is_retried_on_the_writer() {
        let bad = "تم فتح الملف привет بنجاح";
        let good = "تم فتح الملف بنجاح";
        let m = model(text("command model text"));
        let w = model_seq(vec![text(bad), text(good)]);
        let (outcome, cmd, writer) = turn_with_writer(&m, &w, "اشرح لي ما حدث", Language::Arabic);
        assert_eq!(outcome, reply(good));
        assert_eq!((cmd, writer), (1, 2));
        assert!(w.last_system.borrow().contains("Reply again in Arabic"));
        assert!(!m.last_system.borrow().contains("Reply again"));
    }

    #[test]
    fn prompts_require_the_users_language_and_forbid_naming_tools() {
        let m = model(call("open_app", r#"{"name":"email"}"#));
        turn(&m, "فيك تفتحلي الإيميل؟", Language::Arabic);
        let system = m.last_system.borrow().clone();
        for needed in [
            "language and dialect of the user's latest message",
            "never in English",
            "Never mention tools or functions",
            "Never refuse because no tool fits",
            "Tools are only for acting on this computer",
            "is a command and not a question",
            "فيك",
            "ممكن",
            "لو سمحت",
            "بالعربية فقط",
            "لا تذكر الأدوات",
        ] {
            assert!(system.contains(needed), "{needed}: {system}");
        }
        let w = model(text("اهلا"));
        turn_with_writer(&model(text("x")), &w, "مرحبا", Language::Arabic);
        let writer_system = w.last_system.borrow().clone();
        assert!(writer_system.contains("Never mention tools or functions"));
        assert!(writer_system.contains("Never refuse because no tool fits"));
        assert!(!writer_system.contains("Tools are only for acting"));
    }

    #[test]
    fn english_reply_to_an_arabic_request_is_retried_once() {
        let english = "Sorry, I can't play music.";
        let arabic = "عذرًا، لا أستطيع تشغيل الموسيقى هنا.";
        let m = model_seq(vec![text(english), text(arabic)]);
        assert_eq!(turn(&m, "شغلي أغنية", Language::Arabic), (reply(arabic), 2));
        assert!(m.last_system.borrow().contains("the user wrote in Arabic"));
        // Still English on the retry: the first reply is kept, with no third call.
        let m = model_seq(vec![text(english), text(english)]);
        assert_eq!(turn(&m, "شغلي أغنية", Language::Arabic), (reply(english), 2));
    }

    #[test]
    fn arabic_reply_to_an_english_request_is_retried_once() {
        let q = "What is the capital of Egypt? One word.";
        let m = model_seq(vec![text("القاهرة"), text("Cairo")]);
        assert_eq!(turn(&m, q, Language::English), (reply("Cairo"), 2));
        assert!(m.last_system.borrow().contains("the user wrote in English"));
        // Still Arabic on the retry: the first reply is kept, with no third call.
        let m = model_seq(vec![text("القاهرة"), text("القاهرة")]);
        assert_eq!(turn(&m, q, Language::English), (reply("القاهرة"), 2));
        // A failed retry keeps the first reply.
        let m = model_seq(vec![text("القاهرة"), Err("down".to_string())]);
        assert_eq!(turn(&m, q, Language::English), (reply("القاهرة"), 2));
        // The Arabic request keeps its own check, with its own retry text.
        let m = model_seq(vec![text("The capital is Cairo."), text("القاهرة")]);
        assert_eq!(turn(&m, "ما هي عاصمة مصر؟", Language::Arabic), (reply("القاهرة"), 2));
        assert!(m.last_system.borrow().contains("the user wrote in Arabic"));
    }

    #[test]
    fn a_right_language_or_a_translation_is_not_retried() {
        let m = model(text("Cairo"));
        assert_eq!(turn(&m, "What is the capital of Egypt? One word.", Language::English).1, 1);
        let m = model(text("القاهرة"));
        assert_eq!(turn(&m, "ما هي عاصمة مصر؟ كلمة واحدة.", Language::Arabic).1, 1);
        for user in ["Translate to Arabic: Cairo", "How do you say Cairo in Arabic?"] {
            let m = model(text("القاهرة"));
            assert_eq!(turn(&m, user, Language::English), (reply("القاهرة"), 1), "{user}");
        }
        // Mixed requests are not checked in either direction.
        let m = model(text("القاهرة"));
        assert_eq!(turn(&m, "What is القاهرة?", Language::English).1, 1);
    }

    #[test]
    fn the_writer_retries_an_arabic_reply_to_an_english_request() {
        let m = model(text("command model text"));
        let w = model_seq(vec![text("القاهرة"), text("Cairo")]);
        let (outcome, cmd, writer) = turn_with_writer(&m, &w, "What is the capital of Egypt? One word.", Language::English);
        assert_eq!((outcome, cmd, writer), (reply("Cairo"), 1, 2));
        assert!(w.last_system.borrow().contains("the user wrote in English"));
    }

    #[test]
    fn prompts_set_the_language_music_and_no_invented_features() {
        let m = model(call("open_app", r#"{"name":"music"}"#));
        turn(&m, "شغّل لي الموسيقى", Language::Arabic);
        let system = m.last_system.borrow().clone();
        for needed in [
            "reply in English only, and never in Arabic",
            "reply in the language of most of its words",
            "Only a request to translate",
            "فاتبع اللغة الغالبة",
            "keep it general",
            "unless a tool for it is offered",
            "Playing music, a song or a playlist means opening the music app",
            "Never say you have no tool for music",
            "تشغيل الموسيقى",
        ] {
            assert!(system.contains(needed), "{needed}: {system}");
        }
        let w = model(text("اهلا"));
        turn_with_writer(&model(text("x")), &w, "مرحبا", Language::Arabic);
        assert!(w.last_system.borrow().contains("keep it general"));
        assert!(!w.last_system.borrow().contains("opening the music app"));
    }

    type DialectPrompt = (String, String, Language, Option<(String, String)>);

    /// The dialect benchmark prompts: (id, text, language, expected tool name and arguments).
    fn dialect_prompts() -> Vec<DialectPrompt> {
        include_str!("../eval/prompts-dialects.jsonl")
            .lines()
            .map(|line| {
                let v: Value = serde_json::from_str(line).unwrap();
                let language = if v["lang"] == "en" { Language::English } else { Language::Arabic };
                let tool = v.get("tool").map(|t| (t["name"].as_str().unwrap().to_string(), t["args"].to_string()));
                (v["id"].as_str().unwrap().to_string(), v["prompt"].as_str().unwrap().to_string(), language, tool)
            })
            .collect()
    }

    #[test]
    fn code_does_not_stop_the_dialect_file_and_language_commands() {
        // dialect-eg-005/006 opened an app and dialect-gulf-005/013 called no tool on
        // the model. If the model proposes the right tool, nothing in code may block it.
        let mut checked = 0;
        for (id, user, language, tool) in dialect_prompts() {
            let Some((name, args)) = tool else { continue };
            if !matches!(name.as_str(), "open_app" | "list_files" | "search_files" | "set_language") {
                continue;
            }
            let m = model(call(&name, &args));
            let (outcome, calls) = turn(&m, &user, language);
            assert_eq!(calls, 1, "{id}: the model must be asked");
            assert!(offered_tools(&m).contains(&name), "{id}: {name} must be offered");
            match outcome {
                Outcome::Run(_) if name != "set_language" => {}
                Outcome::NeedsConfirmation(ToolCall::SetLanguage { .. }) if name == "set_language" => {}
                other => panic!("{id}: {other:?}"),
            }
            checked += 1;
        }
        assert_eq!(checked, 14);
    }

    #[test]
    fn prompt_separates_file_search_from_opening_an_app_and_names_the_language_command() {
        let m = model(call("search_files", r#"{"query":"فاتورة الكهربا"}"#));
        let (outcome, _) = turn(&m, "دوّرلي على ملف اسمه فاتورة الكهربا.", Language::Arabic);
        assert_eq!(outcome, Outcome::Run(ToolCall::SearchFiles { query: "فاتورة الكهربا".into() }));
        let system = m.last_system.borrow().clone();
        for needed in [
            "search_files with the name as spoken, never open_app",
            "دوّرلي على ملف",
            "دور لي على ملف",
            "هو search_files وليس open_app",
            "Changing the system language is set_language",
            "خلّ اللغة عربي",
            "هو set_language",
        ] {
            assert!(system.contains(needed), "{needed}");
        }
        let schema = tool_schema();
        let description = |name: &str| {
            schema.as_array().unwrap().iter().find(|t| t["function"]["name"] == name).unwrap()["function"]["description"]
                .as_str()
                .unwrap()
                .to_string()
        };
        assert!(description("open_app").contains("Not for files"));
        assert!(description("search_files").contains("دوّر"));
    }

    #[test]
    fn chinese_punctuation_in_an_arabic_reply_is_retried_once() {
        // dialect-eg-015 leaked Chinese into the answer.
        let m = model_seq(vec![text("الواي فاي，هو اتصال لاسلكي"), text("الواي فاي اتصال لاسلكي.")]);
        let (outcome, calls) = turn(&m, "إيه الفرق بين الواي فاي والداتا؟", Language::Arabic);
        assert_eq!(calls, 2);
        assert_eq!(outcome, reply("الواي فاي اتصال لاسلكي."));
        assert!(m.last_system.borrow().contains("foreign script"));
    }

    #[test]
    fn leaked_characters_are_removed_when_a_sensible_reply_is_left() {
        // The retry is as bad: only the leaked runs go, the rest is shown.
        let m = model_seq(vec![
            text("الواي فاي هو اتصال لاسلكي 移动 بالراوتر، والداتا هي إنترنت الموبايل（移动网络）."),
            text("الواي فاي اتصال لاسلكي 移动 بالراوتر، والداتا إنترنت الموبايل."),
        ]);
        let (outcome, calls) = turn(&m, "إيه الفرق بين الواي فاي والداتا؟", Language::Arabic);
        assert_eq!(calls, 2);
        assert_eq!(outcome, reply("الواي فاي اتصال لاسلكي بالراوتر، والداتا إنترنت الموبايل."));
        // Cyrillic in an English reply, and no retry text at all.
        let m = model(text("Wi-Fi is a wireless link to your router, while data uses the phone network. Привет"));
        let (outcome, calls) = turn(&m, "What is the difference between Wi-Fi and data?", Language::English);
        assert_eq!(calls, 2);
        assert_eq!(
            outcome,
            reply("Wi-Fi is a wireless link to your router, while data uses the phone network.")
        );
    }

    #[test]
    fn a_reply_that_is_mostly_leaked_characters_becomes_a_fixed_apology() {
        for (user, language, bad, apology) in [
            ("إيه الفرق بين الواي فاي والداتا؟", Language::Arabic, "移动网络是一种无线连接方式 شبكة", APOLOGY_AR),
            ("What is Wi-Fi?", Language::English, "移动", APOLOGY_EN),
            ("What is Wi-Fi?", Language::English, "Это беспроводная сеть, Wi-Fi", APOLOGY_EN),
        ] {
            let m = model(text(bad));
            let (outcome, calls) = turn(&m, user, language);
            assert_eq!((outcome, calls), (reply(apology), 2), "{bad}");
        }
        assert!(guard::check_reply(APOLOGY_AR, Language::Arabic).is_empty());
        assert!(guard::check_reply(APOLOGY_EN, Language::English).is_empty());
        // A failed retry call still ends in the apology.
        let m = model_seq(vec![text("移动"), Err("down".into())]);
        assert_eq!(turn(&m, "What is Wi-Fi?", Language::English), (reply(APOLOGY_EN), 2));
    }

    #[test]
    fn a_script_the_user_wrote_or_asked_for_is_kept() {
        let m = model(text("你好"));
        assert_eq!(turn(&m, "How do you say hello in Chinese?", Language::English), (reply("你好"), 1));
        let m = model(text("Привет"));
        assert_eq!(turn(&m, "ترجم مرحبا إلى الروسية", Language::Arabic), (reply("Привет"), 1));
        let m = model(text("移动 means mobile."));
        assert_eq!(turn(&m, "What does 移动 mean?", Language::English), (reply("移动 means mobile."), 1));
    }

    #[test]
    fn a_reply_that_offers_a_missing_ability_is_retried_once() {
        let bad = "I can delete the old files for you.";
        let good = "Old files are best reviewed by hand; keep what you still use.";
        let user = "My downloads folder is a mess, what should I do?";
        let m = model_seq(vec![text(bad), text(good)]);
        assert_eq!(turn(&m, user, Language::English), (reply(good), 2));
        assert!(m.last_system.borrow().contains("cannot do here"));
        let m = model_seq(vec![text(bad), text(bad)]);
        assert_eq!(turn(&m, user, Language::English), (reply(bad), 2));
        let m = model(text(good));
        assert_eq!(turn(&m, user, Language::English).1, 1);
        let m = model_seq(vec![text("سأحذف الملفات القديمة لك."), text("راجع الملفات القديمة بنفسك.")]);
        assert_eq!(turn(&m, "مجلد التنزيلات مليان، شو أعمل؟", Language::Arabic), (reply("راجع الملفات القديمة بنفسك."), 2));
    }

    #[test]
    fn the_ability_check_gets_the_tools_that_were_offered() {
        // list_files is offered to the command model, so listing is no false claim there.
        let m = model(text("I can list the files in your Documents folder."));
        assert_eq!(turn(&m, "hello there", Language::English).1, 1);
        assert!(offered_tools(&m).contains(&"list_files".to_string()));
        // The writer has no tools, so the same sentence is a claim on the writer.
        let m = model(text("command model text"));
        let w = model_seq(vec![text("I can list the files in your Documents folder."), text("Hello! How can I help?")]);
        let (outcome, cmd, writer) = turn_with_writer(&m, &w, "hello there", Language::English);
        assert_eq!((outcome, cmd, writer), (reply("Hello! How can I help?"), 1, 2));
    }

    #[test]
    fn english_reply_is_right_when_the_arabic_user_asked_for_it() {
        for user in ["ترجم إلى الإنجليزية: صباح الخير", "اكتب لي رسالة بالإنجليزية"] {
            let m = model(text("Good morning"));
            assert_eq!(turn(&m, user, Language::Arabic), (reply("Good morning"), 1), "{user}");
        }
        // Code-switched and English requests are not checked.
        let m = model(text("Sure, here it is."));
        assert_eq!(turn(&m, "اكتب email قصير", Language::Arabic).1, 1);
        assert_eq!(turn(&m, "Write a short email", Language::English).1, 2);
    }

    #[test]
    fn reply_that_names_a_tool_is_retried_once() {
        let bad = "I can use the delete_file tool for that.";
        let good = "I cannot delete files here.";
        let m = model_seq(vec![text(bad), text(good)]);
        assert_eq!(turn(&m, "Delete my old files", Language::English), (reply(good), 2));
        assert!(m.last_system.borrow().contains("named a tool or function"));
        let m = model_seq(vec![text(bad), text(bad)]);
        assert_eq!(turn(&m, "Delete my old files", Language::English), (reply(bad), 2));
        let m = model(text("Opening your files now."));
        assert_eq!(turn(&m, "Delete my old files", Language::English).1, 1);
    }

    #[test]
    fn language_and_tool_name_retries_happen_on_the_writer() {
        let m = model(text("command model text"));
        let w = model_seq(vec![text("I would call open_app."), text("Done.")]);
        let (outcome, cmd, writer) = turn_with_writer(&m, &w, "Open email", Language::English);
        assert_eq!((outcome, cmd, writer), (reply("Done."), 1, 2));
        assert!(w.last_system.borrow().contains("named a tool or function"));
        assert!(!w.last_system.borrow().contains("Tools are only for acting"));
    }
}
