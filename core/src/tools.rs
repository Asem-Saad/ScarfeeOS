//! Tool calls proposed by the model. Dates and times in them are computed in
//! code, never trusted from the model.

use std::fmt;
use std::process::Command;

use crate::datetime::{self, Date, Repeat};
use crate::lang::Language;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCall {
    OpenApp { name: String },
    ListFiles { path: String },
    SearchFiles { query: String },
    /// `time` is ISO 8601 local time, or a date alone when the text gave no time.
    CreateReminder { text: String, time: String, repeat: Option<Repeat> },
    ScheduleMeeting { title: String, time: String, attendees: Vec<String>, repeat: Option<Repeat> },
    SetLanguage { lang: Language },
    /// `timezone` of None or "local" means the device's time zone.
    CurrentTime { timezone: Option<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Ready(ToolCall),
    /// The time could mean two things. The call carries the best candidate so
    /// the UI can ask the user instead of trusting it.
    NeedsClarification(ToolCall),
    /// The text names no date or time. The caller must ask the user.
    MissingTime,
}

/// Replaces any time the model proposed with the one found in `user_text`.
/// Tools without a time pass through unchanged.
pub fn resolve_call(user_text: &str, today: Date, proposed: ToolCall) -> Resolution {
    if !matches!(proposed, ToolCall::CreateReminder { .. } | ToolCall::ScheduleMeeting { .. }) {
        return Resolution::Ready(proposed);
    }
    let Some(resolved) = datetime::resolve(user_text, today) else {
        return Resolution::MissingTime;
    };
    let time = resolved.to_iso();
    let repeat = resolved.repeat;
    let call = match proposed {
        ToolCall::CreateReminder { text, .. } => ToolCall::CreateReminder { text, time, repeat },
        ToolCall::ScheduleMeeting { title, attendees, .. } => {
            ToolCall::ScheduleMeeting { title, time, attendees, repeat }
        }
        other => other,
    };
    if resolved.ambiguous {
        Resolution::NeedsClarification(call)
    } else {
        Resolution::Ready(call)
    }
}

/// Local wall-clock time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    pub date: Date,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl LocalTime {
    pub fn weekday_name(&self) -> &'static str {
        const NAMES: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
        NAMES[self.date.weekday() as usize]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClockError {
    /// Only the device's local time zone is supported.
    UnsupportedTimeZone(String),
    Unavailable(String),
}

impl fmt::Display for ClockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClockError::UnsupportedTimeZone(zone) => write!(f, "time zone not supported: {zone}"),
            ClockError::Unavailable(why) => write!(f, "clock unavailable: {why}"),
        }
    }
}

pub trait Clock {
    fn now(&self) -> Result<LocalTime, ClockError>;
}

/// Reads local time from the system `date` command, because the standard
/// library cannot find the local time zone offset.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Result<LocalTime, ClockError> {
        let out = Command::new("date")
            .arg("+%Y-%m-%d %H:%M:%S")
            .output()
            .map_err(|e| ClockError::Unavailable(e.to_string()))?;
        if !out.status.success() {
            return Err(ClockError::Unavailable("date command failed".into()));
        }
        parse_local_time(String::from_utf8_lossy(&out.stdout).trim())
            .ok_or_else(|| ClockError::Unavailable("unreadable date output".into()))
    }
}

fn parse_local_time(s: &str) -> Option<LocalTime> {
    let (date, time) = s.split_once(' ')?;
    let mut d = date.split('-');
    let mut t = time.split(':');
    let next = |it: &mut std::str::Split<'_, char>| it.next()?.parse::<u32>().ok();
    let date = Date::new(next(&mut d)? as i32, next(&mut d)?, next(&mut d)?);
    let (hour, minute, second) = (next(&mut t)?, next(&mut t)?, next(&mut t)?);
    let valid = (1..=12).contains(&date.month) && (1..=31).contains(&date.day);
    (valid && hour < 24 && minute < 60 && second < 61).then_some(LocalTime { date, hour, minute, second })
}

/// A clock that always reads the same time, for tests.
pub struct FixedClock(pub LocalTime);

impl Clock for FixedClock {
    fn now(&self) -> Result<LocalTime, ClockError> {
        Ok(self.0)
    }
}

/// Runs `current_time`. Only the local zone is supported: a named zone is an
/// error, never a guessed time.
pub fn current_time(clock: &dyn Clock, timezone: Option<&str>) -> Result<LocalTime, ClockError> {
    match timezone.map(str::trim) {
        None | Some("") => clock.now(),
        Some(zone) if zone.eq_ignore_ascii_case("local") => clock.now(),
        Some(zone) => Err(ClockError::UnsupportedTimeZone(zone.to_string())),
    }
}

/// Today's date in local time, to pass to `resolve_call`.
pub fn today(clock: &dyn Clock) -> Result<Date, ClockError> {
    Ok(clock.now()?.date)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Friday, the day of the baseline run where the model got all of these wrong.
    const TODAY: Date = Date { year: 2026, month: 10, day: 30 };
    const WRONG: &str = "2020-01-01T00:00:00";

    fn reminder() -> ToolCall {
        ToolCall::CreateReminder { text: "x".into(), time: WRONG.into(), repeat: None }
    }

    fn meeting() -> ToolCall {
        ToolCall::ScheduleMeeting {
            title: "x".into(),
            time: WRONG.into(),
            attendees: vec!["Sara".into()],
            repeat: None,
        }
    }

    fn time_of(call: &ToolCall) -> &str {
        match call {
            ToolCall::CreateReminder { time, .. } | ToolCall::ScheduleMeeting { time, .. } => time,
            other => panic!("no time in {other:?}"),
        }
    }

    fn ready(text: &str, proposed: ToolCall) -> String {
        match resolve_call(text, TODAY, proposed) {
            Resolution::Ready(call) => time_of(&call).to_string(),
            other => panic!("{text}: {other:?}"),
        }
    }

    #[test]
    fn baseline_phrases_override_the_model() {
        assert_eq!(ready("Remind me to call the bank tomorrow at 10 am.", reminder()), "2026-10-31T10:00:00");
        assert_eq!(
            ready(
                "Schedule a meeting called Project Review with Sara and Omar on Sunday at 2 pm.",
                meeting()
            ),
            "2026-11-01T14:00:00"
        );
        assert_eq!(
            ready("ذكّرني بالاتصال بالبنك غدًا الساعة العاشرة صباحًا.", reminder()),
            "2026-10-31T10:00:00"
        );
        assert_eq!(
            ready("حدّد اجتماعًا باسم مراجعة المشروع مع سارة وعمر يوم الأحد الساعة الثانية ظهرًا.", meeting()),
            "2026-11-01T14:00:00"
        );
        assert_eq!(ready("عايز أفتكر أكلم المهندس بكرة الصبح.", reminder()), "2026-10-31T09:00:00");
    }

    #[test]
    fn mixed_phrase_with_bare_hour_asks_the_user() {
        let text = "ممكن تعمل schedule لـ meeting مع الـ team يوم الخميس الساعة 11؟";
        match resolve_call(text, TODAY, meeting()) {
            Resolution::NeedsClarification(call) => assert_eq!(time_of(&call), "2026-11-05T11:00:00"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn bare_hour_needs_clarification_and_keeps_the_candidate() {
        match resolve_call("الساعة 11", TODAY, reminder()) {
            Resolution::NeedsClarification(call) => assert_eq!(time_of(&call), "2026-10-30T11:00:00"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn no_time_is_missing() {
        assert_eq!(resolve_call("Remind me to call the bank", TODAY, reminder()), Resolution::MissingTime);
        assert_eq!(resolve_call("حدد اجتماعا مع سارة", TODAY, meeting()), Resolution::MissingTime);
    }

    #[test]
    fn weekly_repeat_is_kept() {
        let proposed = reminder();
        match resolve_call("ذكّرني أعمل backup للـ laptop كل جمعة.", TODAY, proposed) {
            Resolution::Ready(ToolCall::CreateReminder { time, repeat, .. }) => {
                assert_eq!(time, "2026-11-06");
                assert_eq!(repeat, Some(Repeat::Weekly));
            }
            other => panic!("{other:?}"),
        }
        match resolve_call("team sync every Friday at 3 pm", TODAY, meeting()) {
            Resolution::Ready(ToolCall::ScheduleMeeting { time, repeat, attendees, .. }) => {
                assert_eq!(time, "2026-11-06T15:00:00");
                assert_eq!(repeat, Some(Repeat::Weekly));
                assert_eq!(attendees, vec!["Sara".to_string()]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn other_tools_pass_through() {
        let calls = [
            ToolCall::OpenApp { name: "calculator".into() },
            ToolCall::ListFiles { path: "~/Documents".into() },
            ToolCall::SearchFiles { query: "budget 2026".into() },
            ToolCall::SetLanguage { lang: Language::Arabic },
            ToolCall::CurrentTime { timezone: None },
        ];
        for call in calls {
            // Text with a date in it must not matter for these tools.
            let out = resolve_call("tomorrow at 10 am", TODAY, call.clone());
            assert_eq!(out, Resolution::Ready(call));
        }
    }

    fn fixed() -> FixedClock {
        FixedClock(LocalTime { date: TODAY, hour: 9, minute: 5, second: 30 })
    }

    #[test]
    fn current_time_reads_the_clock() {
        let now = current_time(&fixed(), None).unwrap();
        assert_eq!(now.date, TODAY);
        assert_eq!((now.hour, now.minute, now.second), (9, 5, 30));
        assert_eq!(now.weekday_name(), "Friday");
        assert_eq!(current_time(&fixed(), Some("local")), Ok(now));
        assert_eq!(today(&fixed()), Ok(TODAY));
    }

    #[test]
    fn named_zone_is_an_error() {
        assert_eq!(
            current_time(&fixed(), Some("Asia/Tokyo")),
            Err(ClockError::UnsupportedTimeZone("Asia/Tokyo".into()))
        );
    }

    #[test]
    fn parses_date_output() {
        let t = parse_local_time("2026-10-30 09:05:30").unwrap();
        assert_eq!(t, LocalTime { date: TODAY, hour: 9, minute: 5, second: 30 });
        assert_eq!(parse_local_time("garbage"), None);
        assert_eq!(parse_local_time("2026-13-30 09:05:30"), None);
    }

    #[test]
    fn system_clock_reads_a_plausible_date() {
        if let Ok(now) = SystemClock.now() {
            assert!(now.date.year >= 2026);
        }
    }
}
