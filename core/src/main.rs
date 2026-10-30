use scarfeeos_core::assistant::{Assistant, Outcome};
use scarfeeos_core::guard::LiveData;
use scarfeeos_core::ipc;
use scarfeeos_core::lang::Language;
use scarfeeos_core::provider::LocalProvider;
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

/// One turn against the local server at http://127.0.0.1:8080.
fn chat(text: &str, lang: Language) {
    let provider = LocalProvider::default();
    let outcome = Assistant::new(&provider, &SystemClock).handle(text, lang);
    println!("{}", describe(&outcome));
    if matches!(outcome, Outcome::Error(_)) {
        std::process::exit(1);
    }
}

/// JSON lines on stdin and stdout, for the shell. Only responses go to stdout.
fn serve() {
    let provider = LocalProvider::default();
    let assistant = Assistant::new(&provider, &SystemClock);
    if let Err(e) = ipc::serve(&assistant, std::io::stdin().lock(), std::io::stdout().lock()) {
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
