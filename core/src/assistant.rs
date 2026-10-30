//! One turn of the assistant: user text in, a description of what should
//! happen out. Nothing here runs a tool. Time and live data come from code,
//! never from the model, and every reply is checked before it is returned.

use serde_json::{json, Value};

use crate::datetime::Date;
use crate::guard::{self, Issue, LiveData};
use crate::hijri::{self, HijriDate};
use crate::lang::Language;
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
    Error(String),
}

pub struct Assistant<'a> {
    model: &'a dyn Model,
    clock: &'a dyn Clock,
}

impl<'a> Assistant<'a> {
    pub fn new(model: &'a dyn Model, clock: &'a dyn Clock) -> Self {
        Assistant { model, clock }
    }

    pub fn handle(&self, user_text: &str, language: Language) -> Outcome {
        if guard::asks_religious(user_text) {
            return Outcome::Declined;
        }
        match guard::asks_for_live_data(user_text) {
            Some(LiveData::Time) => return self.time_outcome(requested_place(user_text).as_deref()),
            Some(live) => return Outcome::CannotDoOffline(live),
            None => {}
        }
        if guard::asks_for_hijri(user_text) {
            return match gregorian_date_in(user_text) {
                Some(date) => Outcome::HijriApprox { gregorian: date, hijri: hijri::to_hijri_approx(date) },
                None => Outcome::MissingDate,
            };
        }
        let now = match self.clock.now() {
            Ok(now) => now,
            Err(e) => return Outcome::Error(e.to_string()),
        };
        let system = format!(
            "You are the assistant built into ScarfeeOS. Today is {}, {:04}-{:02}-{:02}. \
             Reply in the language the user wrote in. Call a tool only when one fits the request. Never invent data.",
            now.weekday_name(),
            now.date.year,
            now.date.month,
            now.date.day
        );
        let translating = guard::asks_to_translate(user_text);
        let tools = tool_schema_for(!translating);
        match self.model.chat(&system, user_text, &tools) {
            Err(e) => Outcome::Error(e),
            Ok(ModelReply::Text(text)) => self.checked_reply(&system, user_text, language, &tools, text),
            Ok(ModelReply::ToolCalls(calls)) => self.tool_outcome(user_text, now, translating, calls),
        }
    }

    /// Asks the model once more when the reply has words in a foreign script,
    /// and keeps whichever reply has fewer of them.
    fn checked_reply(&self, system: &str, user_text: &str, language: Language, tools: &Value, text: String) -> Outcome {
        let issues = guard::check_reply(&text, language);
        if !issues.is_empty() {
            let name = match language {
                Language::English => "English",
                Language::Arabic => "Arabic",
            };
            let retry = format!(
                "{system} Your last reply had words in a foreign script. Reply again in {name}, with no words in other scripts."
            );
            if let Ok(ModelReply::Text(again)) = self.model.chat(&retry, user_text, tools) {
                let again_issues = guard::check_reply(&again, language);
                if again_issues.len() < issues.len() {
                    return Outcome::Reply { text: again, issues: again_issues };
                }
            }
        }
        Outcome::Reply { text, issues }
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

/// The tools offered to the model, in the OpenAI `tools` format.
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
        tool("open_app", "Open an application.", json!({"name": text("App name")}), &["name"]),
        tool("list_files", "List files in a folder.", json!({"path": text("Folder path")}), &["path"]),
        tool("search_files", "Search for files by name.", json!({"query": text("Search text")}), &["query"]),
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
        tool(
            "current_time",
            "Get the current local time.",
            json!({"timezone": text("Time zone, or local")}),
            &[],
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
    use crate::guard::Script;
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
        let (outcome, _) = turn(&m, "الساعة 11", Language::Arabic);
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
    fn russian_in_an_arabic_reply_is_flagged_with_the_reply() {
        let m = model(Ok(ModelReply::Text("تم فتح الملف привет بنجاح".into())));
        let (outcome, _) = turn(&m, "اشرح لي ما حدث", Language::Arabic);
        assert_eq!(
            outcome,
            Outcome::Reply {
                text: "تم فتح الملف привет بنجاح".into(),
                issues: vec![Issue { script: Script::Cyrillic, word: "привет".into() }],
            }
        );
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
            ["open_app", "list_files", "search_files", "create_reminder", "schedule_meeting", "set_language", "current_time"]
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
    fn religious_questions_are_declined_without_the_model() {
        let m = model(text("The five pillars are ..."));
        for (user, language) in [
            ("What are the five pillars of Islam?", Language::English),
            ("ما هي أركان الإسلام الخمسة؟", Language::Arabic),
            ("Who wrote the Bible?", Language::English),
            ("ما حكم الصلاة في السفر", Language::Arabic),
            ("What's the news about Islam?", Language::English),
        ] {
            assert_eq!(turn(&m, user, language), (Outcome::Declined, 0), "{user}");
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
        // The retry is worse: the first reply is kept with its issues.
        let worse = "تم فتح الملف привет مرحبا بنجاح";
        let m = model_seq(vec![text(bad), text(worse)]);
        let (outcome, calls) = turn(&m, "اشرح لي ما حدث", Language::Arabic);
        assert_eq!(calls, 2);
        assert_eq!(
            outcome,
            Outcome::Reply { text: bad.into(), issues: vec![Issue { script: Script::Cyrillic, word: "привет".into() }] }
        );
        // The retry is just as bad: the first reply is kept, and there is no third call.
        let m = model_seq(vec![text(bad), text(bad)]);
        assert_eq!(turn(&m, "اشرح لي ما حدث", Language::Arabic).1, 2);
    }

    #[test]
    fn retry_with_fewer_issues_is_returned_with_its_issues() {
        let two = "ملف привет и мир";
        let one = "ملف привет";
        let m = model_seq(vec![text(two), text(one)]);
        let (outcome, calls) = turn(&m, "اشرح", Language::Arabic);
        assert_eq!(calls, 2);
        assert_eq!(
            outcome,
            Outcome::Reply { text: one.into(), issues: vec![Issue { script: Script::Cyrillic, word: "привет".into() }] }
        );
    }

    #[test]
    fn failed_or_tool_retry_keeps_the_first_reply() {
        let bad = "تم فتح الملف привет بنجاح";
        for retry in [Err("local provider: request failed".to_string()), call("open_app", r#"{"name":"x"}"#)] {
            let m = model_seq(vec![text(bad), retry]);
            let (outcome, calls) = turn(&m, "اشرح لي ما حدث", Language::Arabic);
            assert_eq!(calls, 2);
            assert!(matches!(outcome, Outcome::Reply { ref text, ref issues } if text == bad && issues.len() == 1), "{outcome:?}");
        }
    }

    #[test]
    fn clean_reply_is_not_retried() {
        let m = model(text("أهلاً"));
        assert_eq!(turn(&m, "مرحبا", Language::Arabic).1, 1);
    }
}
