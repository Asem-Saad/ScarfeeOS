//! JSON line interface between the panel and the core. One request per line in,
//! one response per line out. Nothing here runs a tool: a tool call is only
//! described, and the shell decides what to do with it.
//!
//! Request:  {"id": 1, "text": "...", "language": "en" | "ar"}
//! Response: {"id": 1, "kind": "...", ...}. A request that cannot be read gets
//! {"id": <the id if it had one, else null>, "kind": "error", "message": "..."}.

use std::io::{self, BufRead, Write};

use serde_json::{json, Value};

use crate::assistant::{Assistant, Outcome};
use crate::datetime::Repeat;
use crate::guard::LiveData;
use crate::lang::Language;
use crate::tools::ToolCall;

/// Answers every line of `input` with one line on `output`, until end of input.
/// A bad line gets an error response and the loop goes on. Only an I/O error
/// on the streams ends it early.
pub fn serve(assistant: &Assistant, mut input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    let mut buf = Vec::new();
    loop {
        buf.clear();
        if input.read_until(b'\n', &mut buf)? == 0 {
            return Ok(());
        }
        let response = match std::str::from_utf8(&buf) {
            Ok(line) => handle_line(assistant, line),
            Err(_) => error_response(Value::Null, "request is not valid UTF-8"),
        };
        writeln!(output, "{response}")?;
        output.flush()?;
    }
}

/// One request line to one response. A blank line is an error, so the panel
/// always gets exactly one response per line it sends.
pub fn handle_line(assistant: &Assistant, line: &str) -> Value {
    let request: Value = match serde_json::from_str(line.trim()) {
        Ok(v) => v,
        Err(e) => return error_response(Value::Null, &format!("malformed JSON: {e}")),
    };
    let Value::Object(request) = request else {
        return error_response(Value::Null, "request must be a JSON object");
    };
    let id = match request.get("id") {
        Some(id) if id.is_number() => id.clone(),
        _ => return error_response(Value::Null, "missing or non-numeric field `id`"),
    };
    let Some(Value::String(text)) = request.get("text") else {
        return error_response(id, "missing or non-string field `text`");
    };
    let Some(Value::String(code)) = request.get("language") else {
        return error_response(id, "missing or non-string field `language`");
    };
    let Some(language) = Language::from_code(code).filter(|l| l.code() == code) else {
        return error_response(id, &format!("unsupported language `{code}`: use \"en\" or \"ar\""));
    };
    response(id, &assistant.handle(text, language))
}

fn error_response(id: Value, message: &str) -> Value {
    json!({"id": id, "kind": "error", "message": message})
}

fn response(id: Value, outcome: &Outcome) -> Value {
    let mut body = match outcome {
        Outcome::Time(t) => json!({
            "kind": "time",
            "date": format!("{:04}-{:02}-{:02}", t.date.year, t.date.month, t.date.day),
            "hour": t.hour,
            "minute": t.minute,
            "weekday": t.weekday_name(),
        }),
        Outcome::UnsupportedTimeZone(zone) => json!({"kind": "unsupported_time_zone", "zone": zone}),
        Outcome::CannotDoOffline(live) => json!({
            "kind": "cannot_do_offline",
            "what": match live {
                LiveData::Time => "time",
                LiveData::News => "news",
                LiveData::Weather => "weather",
            },
        }),
        Outcome::Reply { text, issues } => json!({
            "kind": "reply",
            "text": text,
            "foreign_words": issues
                .iter()
                .map(|i| json!({"word": i.word, "script": format!("{:?}", i.script).to_lowercase()}))
                .collect::<Vec<_>>(),
        }),
        Outcome::Run(call) => tool_body("run", call),
        Outcome::NeedsClarification(call) => tool_body("needs_clarification", call),
        Outcome::NeedsConfirmation(call) => tool_body("needs_confirmation", call),
        Outcome::MissingTime(call) => tool_body("missing_time", call),
        Outcome::Error(message) => json!({"kind": "error", "message": message}),
        // Outcome gains variants from other work. Until this match learns them,
        // they reach the panel as kind "other" with their Debug text. The allow
        // is for while every variant is covered above.
        #[allow(unreachable_patterns)]
        other => json!({"kind": "other", "debug": format!("{other:?}")}),
    };
    body["id"] = id;
    body
}

fn tool_body(kind: &str, call: &ToolCall) -> Value {
    let (tool, arguments) = match call {
        ToolCall::OpenApp { name } => ("open_app", json!({"name": name})),
        ToolCall::ListFiles { path } => ("list_files", json!({"path": path})),
        ToolCall::SearchFiles { query } => ("search_files", json!({"query": query})),
        ToolCall::CreateReminder { text, time, repeat } => {
            ("create_reminder", json!({"text": text, "time": time, "repeat": repeat_name(*repeat)}))
        }
        ToolCall::ScheduleMeeting { title, time, attendees, repeat } => (
            "schedule_meeting",
            json!({"title": title, "time": time, "attendees": attendees, "repeat": repeat_name(*repeat)}),
        ),
        ToolCall::SetLanguage { lang } => ("set_language", json!({"lang": lang.code()})),
        ToolCall::CurrentTime { timezone } => ("current_time", json!({"timezone": timezone})),
    };
    json!({"kind": kind, "tool": tool, "arguments": arguments})
}

fn repeat_name(repeat: Option<Repeat>) -> Option<&'static str> {
    repeat.map(|r| match r {
        Repeat::Daily => "daily",
        Repeat::Weekly => "weekly",
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::assistant::Model;
    use crate::datetime::Date;
    use crate::provider::{ModelReply, RawToolCall};
    use crate::tools::{FixedClock, LocalTime};

    const NOW: LocalTime = LocalTime { date: Date { year: 2026, month: 10, day: 30 }, hour: 9, minute: 5, second: 30 };

    struct MockModel {
        reply: Result<ModelReply, String>,
        calls: Cell<u32>,
    }

    impl Model for MockModel {
        fn chat(&self, _system: &str, _user: &str, _tools: &Value) -> Result<ModelReply, String> {
            self.calls.set(self.calls.get() + 1);
            self.reply.clone()
        }
    }

    fn model(reply: Result<ModelReply, String>) -> MockModel {
        MockModel { reply, calls: Cell::new(0) }
    }

    fn call(name: &str, arguments: &str) -> Result<ModelReply, String> {
        Ok(ModelReply::ToolCalls(vec![RawToolCall { name: name.into(), arguments: arguments.into() }]))
    }

    fn ask(m: &MockModel, request: &Value) -> Value {
        let clock = FixedClock(NOW);
        handle_line(&Assistant::new(m, &clock), &request.to_string())
    }

    fn req(text: &str) -> Value {
        json!({"id": 7, "text": text, "language": "en"})
    }

    #[test]
    fn time_has_date_hour_minute_and_weekday() {
        let m = model(Err("must not be called".into()));
        assert_eq!(
            ask(&m, &req("What time is it?")),
            json!({"id": 7, "kind": "time", "date": "2026-10-30", "hour": 9, "minute": 5, "weekday": "Friday"})
        );
    }

    #[test]
    fn unsupported_time_zone_and_offline_kinds() {
        let m = model(Err("must not be called".into()));
        assert_eq!(
            ask(&m, &req("What time is it in Tokyo?")),
            json!({"id": 7, "kind": "unsupported_time_zone", "zone": "Tokyo"})
        );
        assert_eq!(
            ask(&m, &req("What's the news?")),
            json!({"id": 7, "kind": "cannot_do_offline", "what": "news"})
        );
        assert_eq!(
            ask(&m, &req("What's the weather today?")),
            json!({"id": 7, "kind": "cannot_do_offline", "what": "weather"})
        );
    }

    #[test]
    fn reply_carries_text_and_foreign_words() {
        let m = model(Ok(ModelReply::Text("Hello there".into())));
        assert_eq!(
            ask(&m, &req("Hi")),
            json!({"id": 7, "kind": "reply", "text": "Hello there", "foreign_words": []})
        );
        let m = model(Ok(ModelReply::Text("تم فتح الملف привет بنجاح".into())));
        assert_eq!(
            ask(&m, &json!({"id": 8, "text": "اشرح لي ما حدث", "language": "ar"})),
            json!({
                "id": 8,
                "kind": "reply",
                "text": "تم فتح الملف привет بنجاح",
                "foreign_words": [{"word": "привет", "script": "cyrillic"}],
            })
        );
    }

    #[test]
    fn run_names_the_tool_and_its_arguments() {
        let m = model(call("open_app", r#"{"name":"calculator"}"#));
        assert_eq!(
            ask(&m, &req("Open the calculator.")),
            json!({"id": 7, "kind": "run", "tool": "open_app", "arguments": {"name": "calculator"}})
        );
        let m = model(call("schedule_meeting", r#"{"title":"Review","time":"x","attendees":["Sara","Omar"]}"#));
        assert_eq!(
            ask(&m, &req("Schedule Review with Sara and Omar on Sunday at 2 pm.")),
            json!({"id": 7, "kind": "run", "tool": "schedule_meeting", "arguments": {
                "title": "Review", "time": "2026-11-01T14:00:00", "attendees": ["Sara", "Omar"], "repeat": null,
            }})
        );
    }

    #[test]
    fn confirmation_clarification_and_missing_time_kinds() {
        let m = model(call("set_language", r#"{"lang":"ar"}"#));
        assert_eq!(
            ask(&m, &req("Switch the system language to Arabic")),
            json!({"id": 7, "kind": "needs_confirmation", "tool": "set_language", "arguments": {"lang": "ar"}})
        );
        let m = model(call("create_reminder", r#"{"text":"x","time":"2020-01-01T11:00:00"}"#));
        assert_eq!(
            ask(&m, &json!({"id": 9, "text": "الساعة 11", "language": "ar"})),
            json!({"id": 9, "kind": "needs_clarification", "tool": "create_reminder", "arguments": {
                "text": "x", "time": "2026-10-30T11:00:00", "repeat": null,
            }})
        );
        let m = model(call("create_reminder", r#"{"text":"call the bank","time":"2020-01-01T00:00:00"}"#));
        assert_eq!(
            ask(&m, &req("Remind me to call the bank")),
            json!({"id": 7, "kind": "missing_time", "tool": "create_reminder", "arguments": {
                "text": "call the bank", "time": "", "repeat": null,
            }})
        );
    }

    #[test]
    fn repeat_is_named() {
        let m = model(call("create_reminder", r#"{"text":"stand up","time":"x"}"#));
        let r = ask(&m, &req("Remind me to stand up every day at 9 am"));
        assert_eq!(r["kind"], "run", "{r}");
        assert_eq!(r["arguments"]["repeat"], "daily", "{r}");
    }

    #[test]
    fn assistant_error_is_an_error_with_the_id() {
        let m = model(Err("local provider: request failed".into()));
        assert_eq!(
            ask(&m, &req("Hello")),
            json!({"id": 7, "kind": "error", "message": "local provider: request failed"})
        );
    }

    #[test]
    fn bad_requests_are_errors_and_never_reach_the_model() {
        let m = model(Err("must not be called".into()));
        let clock = FixedClock(NOW);
        let assistant = Assistant::new(&m, &clock);
        for (line, id) in [
            ("{not json", Value::Null),
            ("", Value::Null),
            ("[1,2]", Value::Null),
            (r#"{"text":"hi","language":"en"}"#, Value::Null),
            (r#"{"id":"1","text":"hi","language":"en"}"#, Value::Null),
            (r#"{"id":3,"language":"en"}"#, json!(3)),
            (r#"{"id":3,"text":5,"language":"en"}"#, json!(3)),
            (r#"{"id":3,"text":"hi"}"#, json!(3)),
            (r#"{"id":3,"text":"hi","language":"fr"}"#, json!(3)),
            (r#"{"id":3,"text":"hi","language":"ar_SA"}"#, json!(3)),
            (r#"{"id":3,"text":"hi","language":"EN"}"#, json!(3)),
        ] {
            let r = handle_line(&assistant, line);
            assert_eq!(r["kind"], "error", "{line}");
            assert_eq!(r["id"], id, "{line}");
            assert!(r["message"].as_str().is_some_and(|s| !s.is_empty()), "{line}");
        }
        assert_eq!(m.calls.get(), 0);
    }

    #[test]
    fn arabic_text_round_trips_unescaped() {
        let m = model(Ok(ModelReply::Text("أهلاً، كيف أساعدك؟".into())));
        let clock = FixedClock(NOW);
        let line = r#"{"id":1,"text":"مرحبا","language":"ar"}"#;
        let mut out = Vec::new();
        serve(&Assistant::new(&m, &clock), line.as_bytes(), &mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(out.ends_with('\n') && out.matches('\n').count() == 1);
        assert!(out.contains("\"text\":\"أهلاً، كيف أساعدك؟\""), "{out}");
        let r: Value = serde_json::from_str(&out).unwrap();
        assert_eq!((&r["id"], &r["kind"]), (&json!(1), &json!("reply")));
    }

    #[test]
    fn loop_answers_every_line_and_survives_bad_ones() {
        let m = model(Ok(ModelReply::Text("ok".into())));
        let clock = FixedClock(NOW);
        let input = concat!(
            "{\"id\":1,\"text\":\"What time is it?\",\"language\":\"en\"}\n",
            "garbage\n",
            "\n",
            "{\"id\":2,\"text\":\"Hi\",\"language\":\"en\"}\r\n",
            "{\"id\":3,\"text\":\"What's the news?\",\"language\":\"en\"}",
        );
        let mut out = Vec::new();
        serve(&Assistant::new(&m, &clock), input.as_bytes(), &mut out).unwrap();
        let lines: Vec<Value> =
            String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        let kinds: Vec<(&Value, &str)> = lines.iter().map(|l| (&l["id"], l["kind"].as_str().unwrap())).collect();
        assert_eq!(
            kinds,
            [
                (&json!(1), "time"),
                (&Value::Null, "error"),
                (&Value::Null, "error"),
                (&json!(2), "reply"),
                (&json!(3), "cannot_do_offline"),
            ]
        );
        assert_eq!(m.calls.get(), 1);
    }

    #[test]
    fn invalid_utf8_is_an_error_and_the_loop_continues() {
        let m = model(Ok(ModelReply::Text("ok".into())));
        let clock = FixedClock(NOW);
        let mut input = b"\xff\xfe\n".to_vec();
        input.extend_from_slice(b"{\"id\":2,\"text\":\"Hi\",\"language\":\"en\"}\n");
        let mut out = Vec::new();
        serve(&Assistant::new(&m, &clock), input.as_slice(), &mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"kind\":\"error\""), "{}", lines[0]);
        assert!(lines[1].contains("\"id\":2") && lines[1].contains("\"kind\":\"reply\""), "{}", lines[1]);
    }

    #[test]
    fn empty_input_exits_cleanly_with_no_output() {
        let m = model(Err("must not be called".into()));
        let clock = FixedClock(NOW);
        let mut out = Vec::new();
        serve(&Assistant::new(&m, &clock), &b""[..], &mut out).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn id_may_be_any_json_number() {
        let m = model(Err("must not be called".into()));
        let r = ask(&m, &json!({"id": 1.5, "text": "What time is it?", "language": "en"}));
        assert_eq!((r["id"].clone(), r["kind"].clone()), (json!(1.5), json!("time")));
    }
}
