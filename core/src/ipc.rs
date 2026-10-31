//! JSON line interface between the panel and the core. One request per line in,
//! one response per line out. Every request has a numeric `id` and exactly one
//! of these keys:
//!
//! - `text` (with `language`, "en" or "ar"): a turn with the assistant.
//! - `confirm`: an action id from a held response. Runs that call, once. For
//!   `needs_clarification` it may also carry `time`, local ISO 8601, to use
//!   instead of the time the assistant guessed.
//! - `cancel`: an action id. Drops that call without running it.
//! - `upcoming` (true, with `from` as YYYY-MM-DD, default today): the events
//!   from that day on, soonest first.
//! - `delete_event`: the UID of an event to remove from the calendar.
//! - `hardware` (true): what this machine has, the tier the installer
//!   recommends and the model plans (see `hardware_body`).
//!
//! Response: {"id": 1, "kind": "...", ...}. A request that cannot be read gets
//! {"id": <the id if it had one, else null>, "kind": "error", "message": "..."}.
//!
//! `time_only` has the `time` the user wrote and nothing held: the message was
//! only a time, so the panel asks what the user wants at that time.
//!
//! `run` has a `result` saying what happened, `{"ok": true, "type": ...}` or
//! `{"ok": false, "error": {"code", "message"}}`. `needs_clarification` and
//! `needs_confirmation` have an `action_id` and have run nothing; a confirm
//! answers with `run`, a cancel with `cancelled`. Action ids are random, work
//! once and are forgotten when the core stops. The call that runs is always the
//! one held by the core, never one sent by the shell or written by the model.

use std::io::{self, BufRead, Write};

use serde_json::{json, Map, Value};

use crate::assistant::{Assistant, Outcome};
use crate::executor::{repeat_name, Executor, Held};
use crate::guard::LiveData;
use crate::hardware::{self, GpuDevice, GpuKind, GpuVendor, Hardware, Tier};
use crate::lang::Language;
use crate::tiers::{self, ModelPlan, ModelSpec};
use crate::tools::ToolCall;

/// Answers every line of `input` with one line on `output`, until end of input.
/// A bad line gets an error response and the loop goes on. Only an I/O error
/// on the streams ends it early.
pub fn serve(
    assistant: &Assistant,
    executor: &mut Executor,
    mut input: impl BufRead,
    mut output: impl Write,
) -> io::Result<()> {
    let mut buf = Vec::new();
    loop {
        buf.clear();
        if input.read_until(b'\n', &mut buf)? == 0 {
            return Ok(());
        }
        let response = match std::str::from_utf8(&buf) {
            Ok(line) => handle_line(assistant, executor, line),
            Err(_) => error_response(Value::Null, "request is not valid UTF-8"),
        };
        writeln!(output, "{response}")?;
        output.flush()?;
    }
}

/// One request line to one response. A blank line is an error, so the panel
/// always gets exactly one response per line it sends.
pub fn handle_line(assistant: &Assistant, executor: &mut Executor, line: &str) -> Value {
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
    let present: Vec<&str> = ACTIONS.iter().copied().filter(|key| request.contains_key(*key)).collect();
    let [action] = present[..] else {
        return error_response(id, "request must have exactly one of `text`, `confirm`, `cancel`, `upcoming`, `delete_event`, `hardware`");
    };
    let string = |key: &str| match request.get(key) {
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        None => Ok(None),
        Some(_) => Err(format!("non-string field `{key}`")),
    };
    let handled: Result<Value, String> = match action {
        "text" => return handle_text(assistant, executor, id, &request),
        "confirm" => string("confirm").and_then(|action_id| {
            let (call, result) = executor.confirm(action_id.unwrap_or_default(), string("time")?)?;
            let mut body = tool_body("run", &call);
            body["result"] = result;
            Ok(body)
        }),
        "cancel" => string("cancel")
            .and_then(|action_id| executor.cancel(action_id.unwrap_or_default()))
            .map(|()| json!({"kind": "cancelled"})),
        "upcoming" if request["upcoming"] != json!(true) => Err("`upcoming` must be true".into()),
        "upcoming" => string("from").and_then(|from| executor.upcoming(from)).map(|mut events| {
            events["kind"] = json!("events");
            events
        }),
        "hardware" if request["hardware"] != json!(true) => Err("`hardware` must be true".into()),
        "hardware" => Ok(hardware_body(&hardware::detect())),
        _ => match string("delete_event") {
            Ok(Some(uid)) if !uid.is_empty() => {
                executor.delete_event(uid).map(|()| json!({"kind": "event_deleted", "uid": uid}))
            }
            Ok(_) => Err("`delete_event` must be the UID of an event".into()),
            Err(e) => Err(e),
        },
    };
    match handled {
        Ok(mut body) => {
            body["id"] = id;
            body
        }
        Err(message) => error_response(id, &message),
    }
}

const ACTIONS: [&str; 6] = ["text", "confirm", "cancel", "upcoming", "delete_event", "hardware"];

fn tier_name(tier: Tier) -> &'static str {
    match tier {
        Tier::Lite => "lite",
        Tier::Full => "full",
        Tier::Dual => "dual",
    }
}

fn model_body(model: &ModelSpec) -> Value {
    json!({"name": model.name, "memory_mib": model.memory_mib, "measured": model.measured})
}

fn plan_body(plan: &ModelPlan) -> Value {
    json!({
        "tier": tier_name(plan.tier),
        "command": model_body(&plan.command),
        "writer": plan.writer.as_ref().map(model_body),
        "gpu_layers": plan.gpu_layers,
    })
}

fn gpu_body(d: &GpuDevice) -> Value {
    json!({
        "vendor": match d.vendor {
            GpuVendor::Amd => "amd",
            GpuVendor::Intel => "intel",
            GpuVendor::Nvidia => "nvidia",
        },
        "name": d.name,
        "kind": match d.kind {
            GpuKind::Discrete => "discrete",
            GpuKind::Integrated => "integrated",
            GpuKind::Unknown => "unknown",
        },
        "memory_mib": d.dedicated_memory_mib,
        "llama_cpp_usable": d.llama_cpp_usable,
    })
}

/// The `hardware` response: what was detected (`ram_kib` and `gpu` are null when unknown), the recommended
/// tier with the core's reason (English, for logs), the `plan` for that tier and a plan for each tier, so
/// the installer can show every choice. Nothing here blocks a tier: the installer only warns.
///
/// `gpu` is the one GPU the advice uses. `gpus` lists every GPU found, in order, each with `vendor`
/// ("amd", "intel" or "nvidia"), `name`, `kind` ("discrete", "integrated" or "unknown"), `memory_mib`
/// (dedicated memory; null for integrated GPUs and when unknown) and `llama_cpp_usable` (false: found
/// but cannot be used for the assistant).
fn hardware_body(hw: &Hardware) -> Value {
    let gpu_memory_mib = hw.gpu.as_ref().map(|g| g.memory_mib);
    let recommendation = hardware::recommend_tier(hw);
    let plan = |tier| plan_body(&tiers::plan_for(tier, gpu_memory_mib));
    json!({
        "kind": "hardware",
        "ram_kib": hw.ram_kib,
        "cpu": {"avx2": hw.cpu.avx2, "avx512f": hw.cpu.avx512f},
        "gpu": hw.gpu.as_ref().map(|g| json!({"name": g.name, "memory_mib": g.memory_mib})),
        "gpus": hw.gpus.iter().map(gpu_body).collect::<Vec<_>>(),
        "recommended": {"tier": tier_name(recommendation.tier), "reason": recommendation.reason},
        "plan": plan(recommendation.tier),
        "plans": {"lite": plan(Tier::Lite), "full": plan(Tier::Full), "dual": plan(Tier::Dual)},
    })
}

fn handle_text(assistant: &Assistant, executor: &mut Executor, id: Value, request: &Map<String, Value>) -> Value {
    let Some(Value::String(text)) = request.get("text") else {
        return error_response(id, "missing or non-string field `text`");
    };
    let Some(Value::String(code)) = request.get("language") else {
        return error_response(id, "missing or non-string field `language`");
    };
    let Some(language) = Language::from_code(code).filter(|l| l.code() == code) else {
        return error_response(id, &format!("unsupported language `{code}`: use \"en\" or \"ar\""));
    };
    response(id, &assistant.handle(text, language), executor)
}

fn error_response(id: Value, message: &str) -> Value {
    json!({"id": id, "kind": "error", "message": message})
}

fn response(id: Value, outcome: &Outcome, executor: &mut Executor) -> Value {
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
        Outcome::Run(call) => {
            let mut body = tool_body("run", call);
            body["result"] = executor.run(call);
            body
        }
        Outcome::NeedsClarification(call) => held_body(executor, Held::Clarification, "needs_clarification", call),
        Outcome::NeedsConfirmation(call) => held_body(executor, Held::Confirmation, "needs_confirmation", call),
        Outcome::MissingTime(call) => tool_body("missing_time", call),
        Outcome::TimeOnly(time) => json!({"kind": "time_only", "time": time}),
        Outcome::Error(message) => json!({"kind": "error", "message": message}),
        Outcome::Declined => json!({"kind": "declined"}),
        Outcome::HijriApprox { gregorian, hijri } => json!({
            "kind": "hijri_approx",
            "gregorian": format!("{:04}-{:02}-{:02}", gregorian.year, gregorian.month, gregorian.day),
            "year": hijri.year,
            "month": hijri.month,
            "day": hijri.day,
            "month_name_en": hijri.month_name_en(),
            "month_name_ar": hijri.month_name_ar(),
            "approximate": true,
        }),
        Outcome::MissingDate => json!({"kind": "missing_date"}),
    };
    body["id"] = id;
    body
}

fn held_body(executor: &mut Executor, held: Held, kind: &str, call: &ToolCall) -> Value {
    match executor.hold(held, call.clone()) {
        Ok(action_id) => {
            let mut body = tool_body(kind, call);
            body["action_id"] = json!(action_id);
            body
        }
        Err(message) => json!({"kind": "error", "message": message}),
    }
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    #[cfg(unix)]
    use std::rc::Rc;

    use super::*;
    use crate::assistant::Model;
    use crate::datetime::Date;
    use crate::executor::testing::Env;
    #[cfg(unix)]
    use crate::{
        actions::{App, LaunchError, Launcher},
        executor::testing::{app, FakeLauncher},
        executor::CommandLauncher,
    };
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

    /// One request in a fresh environment.
    fn ask(m: &MockModel, request: &Value) -> Value {
        ask_in(m, &mut Env::new(), request)
    }

    /// One request, in an environment that remembers held actions.
    fn ask_in(m: &MockModel, env: &mut Env, request: &Value) -> Value {
        let clock = FixedClock(NOW);
        handle_line(&Assistant::new(m, &clock), &mut env.executor, &request.to_string())
    }

    fn action_id(r: &Value) -> String {
        r["action_id"].as_str().unwrap_or_else(|| panic!("no action_id in {r}")).to_string()
    }

    fn without_action_id(mut r: Value) -> Value {
        assert_eq!(action_id(&r).len(), 32);
        r.as_object_mut().unwrap().remove("action_id");
        r
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
        let mut env = Env::new();
        assert_eq!(
            ask_in(&m, &mut env, &req("Open the calculator.")),
            json!({"id": 7, "kind": "run", "tool": "open_app", "arguments": {"name": "calculator"}, "result": {
                "ok": true, "type": "opened", "app": {"id": "calc.desktop", "name": "Calculator"},
            }})
        );
        assert_eq!(*env.launched.0.borrow(), ["calc.desktop"]);
        let m = model(call("schedule_meeting", r#"{"title":"Review","time":"x","attendees":["Sara","Omar"]}"#));
        let mut r = ask_in(&m, &mut env, &req("Schedule Review with Sara and Omar on Sunday at 2 pm."));
        let uid = r["result"]["event"]["uid"].as_str().unwrap().to_string();
        r["result"]["event"]["uid"] = json!("UID");
        assert_eq!(
            r,
            json!({"id": 7, "kind": "run", "tool": "schedule_meeting", "arguments": {
                "title": "Review", "time": "2026-11-01T14:00:00", "attendees": ["Sara", "Omar"], "repeat": null,
            }, "result": {"ok": true, "type": "event", "event": {
                "uid": "UID", "summary": "Review", "start": "2026-11-01T14:00:00",
                "attendees": ["Sara", "Omar"], "repeat": null,
            }}})
        );
        assert!(std::fs::read_to_string(&env.calendar).unwrap().contains(&uid));
    }

    #[test]
    fn confirmation_clarification_and_missing_time_kinds() {
        let m = model(call("set_language", r#"{"lang":"ar"}"#));
        assert_eq!(
            without_action_id(ask(&m, &req("Switch the system language to Arabic"))),
            json!({"id": 7, "kind": "needs_confirmation", "tool": "set_language", "arguments": {"lang": "ar"}})
        );
        let m = model(call("create_reminder", r#"{"text":"x","time":"2020-01-01T11:00:00"}"#));
        assert_eq!(
            without_action_id(ask(&m, &json!({"id": 9, "text": "ذكرني الساعة 11 x", "language": "ar"}))),
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
    fn a_message_that_is_only_a_time_asks_what_to_do_and_holds_nothing() {
        let m = model(Ok(ModelReply::Text("الساعة الآن 11 صباحًا".into())));
        assert_eq!(
            ask(&m, &json!({"id": 4, "text": "الساعة 11", "language": "ar"})),
            json!({"id": 4, "kind": "time_only", "time": "11"})
        );
        assert_eq!(ask(&m, &req("at 3:30pm")), json!({"id": 7, "kind": "time_only", "time": "3:30pm"}));
    }

    #[test]
    fn declined_has_no_extra_fields() {
        let m = model(Ok(ModelReply::Text("The five pillars are ...".into())));
        assert_eq!(ask(&m, &req("What are the five pillars of Islam?")), json!({"id": 7, "kind": "declined"}));
        assert_eq!(
            ask(&m, &json!({"id": 8, "text": "ما هي أركان الإسلام الخمسة؟", "language": "ar"})),
            json!({"id": 8, "kind": "declined"})
        );
    }

    #[test]
    fn hijri_approx_has_both_month_names_and_is_marked_approximate() {
        let m = model(Err("must not be called".into()));
        assert_eq!(
            ask(&m, &req("What is the Hijri date for 1 January 2026?")),
            json!({
                "id": 7, "kind": "hijri_approx", "gregorian": "2026-01-01",
                "year": 1447, "month": 7, "day": 12,
                "month_name_en": "Rajab", "month_name_ar": "رجب", "approximate": true,
            })
        );
        let r = ask(&m, &json!({"id": 8, "text": "حول 11 مارس 2024 إلى التاريخ الهجري", "language": "ar"}));
        assert_eq!(
            r,
            json!({
                "id": 8, "kind": "hijri_approx", "gregorian": "2024-03-11",
                "year": 1445, "month": 9, "day": 1,
                "month_name_en": "Ramadan", "month_name_ar": "رمضان", "approximate": true,
            })
        );
        let r = ask(&m, &req("Hijri date for 2024-06-30"));
        assert_eq!((&r["month_name_en"], &r["month_name_ar"]), (&json!("Dhu al-Hijjah"), &json!("ذو الحجة")), "{r}");
    }

    #[test]
    fn missing_date_has_no_extra_fields() {
        let m = model(Err("must not be called".into()));
        assert_eq!(ask(&m, &req("What is the Hijri date?")), json!({"id": 7, "kind": "missing_date"}));
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
        let mut env = Env::new();
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
            (r#"{"id":3,"text":"hi","language":"en","confirm":"x"}"#, json!(3)),
            (r#"{"id":3,"confirm":"x","cancel":"x"}"#, json!(3)),
            (r#"{"id":3,"confirm":5}"#, json!(3)),
            (r#"{"id":3,"confirm":"x","time":7}"#, json!(3)),
            (r#"{"id":3,"cancel":null}"#, json!(3)),
            (r#"{"id":3,"upcoming":false}"#, json!(3)),
            (r#"{"id":3,"upcoming":true,"from":"soon"}"#, json!(3)),
            (r#"{"id":3,"delete_event":""}"#, json!(3)),
            (r#"{"id":3,"delete_event":4}"#, json!(3)),
            (r#"{"confirm":"x"}"#, Value::Null),
        ] {
            let r = handle_line(&assistant, &mut env.executor, line);
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
        serve(&Assistant::new(&m, &clock), &mut Env::new().executor, line.as_bytes(), &mut out).unwrap();
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
        serve(&Assistant::new(&m, &clock), &mut Env::new().executor, input.as_bytes(), &mut out).unwrap();
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
        serve(&Assistant::new(&m, &clock), &mut Env::new().executor, input.as_slice(), &mut out).unwrap();
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
        serve(&Assistant::new(&m, &clock), &mut Env::new().executor, &b""[..], &mut out).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn id_may_be_any_json_number() {
        let m = model(Err("must not be called".into()));
        let r = ask(&m, &json!({"id": 1.5, "text": "What time is it?", "language": "en"}));
        assert_eq!((r["id"].clone(), r["kind"].clone()), (json!(1.5), json!("time")));
    }

    #[test]
    fn a_held_call_waits_for_its_confirm_and_runs_once() {
        let mut env = Env::new();
        let m = model(call("create_reminder", r#"{"text":"x","time":"2020-01-01T11:00:00"}"#));
        let held = ask_in(&m, &mut env, &json!({"id": 9, "text": "ذكرني الساعة 11 x", "language": "ar"}));
        assert_eq!(held["kind"], "needs_clarification");
        assert!(held.get("result").is_none() && !env.calendar.exists());
        let id = action_id(&held);

        let mut r = ask_in(&m, &mut env, &json!({"id": 10, "confirm": id, "time": "2026-10-30T15:30"}));
        let uid = r["result"]["event"]["uid"].as_str().unwrap().to_string();
        r["result"]["event"]["uid"] = json!("UID");
        assert_eq!(
            r,
            json!({"id": 10, "kind": "run", "tool": "create_reminder",
                "arguments": {"text": "x", "time": "2026-10-30T15:30", "repeat": null},
                "result": {"ok": true, "type": "event", "event": {
                    "uid": "UID", "summary": "x", "start": "2026-10-30T15:30:00", "attendees": [], "repeat": null,
                }}})
        );
        assert!(std::fs::read_to_string(&env.calendar).unwrap().contains(&uid));

        let again = ask_in(&m, &mut env, &json!({"id": 11, "confirm": id}));
        assert_eq!((&again["kind"], &again["id"]), (&json!("error"), &json!(11)), "{again}");
        assert_eq!(std::fs::read_to_string(&env.calendar).unwrap().matches("BEGIN:VEVENT").count(), 1);
    }

    #[test]
    fn a_bad_time_keeps_the_action_and_the_guess_runs_without_one() {
        let mut env = Env::new();
        let m = model(call("create_reminder", r#"{"text":"x","time":"2020-01-01T11:00:00"}"#));
        let id = action_id(&ask_in(&m, &mut env, &json!({"id": 1, "text": "ذكرني الساعة 11 x", "language": "ar"})));
        let r = ask_in(&m, &mut env, &json!({"id": 2, "confirm": id, "time": "eleven"}));
        assert_eq!(r["kind"], "error", "{r}");
        assert!(!env.calendar.exists());
        let r = ask_in(&m, &mut env, &json!({"id": 3, "confirm": id}));
        assert_eq!(r["result"]["event"]["start"], "2026-10-30T11:00:00", "{r}");
    }

    #[test]
    fn set_language_is_returned_after_confirm_and_nothing_is_written() {
        let mut env = Env::new();
        let m = model(call("set_language", r#"{"lang":"ar"}"#));
        let held = ask_in(&m, &mut env, &req("Switch the system language to Arabic"));
        let r = ask_in(&m, &mut env, &json!({"id": 8, "confirm": action_id(&held)}));
        assert_eq!(
            r,
            json!({"id": 8, "kind": "run", "tool": "set_language", "arguments": {"lang": "ar"},
                "result": {"ok": true, "type": "language", "lang": "ar"}})
        );
        assert!(!env.calendar.exists());
    }

    #[test]
    fn cancel_drops_the_action() {
        let mut env = Env::new();
        let m = model(call("set_language", r#"{"lang":"ar"}"#));
        let id = action_id(&ask_in(&m, &mut env, &req("Switch the system language to Arabic")));
        assert_eq!(ask_in(&m, &mut env, &json!({"id": 2, "cancel": id})), json!({"id": 2, "kind": "cancelled"}));
        assert_eq!(ask_in(&m, &mut env, &json!({"id": 3, "confirm": id}))["kind"], "error");
        assert_eq!(ask_in(&m, &mut env, &json!({"id": 4, "cancel": id}))["kind"], "error");
    }

    #[test]
    fn confirming_an_unknown_id_is_an_error_and_runs_nothing() {
        let mut env = Env::new();
        let m = model(Err("must not be called".into()));
        for id in ["", "nope", "0123456789abcdef0123456789abcdef"] {
            let r = ask_in(&m, &mut env, &json!({"id": 5, "confirm": id, "time": "2026-10-30T15:30"}));
            assert_eq!((&r["kind"], &r["id"]), (&json!("error"), &json!(5)), "{id}: {r}");
            assert!(r.get("result").is_none());
        }
        assert!(!env.calendar.exists() && env.launched.0.borrow().is_empty());
        assert_eq!(m.calls.get(), 0);
    }

    #[test]
    fn a_failed_run_has_a_clear_error_result() {
        let m = model(call("open_app", r#"{"name":"calculater"}"#));
        let r = ask(&m, &req("Open the calculater"));
        assert_eq!(r["kind"], "run");
        assert_eq!(r["result"]["ok"], false);
        assert_eq!(r["result"]["error"]["code"], "app_not_found");
        assert_eq!(r["result"]["error"]["closest"], json!(["Calculator"]));
        // A real folder above the root; "/etc" is not an absolute path on Windows.
        let mut env = Env::new();
        let outside = env.tmp.0.display().to_string();
        let m = model(call("list_files", &json!({"path": outside}).to_string()));
        let r = ask_in(&m, &mut env, &req("List the folder above"));
        assert_eq!((&r["result"]["ok"], &r["result"]["error"]["code"]), (&json!(false), &json!("outside_root")), "{r}");
    }

    #[test]
    fn list_and_search_results_come_back() {
        let m = model(call("list_files", r#"{"path":"Documents"}"#));
        let r = ask(&m, &req("What is in my Documents folder?"));
        assert_eq!((&r["result"]["ok"], &r["result"]["type"]), (&json!(true), &json!("listing")), "{r}");
        assert!(r["result"]["entries"][0]["path"].as_str().unwrap().ends_with("budget 2026.txt"));
        let m = model(call("search_files", r#"{"query":"budget"}"#));
        let r = ask(&m, &req("Find the budget file"));
        assert_eq!(r["result"]["type"], "search", "{r}");
        assert_eq!(r["result"]["matches"][0]["kind"], "file");
    }

    #[test]
    fn upcoming_and_delete_event_work_on_the_calendar() {
        let mut env = Env::new();
        let m = model(call("schedule_meeting", r#"{"title":"Review","time":"x","attendees":[]}"#));
        let made = ask_in(&m, &mut env, &req("Schedule Review on Sunday at 2 pm."));
        let uid = made["result"]["event"]["uid"].as_str().unwrap().to_string();

        let r = ask_in(&m, &mut env, &json!({"id": 4, "upcoming": true}));
        assert_eq!((&r["id"], &r["kind"], &r["from"]), (&json!(4), &json!("events"), &json!("2026-10-30")), "{r}");
        assert_eq!(r["events"][0]["uid"], json!(uid));
        assert_eq!(r["events"][0]["date"], "2026-11-01");
        let r = ask_in(&m, &mut env, &json!({"id": 5, "upcoming": true, "from": "2026-11-02"}));
        assert_eq!(r["events"], json!([]));

        let r = ask_in(&m, &mut env, &json!({"id": 6, "delete_event": uid}));
        assert_eq!(r, json!({"id": 6, "kind": "event_deleted", "uid": uid}));
        assert_eq!(ask_in(&m, &mut env, &json!({"id": 7, "upcoming": true}))["events"], json!([]));
        assert_eq!(ask_in(&m, &mut env, &json!({"id": 8, "delete_event": uid}))["kind"], "error");
        assert_eq!(m.calls.get(), 1);
    }

    /// Counts launches and starts the real program.
    #[cfg(unix)]
    struct CountingCommand(Rc<Cell<u32>>);

    #[cfg(unix)]
    impl Launcher for CountingCommand {
        fn launch(&self, app: &App) -> Result<(), LaunchError> {
            self.0.set(self.0.get() + 1);
            CommandLauncher.launch(app)
        }
    }

    // Needs a POSIX shell, `touch` and `printf` to prove that no shell sits in between.
    #[cfg(unix)]
    #[test]
    fn model_text_cannot_reach_command() {
        let tmp_probe = Env::new();
        let dir = tmp_probe.tmp.0.join("probe");
        std::fs::create_dir_all(&dir).unwrap();
        let (args_file, pwned) = (dir.join("args"), dir.join("pwned"));
        let touch = format!("touch {}", pwned.display());
        // The app's own arguments are full of shell syntax. They must arrive as they are.
        let literal = [format!("a b; {touch}"), "$HOME".to_string(), "`id`".to_string(), "*".to_string()];
        let mut exec = vec!["/bin/sh".to_string(), "-c".into(), r#"printf '%s\n' "$@" > "$0""#.into()];
        exec.push(args_file.display().to_string());
        exec.extend(literal.iter().cloned());
        let notes = App { exec, ..app("notes.desktop", "Notes", &[]) };

        let launches = Rc::new(Cell::new(0));
        let mut env = Env::build(vec![notes], Box::new(CountingCommand(launches.clone())), FakeLauncher::default());

        // Whatever the model writes as an app name is only compared with the table.
        for name in [
            format!("Notes; {touch}"),
            format!("$({touch})"),
            format!("Notes && {touch}"),
            format!("/bin/sh -c '{touch}'"),
            format!("/usr/bin/{touch}"),
            "sh".to_string(),
            "../../bin/sh".to_string(),
            "notes.desktop --exec /bin/sh".to_string(),
        ] {
            let m = model(call("open_app", &json!({"name": name}).to_string()));
            let r = ask_in(&m, &mut env, &req("Open the notes app"));
            assert_eq!((&r["kind"], &r["result"]["ok"]), (&json!("run"), &json!(false)), "{name}: {r}");
        }
        assert_eq!(launches.get(), 0, "model text started a program");

        // The same holds for text in other tools, which never start a program.
        let m = model(call("create_reminder", &json!({"text": touch, "time": "x"}).to_string()));
        ask_in(&m, &mut env, &req("Remind me tomorrow at 9 am"));
        let m = model(call("list_files", &json!({"path": format!("; {touch}")}).to_string()));
        ask_in(&m, &mut env, &req("List my files"));
        assert_eq!(launches.get(), 0);

        // A real name starts the table's argument list, with no shell in between.
        let m = model(call("open_app", r#"{"name":"notes"}"#));
        let r = ask_in(&m, &mut env, &req("Open the notes app"));
        assert_eq!(r["result"]["ok"], true, "{r}");
        assert_eq!(launches.get(), 1);
        let mut written = String::new();
        for _ in 0..100 {
            written = std::fs::read_to_string(&args_file).unwrap_or_default();
            if written.lines().count() >= literal.len() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(written.lines().collect::<Vec<_>>(), literal.iter().map(String::as_str).collect::<Vec<_>>());
        assert!(!pwned.exists());
    }

    fn hardware_with(gpu_mib: Option<u64>) -> Hardware {
        use crate::hardware::{CpuFlags, Gpu};
        Hardware {
            ram_kib: Some(16_384_000),
            cpu: CpuFlags { avx2: true, avx512f: false },
            gpu: gpu_mib.map(|memory_mib| Gpu { name: "Test GPU".into(), memory_mib }),
            ..Hardware::default()
        }
    }

    #[test]
    fn hardware_body_for_a_big_gpu_recommends_dual_with_the_large_pair() {
        let r = hardware_body(&hardware_with(Some(12 * 1024)));
        assert_eq!(r["kind"], "hardware");
        assert_eq!(r["ram_kib"], 16_384_000);
        assert_eq!(r["cpu"], json!({"avx2": true, "avx512f": false}));
        assert_eq!(r["gpu"], json!({"name": "Test GPU", "memory_mib": 12288}));
        assert_eq!(r["recommended"]["tier"], "dual");
        assert!(r["recommended"]["reason"].as_str().unwrap().contains("Test GPU"));
        assert_eq!(r["plan"], r["plans"]["dual"]);
        assert_eq!(r["plan"]["command"], json!({"name": "Qwen3-8B", "memory_mib": 5939, "measured": true}));
        assert_eq!(r["plan"]["writer"], json!({"name": "Jais-3-3.7B", "memory_mib": 4096, "measured": true}));
        assert_eq!(r["plan"]["gpu_layers"], 999);
    }

    #[test]
    fn hardware_body_without_a_gpu_recommends_lite_but_plans_every_tier() {
        let r = hardware_body(&Hardware::default());
        assert_eq!(r["ram_kib"], Value::Null);
        assert_eq!(r["gpu"], Value::Null);
        assert_eq!(r["recommended"]["tier"], "lite");
        assert_eq!(r["plan"], r["plans"]["lite"]);
        assert_eq!(r["plan"], json!({
            "tier": "lite",
            "command": {"name": "Qwen3-4B", "memory_mib": 3789, "measured": false},
            "writer": null,
            "gpu_layers": 0,
        }));
        assert_eq!(r["plans"]["full"]["command"]["name"], "Qwen3-8B");
        assert_eq!(r["plans"]["dual"]["command"]["name"], "Qwen3-4B");
        assert_eq!(r["plans"]["dual"]["writer"]["name"], "Jais-3-3.7B");
    }

    #[test]
    fn hardware_body_lists_every_gpu_with_its_kind_and_whether_llama_cpp_can_use_it() {
        let device = |vendor, name: &str, kind, memory, usable| GpuDevice {
            vendor,
            name: name.to_string(),
            kind,
            dedicated_memory_mib: memory,
            llama_cpp_usable: usable,
        };
        let hw = Hardware {
            gpus: vec![
                device(GpuVendor::Amd, "AMD GPU 1002:1638", GpuKind::Integrated, None, true),
                device(GpuVendor::Intel, "Intel GPU 8086:46A6", GpuKind::Unknown, None, false),
                device(GpuVendor::Nvidia, "NVIDIA GeForce RTX 3060", GpuKind::Discrete, Some(12288), true),
            ],
            ..hardware_with(Some(12288))
        };
        let r = hardware_body(&hw);
        assert_eq!(
            r["gpus"],
            json!([
                {"vendor": "amd", "name": "AMD GPU 1002:1638", "kind": "integrated", "memory_mib": null, "llama_cpp_usable": true},
                {"vendor": "intel", "name": "Intel GPU 8086:46A6", "kind": "unknown", "memory_mib": null, "llama_cpp_usable": false},
                {"vendor": "nvidia", "name": "NVIDIA GeForce RTX 3060", "kind": "discrete", "memory_mib": 12288, "llama_cpp_usable": true},
            ])
        );
        // The old fields are unchanged.
        assert_eq!(r["gpu"], json!({"name": "Test GPU", "memory_mib": 12288}));
        assert_eq!(r["recommended"]["tier"], "dual");
        for key in ["kind", "ram_kib", "cpu", "gpu", "gpus", "recommended", "plan", "plans"] {
            assert!(r.get(key).is_some(), "missing {key}");
        }
    }

    #[test]
    fn hardware_body_without_gpus_has_an_empty_list() {
        assert_eq!(hardware_body(&Hardware::default())["gpus"], json!([]));
    }

    #[test]
    fn hardware_request_answers_with_the_detected_machine() {
        let m = model(Err("must not be called".into()));
        let r = ask(&m, &json!({"id": 9, "hardware": true}));
        assert_eq!((&r["id"], &r["kind"]), (&json!(9), &json!("hardware")), "{r}");
        let tier = r["recommended"]["tier"].as_str().unwrap();
        assert!(["lite", "full", "dual"].contains(&tier));
        assert_eq!(r["plan"]["tier"], tier);
        assert!(r["gpus"].is_array(), "{r}");
        assert_eq!(m.calls.get(), 0);
    }

    #[test]
    fn hardware_must_be_true_and_stand_alone() {
        let m = model(Err("must not be called".into()));
        for request in [
            json!({"id": 1, "hardware": false}),
            json!({"id": 1, "hardware": "yes"}),
            json!({"id": 1, "hardware": true, "upcoming": true}),
            json!({"id": 1, "hardware": true, "text": "hi", "language": "en"}),
        ] {
            let r = ask(&m, &request);
            assert_eq!((&r["id"], &r["kind"]), (&json!(1), &json!("error")), "{request}: {r}");
        }
    }
}
