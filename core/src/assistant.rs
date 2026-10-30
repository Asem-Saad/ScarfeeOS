//! One turn of the assistant: user text in, a description of what should
//! happen out. Nothing here runs a tool. Time and live data come from code,
//! never from the model, and every reply is checked before it is returned.

use serde_json::{json, Value};

use crate::guard::{self, Issue, LiveData};
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
        match guard::asks_for_live_data(user_text) {
            Some(LiveData::Time) => return self.time_outcome(requested_place(user_text).as_deref()),
            Some(live) => return Outcome::CannotDoOffline(live),
            None => {}
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
        match self.model.chat(&system, user_text, &tool_schema()) {
            Err(e) => Outcome::Error(e),
            Ok(ModelReply::Text(text)) => {
                let issues = guard::check_reply(&text, language);
                Outcome::Reply { text, issues }
            }
            Ok(ModelReply::ToolCalls(calls)) => self.tool_outcome(user_text, now, calls),
        }
    }

    fn time_outcome(&self, timezone: Option<&str>) -> Outcome {
        match tools::current_time(self.clock, timezone) {
            Ok(now) => Outcome::Time(now),
            Err(ClockError::UnsupportedTimeZone(zone)) => Outcome::UnsupportedTimeZone(zone),
            Err(e) => Outcome::Error(e.to_string()),
        }
    }

    fn tool_outcome(&self, user_text: &str, now: LocalTime, mut calls: Vec<RawToolCall>) -> Outcome {
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
    fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
        json!({"type": "function", "function": {
            "name": name,
            "description": description,
            "parameters": {"type": "object", "properties": properties, "required": required},
        }})
    }
    let text = |d: &str| json!({"type": "string", "description": d});
    json!([
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
    ])
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

    struct MockModel {
        reply: Result<ModelReply, String>,
        calls: Cell<u32>,
        last_system: RefCell<String>,
    }

    impl Model for MockModel {
        fn chat(&self, system: &str, _user: &str, tools: &Value) -> Result<ModelReply, String> {
            assert_eq!(tools, &tool_schema());
            self.calls.set(self.calls.get() + 1);
            *self.last_system.borrow_mut() = system.to_string();
            self.reply.clone()
        }
    }

    fn model(reply: Result<ModelReply, String>) -> MockModel {
        MockModel { reply, calls: Cell::new(0), last_system: RefCell::new(String::new()) }
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
        let (outcome, _) = turn(&m, "Translate this sentence into Arabic: good morning", Language::English);
        assert_eq!(outcome, Outcome::NeedsConfirmation(ToolCall::SetLanguage { lang: Language::Arabic }));
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
}
