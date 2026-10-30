use scarfeeos_core::assistant::{Assistant, Outcome};
use scarfeeos_core::executor::Executor;
use scarfeeos_core::guard::LiveData;
use scarfeeos_core::ipc;
use scarfeeos_core::lang::Language;
use std::time::Duration;

use scarfeeos_core::provider::{LocalConfig, LocalProvider};
use scarfeeos_core::tools::SystemClock;

fn main() {
    let lang = std::env::var("SCARFEEOS_LANG")
        .ok()
        .and_then(|v| Language::from_code(&v))
        .unwrap_or(Language::English);
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => println!("ScarfeeOS core starting (language: {})", lang.native_name()),
        Some("chat") if args.len() > 1 => chat(&args[1..].join(" "), lang),
        Some("serve") if args.len() == 1 => serve(),
        Some(_) => {
            eprintln!("usage: scarfeeos-core [chat \"text\" | serve]");
            std::process::exit(2);
        }
    }
}

/// The command model at `SCARFEEOS_COMMAND_URL`, default http://127.0.0.1:8080,
/// and the writer at `SCARFEEOS_WRITER_URL` when that is set. Both must be loopback.
fn providers() -> Result<(LocalProvider, Option<LocalProvider>), String> {
    let url = |var: &str| std::env::var(var).ok().filter(|v| !v.is_empty());
    let mut command = LocalConfig::default();
    if let Some(base_url) = url("SCARFEEOS_COMMAND_URL") {
        command.base_url = base_url;
    }
    // Jais took up to 120 seconds in testing, so the writer waits longer.
    let writer = url("SCARFEEOS_WRITER_URL").map(|base_url| LocalConfig {
        base_url,
        timeout: Duration::from_secs(300),
        ..LocalConfig::default()
    });
    Ok((LocalProvider::new(command)?, writer.map(LocalProvider::new).transpose()?))
}

fn assistant_for<'a>(command: &'a LocalProvider, writer: &'a Option<LocalProvider>) -> Assistant<'a> {
    let assistant = Assistant::new(command, &SystemClock);
    match writer {
        Some(writer) => assistant.with_writer(writer),
        None => assistant,
    }
}

/// One turn against the local servers.
fn chat(text: &str, lang: Language) {
    let (command, writer) = providers().unwrap_or_else(|e| {
        eprintln!("chat: {e}");
        std::process::exit(2);
    });
    let outcome = assistant_for(&command, &writer).handle(text, lang);
    println!("{}", describe(&outcome));
    if matches!(outcome, Outcome::Error(_)) {
        std::process::exit(1);
    }
}

/// JSON lines on stdin and stdout, for the shell. Only responses go to stdout.
/// Confirmed tools run here; `chat` only describes them.
fn serve() {
    let (command, writer) = providers().unwrap_or_else(|e| {
        eprintln!("serve: {e}");
        std::process::exit(2);
    });
    let mut executor = Executor::from_env().unwrap_or_else(|e| {
        eprintln!("serve: {e}");
        std::process::exit(2);
    });
    let assistant = assistant_for(&command, &writer);
    if let Err(e) = ipc::serve(&assistant, &mut executor, std::io::stdin().lock(), std::io::stdout().lock()) {
        eprintln!("serve: {e}");
        std::process::exit(1);
    }
}

fn describe(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Time(t) => format!(
            "time: {}, {:04}-{:02}-{:02} {:02}:{:02}:{:02} (device time)",
            t.weekday_name(),
            t.date.year,
            t.date.month,
            t.date.day,
            t.hour,
            t.minute,
            t.second
        ),
        Outcome::UnsupportedTimeZone(zone) => {
            format!("cannot tell the time in {zone}: only the device's own time zone is supported")
        }
        Outcome::CannotDoOffline(LiveData::News) => "cannot do this offline: news".into(),
        Outcome::CannotDoOffline(LiveData::Weather) => "cannot do this offline: weather".into(),
        Outcome::CannotDoOffline(LiveData::Time) => "cannot do this offline: time".into(),
        Outcome::Reply { text, issues } if issues.is_empty() => format!("reply: {text}"),
        Outcome::Reply { text, issues } => {
            let words: Vec<String> = issues.iter().map(|i| format!("{:?} \"{}\"", i.script, i.word)).collect();
            format!("reply: {text}\nwarning: words in the wrong script: {}", words.join(", "))
        }
        Outcome::Run(call) => format!("run: {call:?}"),
        Outcome::NeedsClarification(call) => format!("ask the user to confirm the time: {call:?}"),
        Outcome::NeedsConfirmation(call) => format!("ask the user to confirm before running: {call:?}"),
        Outcome::MissingTime(call) => format!("ask the user for a date and time: {call:?}"),
        Outcome::Declined => "declined: religious content is not provided".into(),
        Outcome::HijriApprox { gregorian, hijri } => format!(
            "about {} {} {} AH ({:04}-{:02}-{:02}), approximate",
            hijri.day,
            hijri.month_name_en(),
            hijri.year,
            gregorian.year,
            gregorian.month,
            gregorian.day
        ),
        Outcome::MissingDate => "ask the user for a Gregorian date in digits".into(),
        Outcome::Error(e) => format!("error: {e}"),
    }
}
