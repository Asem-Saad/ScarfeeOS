//! A store of reminders and meetings in one iCalendar (RFC 5545) file.
//!
//! The file may also be edited by other programs, so every line this module
//! does not understand is kept as it was. A file that cannot be read as
//! iCalendar is an error, and is never overwritten.

use std::collections::hash_map::RandomState;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::datetime::{Date, Repeat};
use crate::tools::ToolCall;

const PRODID: &str = "-//ScarfeeOS//Calendar//EN";
const FOLD_LIMIT: usize = 75;
/// How long a writer waits for another writer before giving up.
const LOCK_WAIT: Duration = Duration::from_secs(10);
/// A lock file older than this was left by a process that died. A writer holds
/// the lock for milliseconds.
const LOCK_STALE: Duration = Duration::from_secs(5);
const LOCK_POLL: Duration = Duration::from_millis(5);
/// Windows refuses to rename over or delete a file another thread has open.
/// That clears within milliseconds, so such an operation is retried briefly.
const BUSY_RETRIES: u32 = 100;
const BUSY_WAIT: Duration = Duration::from_millis(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalendarError {
    Io(String),
    /// The file is not valid iCalendar. It was left untouched.
    Malformed(String),
    /// The tool call is not a reminder or a meeting.
    NotAnEvent,
    InvalidTime(String),
    EmptyText,
}

impl fmt::Display for CalendarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CalendarError::Io(why) => write!(f, "calendar file error: {why}"),
            CalendarError::Malformed(why) => write!(f, "calendar file is not valid iCalendar: {why}"),
            CalendarError::NotAnEvent => write!(f, "tool call is not a reminder or a meeting"),
            CalendarError::InvalidTime(time) => write!(f, "invalid time: {time}"),
            CalendarError::EmptyText => write!(f, "event has no text"),
        }
    }
}

impl std::error::Error for CalendarError {}

impl From<io::Error> for CalendarError {
    fn from(e: io::Error) -> Self {
        CalendarError::Io(e.to_string())
    }
}

/// Seconds since 1970-01-01 UTC. DTSTAMP must be in UTC, which the local
/// `Clock` in `tools` cannot give.
pub trait UtcClock {
    fn unix_seconds(&self) -> u64;
}

pub struct SystemUtc;

impl UtcClock for SystemUtc {
    fn unix_seconds(&self) -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
    }
}

/// A clock that always reads the same time, for tests.
pub struct FixedUtc(pub u64);

impl UtcClock for FixedUtc {
    fn unix_seconds(&self) -> u64 {
        self.0
    }
}

/// Floating local time, or a whole day when the call gave no time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    Day(Date),
    Time { date: Date, hour: u32, minute: u32, second: u32 },
}

impl Start {
    pub fn date(&self) -> Date {
        match *self {
            Start::Day(date) | Start::Time { date, .. } => date,
        }
    }

    fn seconds_into_day(&self) -> u32 {
        match *self {
            Start::Day(_) => 0,
            Start::Time { hour, minute, second, .. } => hour * 3600 + minute * 60 + second,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub uid: String,
    pub summary: String,
    pub start: Start,
    pub attendees: Vec<String>,
    pub repeat: Option<Repeat>,
}

/// An event on the first day on or after a given date that it takes place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    pub date: Date,
    pub event: Event,
}

/// One line of the VCALENDAR body, or one whole component with its lines.
enum Item {
    Line(String),
    Component { lines: Vec<String>, uid: Option<String>, event: Option<Event> },
}

pub struct CalendarStore {
    path: PathBuf,
}

impl CalendarStore {
    pub fn new(path: impl Into<PathBuf>) -> CalendarStore {
        CalendarStore { path: path.into() }
    }

    /// Adds a reminder or a meeting and returns it with its new UID.
    pub fn add(&self, clock: &dyn UtcClock, call: &ToolCall) -> Result<Event, CalendarError> {
        let (summary, time, attendees, repeat) = match call {
            ToolCall::CreateReminder { text, time, repeat } => (text, time, &[][..], *repeat),
            ToolCall::ScheduleMeeting { title, time, attendees, repeat } => {
                (title, time, attendees.as_slice(), *repeat)
            }
            _ => return Err(CalendarError::NotAnEvent),
        };
        let summary = summary.trim();
        if summary.is_empty() {
            return Err(CalendarError::EmptyText);
        }
        let start = parse_iso(time)?;
        let attendees: Vec<String> =
            attendees.iter().map(|a| clean_param(a)).filter(|a| !a.is_empty()).collect();

        let _lock = Lock::acquire(&self.path)?;
        let mut items = self.load()?;
        let stamp = format_stamp(clock.unix_seconds());
        let uid = unique_uid(&stamp, &items);
        let event = Event { uid, summary: summary.to_string(), start, attendees, repeat };
        items.push(Item::Component {
            lines: event_lines(&event, &stamp),
            uid: Some(event.uid.clone()),
            event: Some(event.clone()),
        });
        self.save(&items)?;
        Ok(event)
    }

    /// Events that take place on or after `from`, soonest first. A repeating
    /// event appears once, at its next occurrence.
    pub fn list_upcoming(&self, from: Date) -> Result<Vec<Occurrence>, CalendarError> {
        let mut found: Vec<Occurrence> = self
            .load()?
            .into_iter()
            .filter_map(|item| match item {
                Item::Component { event: Some(event), .. } => {
                    next_occurrence(&event, from).map(|date| Occurrence { date, event })
                }
                _ => None,
            })
            .collect();
        found.sort_by(|a, b| {
            (date_key(a.date), a.event.start.seconds_into_day(), &a.event.uid).cmp(&(
                date_key(b.date),
                b.event.start.seconds_into_day(),
                &b.event.uid,
            ))
        });
        Ok(found)
    }

    /// Removes every event with this UID. Returns whether any was found.
    pub fn delete(&self, uid: &str) -> Result<bool, CalendarError> {
        let _lock = Lock::acquire(&self.path)?;
        let mut items = self.load()?;
        let before = items.len();
        items.retain(|item| !matches!(item, Item::Component { uid: Some(u), .. } if u == uid));
        if items.len() == before {
            return Ok(false);
        }
        self.save(&items)?;
        Ok(true)
    }

    /// A missing or empty file is an empty calendar.
    fn load(&self) -> Result<Vec<Item>, CalendarError> {
        match retry_if_busy(|| fs::read(&self.path)) {
            Ok(bytes) => parse_calendar(&bytes),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(fresh_items()),
            Err(e) => Err(e.into()),
        }
    }

    fn save(&self, items: &[Item]) -> Result<(), CalendarError> {
        let mut out = String::new();
        fold_into("BEGIN:VCALENDAR", &mut out);
        for item in items {
            match item {
                Item::Line(line) => fold_into(line, &mut out),
                Item::Component { lines, .. } => lines.iter().for_each(|line| fold_into(line, &mut out)),
            }
        }
        fold_into("END:VCALENDAR", &mut out);
        write_atomic(&self.path, out.as_bytes())
    }
}

fn fresh_items() -> Vec<Item> {
    vec![Item::Line("VERSION:2.0".into()), Item::Line(format!("PRODID:{PRODID}"))]
}

fn date_key(d: Date) -> (i32, u32, u32) {
    (d.year, d.month, d.day)
}

fn next_occurrence(event: &Event, from: Date) -> Option<Date> {
    let start = event.start.date();
    if date_key(start) >= date_key(from) {
        return Some(start);
    }
    match event.repeat? {
        Repeat::Daily => Some(from),
        Repeat::Weekly => {
            let ahead = (start.weekday() + 7 - from.weekday()) % 7;
            Some(from.add_days(i64::from(ahead)))
        }
    }
}

/// Parses `YYYY-MM-DD`, or `YYYY-MM-DDTHH:MM[:SS]`.
fn parse_iso(s: &str) -> Result<Start, CalendarError> {
    let invalid = || CalendarError::InvalidTime(s.to_string());
    let number = |part: Option<&str>| part?.parse::<u32>().ok();
    let (day_part, time_part) = match s.trim().split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (s.trim(), None),
    };
    let mut d = day_part.split('-');
    let (year, month, day) = (number(d.next()), number(d.next()), number(d.next()));
    let (Some(year), Some(month), Some(day), None) = (year, month, day, d.next()) else {
        return Err(invalid());
    };
    let date = Date::new(year as i32, month, day);
    if !(1..=12).contains(&month) || date.add_days(0) != date {
        return Err(invalid());
    }
    let Some(time_part) = time_part else {
        return Ok(Start::Day(date));
    };
    let mut t = time_part.split(':');
    let (hour, minute) = (number(t.next()), number(t.next()));
    let second = match t.next() {
        Some(sec) => number(Some(sec)),
        None => Some(0),
    };
    match (hour, minute, second, t.next()) {
        (Some(hour), Some(minute), Some(second), None) if hour < 24 && minute < 60 && second < 60 => {
            Ok(Start::Time { date, hour, minute, second })
        }
        _ => Err(invalid()),
    }
}

fn format_stamp(unix_seconds: u64) -> String {
    let date = Date::new(1970, 1, 1).add_days((unix_seconds / 86400) as i64);
    let rest = unix_seconds % 86400;
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        date.year,
        date.month,
        date.day,
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// 64 bits that differ between calls and between processes, without a crate.
fn random_u64() -> u64 {
    RandomState::new().build_hasher().finish()
}

fn unique_uid(stamp: &str, items: &[Item]) -> String {
    loop {
        let uid = format!("{stamp}-{:016x}@scarfeeos", random_u64());
        let taken = items.iter().any(|i| matches!(i, Item::Component { uid: Some(u), .. } if *u == uid));
        if !taken {
            return uid;
        }
    }
}

fn event_lines(event: &Event, stamp: &str) -> Vec<String> {
    let mut lines = vec![
        "BEGIN:VEVENT".to_string(),
        format!("UID:{}", event.uid),
        format!("DTSTAMP:{stamp}"),
    ];
    lines.push(match event.start {
        Start::Day(d) => format!("DTSTART;VALUE=DATE:{:04}{:02}{:02}", d.year, d.month, d.day),
        Start::Time { date: d, hour, minute, second } => {
            format!("DTSTART:{:04}{:02}{:02}T{hour:02}{minute:02}{second:02}", d.year, d.month, d.day)
        }
    });
    lines.push(format!("SUMMARY:{}", escape_text(&event.summary)));
    match event.repeat {
        Some(Repeat::Daily) => lines.push("RRULE:FREQ=DAILY".into()),
        Some(Repeat::Weekly) => lines.push("RRULE:FREQ=WEEKLY".into()),
        None => {}
    }
    for name in &event.attendees {
        lines.push(format!("ATTENDEE;CN=\"{name}\":invalid:nomail"));
    }
    lines.push("END:VEVENT".into());
    lines
}

fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push_str("\\n");
            }
            c if c.is_control() && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

fn unescape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// Parameter values cannot hold quotes or control characters.
fn clean_param(s: &str) -> String {
    let cleaned: String =
        s.chars().filter(|c| !c.is_control()).map(|c| if c == '"' { '\'' } else { c }).collect();
    cleaned.trim().to_string()
}

/// Appends `line` and CRLF, folding so no physical line exceeds 75 octets
/// and no UTF-8 character is split.
fn fold_into(line: &str, out: &mut String) {
    let mut width = 0;
    for c in line.chars() {
        let n = c.len_utf8();
        if width + n > FOLD_LIMIT {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += n;
    }
    out.push_str("\r\n");
}

struct ContentLine<'a> {
    name: String,
    params: Vec<(String, String)>,
    value: &'a str,
}

fn parse_line(line: &str) -> Option<ContentLine<'_>> {
    let mut quoted = false;
    let colon = line.char_indices().find_map(|(i, c)| {
        match c {
            '"' => quoted = !quoted,
            ':' if !quoted => return Some(i),
            _ => {}
        }
        None
    })?;
    let (head, value) = (&line[..colon], &line[colon + 1..]);
    let mut parts = Vec::new();
    let (mut start, mut quoted) = (0, false);
    for (i, c) in head.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ';' if !quoted => {
                parts.push(&head[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&head[start..]);
    let params = parts[1..]
        .iter()
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            Some((k.trim().to_ascii_uppercase(), v.trim().trim_matches('"').to_string()))
        })
        .collect();
    Some(ContentLine { name: parts[0].trim().to_ascii_uppercase(), params, value })
}

/// `Some((true, name))` for BEGIN, `Some((false, name))` for END.
fn marker(line: &str) -> Option<(bool, String)> {
    let parsed = parse_line(line)?;
    let name = parsed.value.trim().to_ascii_uppercase();
    match parsed.name.as_str() {
        "BEGIN" => Some((true, name)),
        "END" => Some((false, name)),
        _ => None,
    }
}

fn parse_calendar(bytes: &[u8]) -> Result<Vec<Item>, CalendarError> {
    let mut logical: Vec<Vec<u8>> = Vec::new();
    for raw in bytes.split(|&b| b == b'\n') {
        let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
        if matches!(raw.first(), Some(b' ' | b'\t')) && !logical.is_empty() {
            logical.last_mut().expect("checked above").extend_from_slice(&raw[1..]);
        } else if !raw.is_empty() {
            logical.push(raw.to_vec());
        }
    }
    let lines = logical
        .into_iter()
        .map(|l| String::from_utf8(l).map_err(|_| CalendarError::Malformed("not valid UTF-8".into())))
        .collect::<Result<Vec<_>, _>>()?;
    let mut lines = lines.into_iter().filter(|l| !l.trim().is_empty());

    let Some(first) = lines.next() else {
        return Ok(fresh_items());
    };
    if marker(&first) != Some((true, "VCALENDAR".into())) {
        return Err(CalendarError::Malformed("missing BEGIN:VCALENDAR".into()));
    }

    let mut items = Vec::new();
    let mut open: Vec<String> = Vec::new();
    let mut component: Vec<String> = Vec::new();
    let mut ended = false;
    for line in lines {
        if ended {
            return Err(CalendarError::Malformed("data after END:VCALENDAR".into()));
        }
        match marker(&line) {
            Some((true, name)) => {
                open.push(name);
                component.push(line);
            }
            Some((false, name)) => match open.pop() {
                Some(top) if top == name => {
                    component.push(line);
                    if open.is_empty() {
                        items.push(read_component(std::mem::take(&mut component)));
                    }
                }
                None if name == "VCALENDAR" => ended = true,
                _ => return Err(CalendarError::Malformed(format!("unexpected END:{name}"))),
            },
            None if open.is_empty() => items.push(Item::Line(line)),
            None => component.push(line),
        }
    }
    if !ended {
        return Err(CalendarError::Malformed("missing END:VCALENDAR".into()));
    }
    Ok(items)
}

fn read_component(lines: Vec<String>) -> Item {
    let is_event = marker(&lines[0]) == Some((true, "VEVENT".into()));
    let (uid, event) = if is_event { read_event(&lines) } else { (None, None) };
    Item::Component { lines, uid, event }
}

/// Reads the properties of a VEVENT. The event is None when it has no UID or
/// no start we can read; its lines are still kept.
fn read_event(lines: &[String]) -> (Option<String>, Option<Event>) {
    let (mut uid, mut summary, mut start, mut repeat) = (None, None, None, None);
    let mut attendees = Vec::new();
    let mut depth = 0;
    for line in &lines[1..lines.len() - 1] {
        match marker(line) {
            Some((true, _)) => depth += 1,
            Some((false, _)) => depth -= 1,
            None if depth > 0 => {}
            None => {
                let Some(prop) = parse_line(line) else { continue };
                match prop.name.as_str() {
                    "UID" if uid.is_none() => uid = Some(unescape_text(prop.value)),
                    "SUMMARY" if summary.is_none() => summary = Some(unescape_text(prop.value)),
                    "DTSTART" if start.is_none() => start = parse_ics_start(prop.value),
                    "RRULE" if repeat.is_none() => {
                        repeat = match prop.value.trim().to_ascii_uppercase().as_str() {
                            "FREQ=DAILY" => Some(Repeat::Daily),
                            "FREQ=WEEKLY" => Some(Repeat::Weekly),
                            _ => None,
                        }
                    }
                    "ATTENDEE" => {
                        let cn = prop.params.iter().find(|(k, _)| k == "CN").map(|(_, v)| v.clone());
                        let fallback = prop.value.strip_prefix("mailto:").unwrap_or(prop.value);
                        match cn {
                            Some(cn) if !cn.is_empty() => attendees.push(cn),
                            _ if fallback != "invalid:nomail" && !fallback.is_empty() => {
                                attendees.push(fallback.to_string())
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    let event = match (&uid, start) {
        (Some(uid), Some(start)) => Some(Event {
            uid: uid.clone(),
            summary: summary.unwrap_or_default(),
            start,
            attendees,
            repeat,
        }),
        _ => None,
    };
    (uid, event)
}

/// `YYYYMMDD`, or `YYYYMMDDTHHMMSS` with an optional `Z`. A time zone is not
/// converted: the clock time is read as written.
fn parse_ics_start(value: &str) -> Option<Start> {
    let v = value.trim();
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit()).then(|| s.parse::<u32>().ok()).flatten();
    let date = |v: &str| {
        let date = Date::new(digits(v.get(0..4)?)? as i32, digits(v.get(4..6)?)?, digits(v.get(6..8)?)?);
        ((1..=12).contains(&date.month) && date.add_days(0) == date).then_some(date)
    };
    match v.len() {
        8 => Some(Start::Day(date(v)?)),
        15 | 16 => {
            if v.len() == 16 && !v.ends_with('Z') {
                return None;
            }
            let (hour, minute, second) = (digits(v.get(9..11)?)?, digits(v.get(11..13)?)?, digits(v.get(13..15)?)?);
            if v.get(8..9)? != "T" || hour > 23 || minute > 59 || second > 60 {
                return None;
            }
            Some(Start::Time { date: date(v)?, hour, minute, second })
        }
        _ => None,
    }
}

/// Opens a new private file. On Unix it is created 0600, so it is never
/// readable by others, even for an instant.
fn create_new_private(path: &Path) -> io::Result<fs::File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

fn sibling(path: &Path, suffix: &str) -> Result<PathBuf, CalendarError> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| CalendarError::Io("path has no file name".into()))?
        .to_string_lossy();
    Ok(dir.join(format!(".{name}.{suffix}")))
}

/// An exclusive lock on the calendar, held from reading to renaming so that
/// two writers cannot both start from the same old contents. It is a file
/// created with `create_new`, and it is removed when the guard is dropped.
struct Lock {
    path: PathBuf,
    token: String,
}

impl Lock {
    fn acquire(store: &Path) -> Result<Lock, CalendarError> {
        let path = sibling(store, "lock")?;
        let token = format!("{}-{:016x}", std::process::id(), random_u64());
        let deadline = Instant::now() + LOCK_WAIT;
        loop {
            match create_new_private(&path) {
                Ok(mut file) => {
                    let written = file.write_all(token.as_bytes());
                    drop(file);
                    if let Err(e) = written {
                        let _ = fs::remove_file(&path);
                        return Err(e.into());
                    }
                    return Ok(Lock { path, token });
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists || is_busy(&e) => {
                    if Lock::break_if_stale(&path)? {
                        continue;
                    }
                    if Instant::now() >= deadline {
                        return Err(CalendarError::Io("calendar is locked by another writer".into()));
                    }
                    thread::sleep(LOCK_POLL);
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// Removes a lock left by a crashed process. Returns whether it did.
    ///
    /// Only the holder of a second file, the break guard, may remove a lock,
    /// and it re-reads the age once it holds the guard. A waiter that saw the
    /// old lock as stale cannot then remove the fresh lock that another waiter
    /// created after breaking it: by the time it holds the guard the lock is
    /// fresh. The guard is held for microseconds, and it too is removed when
    /// stale; two waiters removing a guard left by a crashed breaker is the
    /// one remaining race, and it needs a crash inside that window.
    fn break_if_stale(path: &Path) -> Result<bool, CalendarError> {
        if !is_stale(path) {
            return Ok(false);
        }
        let guard = sibling(path, "break")?;
        match create_new_private(&guard) {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists || is_busy(&e) => {
                if is_stale(&guard) {
                    let _ = retry_if_busy(|| fs::remove_file(&guard));
                }
                return Ok(false);
            }
            Err(e) => return Err(e.into()),
        }
        let broke = is_stale(path) && retry_if_busy(|| fs::remove_file(path)).is_ok();
        let _ = retry_if_busy(|| fs::remove_file(&guard));
        Ok(broke)
    }
}

/// Whether the file is old enough to have been left by a dead process. A
/// modification time in the future, after the clock was set back, counts as
/// stale. A missing file is not stale.
fn is_stale(path: &Path) -> bool {
    match fs::metadata(path).and_then(|m| m.modified()) {
        Ok(modified) => modified.elapsed().map_or(true, |age| age >= LOCK_STALE),
        Err(_) => false,
    }
}

/// Windows reports a file that another thread has open, or has just deleted,
/// as access denied or a sharing violation. Other systems never do, so
/// nothing is retried there.
fn is_busy(e: &io::Error) -> bool {
    cfg!(windows) && matches!(e.raw_os_error(), Some(5 | 32 | 33))
}

fn retry_if_busy<T>(mut op: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut retries = 0;
    loop {
        match op() {
            Err(e) if is_busy(&e) && retries < BUSY_RETRIES => {
                retries += 1;
                thread::sleep(BUSY_WAIT);
            }
            result => return result,
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        // Leave the file alone if it was broken as stale and is now another
        // writer's.
        if retry_if_busy(|| fs::read_to_string(&self.path)).is_ok_and(|t| t == self.token) {
            let _ = retry_if_busy(|| fs::remove_file(&self.path));
        }
    }
}

/// Writes to a temporary file in the same directory, then renames it over the
/// store, so a crash leaves either the old file or the new one. On Unix the
/// result is always 0600, which also tightens an older, wider file.
fn write_atomic(path: &Path, data: &[u8]) -> Result<(), CalendarError> {
    let tmp = sibling(path, &format!("{:016x}.tmp", random_u64()))?;
    let result = (|| -> io::Result<()> {
        let mut file = create_new_private(&tmp)?;
        file.write_all(data)?;
        file.sync_all()?;
        #[cfg(not(unix))]
        if let Ok(meta) = fs::metadata(path) {
            fs::set_permissions(&tmp, meta.permissions())?;
        }
        retry_if_busy(|| fs::rename(&tmp, path))
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// 2026-10-30 10:15:00 UTC.
    const NOW: FixedUtc = FixedUtc(1_793_355_300);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> TempDir {
            static COUNT: AtomicU32 = AtomicU32::new(0);
            let dir = std::env::temp_dir().join(format!(
                "scarfeeos-calendar-{}-{}",
                std::process::id(),
                COUNT.fetch_add(1, Ordering::SeqCst)
            ));
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn file(&self) -> PathBuf {
            self.0.join("calendar.ics")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn reminder(text: &str, time: &str, repeat: Option<Repeat>) -> ToolCall {
        ToolCall::CreateReminder { text: text.into(), time: time.into(), repeat }
    }

    fn meeting(title: &str, time: &str, attendees: &[&str]) -> ToolCall {
        ToolCall::ScheduleMeeting {
            title: title.into(),
            time: time.into(),
            attendees: attendees.iter().map(|a| a.to_string()).collect(),
            repeat: None,
        }
    }

    fn raw(dir: &TempDir) -> String {
        String::from_utf8(fs::read(dir.file()).unwrap()).unwrap()
    }

    const FROM: Date = Date { year: 2026, month: 11, day: 1 };

    #[test]
    fn stamp_formats_utc() {
        assert_eq!(format_stamp(NOW.0), "20261030T101500Z");
        assert_eq!(format_stamp(0), "19700101T000000Z");
    }

    #[test]
    fn arabic_round_trip() {
        let dir = TempDir::new();
        let store = CalendarStore::new(dir.file());
        let added = store
            .add(&NOW, &meeting("اجتماع مع الفريق؛ مراجعة، الميزانية", "2026-11-05T14:30:00", &["أحمد", "سارة"]))
            .unwrap();
        let listed = store.list_upcoming(FROM).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].event, added);
        assert_eq!(listed[0].event.summary, "اجتماع مع الفريق؛ مراجعة، الميزانية");
        assert_eq!(listed[0].event.attendees, vec!["أحمد", "سارة"]);
        assert_eq!(
            listed[0].event.start,
            Start::Time { date: Date::new(2026, 11, 5), hour: 14, minute: 30, second: 0 }
        );
    }

    #[test]
    fn file_is_valid_ics_shape() {
        let dir = TempDir::new();
        let store = CalendarStore::new(dir.file());
        let e = store.add(&NOW, &meeting("Review", "2026-11-05T14:30:00", &["Sara"])).unwrap();
        let text = raw(&dir);
        assert!(text.starts_with("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:"));
        assert!(text.ends_with("END:VEVENT\r\nEND:VCALENDAR\r\n"));
        assert!(!text.replace("\r\n", "").contains('\n'));
        assert!(text.contains(&format!("UID:{}\r\n", e.uid)));
        assert!(text.contains("DTSTAMP:20261030T101500Z\r\n"));
        assert!(text.contains("DTSTART:20261105T143000\r\n"));
        assert!(text.contains("SUMMARY:Review\r\n"));
        assert!(text.contains("ATTENDEE;CN=\"Sara\":"));
    }

    #[test]
    fn long_arabic_title_folds_at_character_boundaries() {
        let dir = TempDir::new();
        let store = CalendarStore::new(dir.file());
        let title = "اجتماع مراجعة خطة الإطلاق النهائية للمنتج الجديد مع جميع الفرق المعنية في الشركة ".repeat(3);
        let title = title.trim();
        store.add(&NOW, &reminder(title, "2026-11-05T09:00:00", None)).unwrap();

        let bytes = fs::read(dir.file()).unwrap();
        let mut folded = 0;
        for physical in bytes.split(|&b| b == b'\n') {
            let physical = physical.strip_suffix(b"\r").unwrap_or(physical);
            assert!(physical.len() <= 75, "line of {} octets", physical.len());
            assert!(std::str::from_utf8(physical).is_ok(), "a character was split");
            folded += usize::from(physical.first() == Some(&b' '));
        }
        assert!(folded >= 2);
        assert_eq!(store.list_upcoming(FROM).unwrap()[0].event.summary, title);
    }

    #[test]
    fn fold_limit_counts_octets_exactly() {
        let mut out = String::new();
        fold_into(&format!("SUMMARY:{}", "a".repeat(200)), &mut out);
        let lines: Vec<&str> = out.split("\r\n").collect();
        assert_eq!(lines[0].len(), 75);
        assert_eq!(lines[1].len(), 75);
        assert!(lines[1].starts_with(' '));
        let mut short = String::new();
        fold_into(&"b".repeat(75), &mut short);
        assert_eq!(short.matches("\r\n").count(), 1);
    }

    #[test]
    fn text_is_escaped_and_restored() {
        let dir = TempDir::new();
        let store = CalendarStore::new(dir.file());
        let text = "a, b; c\\d\nnew line";
        store.add(&NOW, &reminder(text, "2026-11-05", None)).unwrap();
        assert!(raw(&dir).contains("SUMMARY:a\\, b\\; c\\\\d\\nnew line\r\n"));
        assert_eq!(store.list_upcoming(FROM).unwrap()[0].event.summary, text);
    }

    #[test]
    fn attendee_names_cannot_break_the_parameter() {
        let dir = TempDir::new();
        let store = CalendarStore::new(dir.file());
        store.add(&NOW, &meeting("Sync", "2026-11-05T10:00", &["Lee \"Bo\": x; y", "  ", "نورة"])).unwrap();
        let e = &store.list_upcoming(FROM).unwrap()[0].event;
        assert_eq!(e.attendees, vec!["Lee 'Bo': x; y", "نورة"]);
    }

    #[test]
    fn repeats_are_written_and_read() {
        let dir = TempDir::new();
        let store = CalendarStore::new(dir.file());
        store.add(&NOW, &reminder("daily", "2026-11-02T08:00:00", Some(Repeat::Daily))).unwrap();
        store.add(&NOW, &reminder("weekly", "2026-11-06T08:00:00", Some(Repeat::Weekly))).unwrap();
        store.add(&NOW, &reminder("once", "2026-11-03T08:00:00", None)).unwrap();
        let text = raw(&dir);
        assert!(text.contains("RRULE:FREQ=DAILY\r\n"));
        assert!(text.contains("RRULE:FREQ=WEEKLY\r\n"));
        assert_eq!(text.matches("RRULE").count(), 2);

        let listed = store.list_upcoming(FROM).unwrap();
        let by = |s: &str| listed.iter().find(|o| o.event.summary == s).unwrap();
        assert_eq!(by("daily").event.repeat, Some(Repeat::Daily));
        assert_eq!(by("weekly").event.repeat, Some(Repeat::Weekly));
        assert_eq!(by("once").event.repeat, None);
    }

    #[test]
    fn repeating_events_list_at_their_next_occurrence() {
        let dir = TempDir::new();
        let store = CalendarStore::new(dir.file());
        // 2026-10-02 is a Friday; 2026-11-01 is a Sunday.
        store.add(&NOW, &reminder("weekly", "2026-10-02T08:00:00", Some(Repeat::Weekly))).unwrap();
        store.add(&NOW, &reminder("daily", "2026-10-02T09:00:00", Some(Repeat::Daily))).unwrap();
        store.add(&NOW, &reminder("past", "2026-10-02T09:00:00", None)).unwrap();
        let listed = store.list_upcoming(FROM).unwrap();
        let summaries: Vec<_> = listed.iter().map(|o| o.event.summary.as_str()).collect();
        assert_eq!(summaries, ["daily", "weekly"]);
        assert_eq!(listed[0].date, Date::new(2026, 11, 1));
        assert_eq!(listed[1].date, Date::new(2026, 11, 6));
        assert_eq!(listed[1].date.weekday(), 5);
    }

    #[test]
    fn date_only_events_use_a_date_value() {
        let dir = TempDir::new();
        let store = CalendarStore::new(dir.file());
        store.add(&NOW, &reminder("تسليم التقرير", "2026-11-05", None)).unwrap();
        let text = raw(&dir);
        assert!(text.contains("DTSTART;VALUE=DATE:20261105\r\n"));
        let e = &store.list_upcoming(FROM).unwrap()[0].event;
        assert_eq!(e.start, Start::Day(Date::new(2026, 11, 5)));
    }

    #[test]
    fn upcoming_is_sorted_and_starts_at_the_given_date() {
        let dir = TempDir::new();
        let store = CalendarStore::new(dir.file());
        store.add(&NOW, &reminder("late", "2026-11-09T09:00:00", None)).unwrap();
        store.add(&NOW, &reminder("afternoon", "2026-11-05T15:00:00", None)).unwrap();
        store.add(&NOW, &reminder("morning", "2026-11-05T08:00:00", None)).unwrap();
        store.add(&NOW, &reminder("whole day", "2026-11-05", None)).unwrap();
        store.add(&NOW, &reminder("before", "2026-10-31T08:00:00", None)).unwrap();
        let names = |from| -> Vec<String> {
            store.list_upcoming(from).unwrap().into_iter().map(|o| o.event.summary).collect()
        };
        assert_eq!(names(FROM), ["whole day", "morning", "afternoon", "late"]);
        assert_eq!(names(Date::new(2026, 11, 5)), ["whole day", "morning", "afternoon", "late"]);
        assert_eq!(names(Date::new(2026, 11, 6)), ["late"]);
    }

    #[test]
    fn unknown_lines_survive_a_rewrite() {
        let dir = TempDir::new();
        let existing = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Other//App//EN\r\nX-WR-CALNAME:Work\r\n\
            BEGIN:VTIMEZONE\r\nTZID:Asia/Riyadh\r\nEND:VTIMEZONE\r\n\
            BEGIN:VEVENT\r\nUID:theirs-1\r\nDTSTAMP:20260101T000000Z\r\nDTSTART;TZID=Asia/Riyadh:20261110T090000\r\n\
            SUMMARY:Their event\r\nX-CUSTOM;FOO=bar:baz\r\nCATEGORIES:Work\r\n\
            BEGIN:VALARM\r\nTRIGGER:-PT15M\r\nACTION:DISPLAY\r\nEND:VALARM\r\nEND:VEVENT\r\n\
            BEGIN:VTODO\r\nUID:todo-1\r\nSUMMARY:A task\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
        fs::write(dir.file(), existing).unwrap();
        let store = CalendarStore::new(dir.file());
        assert_eq!(store.list_upcoming(FROM).unwrap()[0].event.summary, "Their event");

        let added = store.add(&NOW, &reminder("mine", "2026-11-05", None)).unwrap();
        let text = raw(&dir);
        let kept = existing.strip_suffix("END:VCALENDAR\r\n").unwrap();
        assert!(text.starts_with(kept), "existing content changed:\n{text}");
        assert!(text.contains(&added.uid));

        assert!(store.delete(&added.uid).unwrap());
        assert_eq!(raw(&dir), existing);
    }

    #[test]
    fn lines_folded_by_other_programs_are_read() {
        let dir = TempDir::new();
        let existing = "BEGIN:VCALENDAR\nVERSION:2.0\nBEGIN:VEVENT\nUID:x\nDTSTART:20261110T090000Z\n\
            SUMMARY:Hello\n \tWorld\nEND:VEVENT\nEND:VCALENDAR\n";
        fs::write(dir.file(), existing).unwrap();
        let store = CalendarStore::new(dir.file());
        let e = &store.list_upcoming(FROM).unwrap()[0].event;
        assert_eq!(e.summary, "Hello\tWorld");
    }

    #[test]
    fn deleting_removes_only_that_event() {
        let dir = TempDir::new();
        let store = CalendarStore::new(dir.file());
        let a = store.add(&NOW, &reminder("a", "2026-11-05", None)).unwrap();
        let b = store.add(&NOW, &reminder("b", "2026-11-06", None)).unwrap();
        assert!(store.delete(&a.uid).unwrap());
        let left = store.list_upcoming(FROM).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].event.uid, b.uid);
        assert!(!store.delete(&a.uid).unwrap());
        assert!(!store.delete("no-such-uid").unwrap());
        assert!(store.delete(&b.uid).unwrap());
        assert!(store.list_upcoming(FROM).unwrap().is_empty());
        assert!(raw(&dir).starts_with("BEGIN:VCALENDAR\r\n"));
    }

    #[test]
    fn missing_file_is_empty_and_is_not_created_by_reading() {
        let dir = TempDir::new();
        let store = CalendarStore::new(dir.file());
        assert!(store.list_upcoming(FROM).unwrap().is_empty());
        assert!(!store.delete("x").unwrap());
        assert!(!dir.file().exists());
        store.add(&NOW, &reminder("first", "2026-11-05", None)).unwrap();
        assert!(dir.file().exists());
    }

    #[test]
    fn empty_file_is_an_empty_calendar() {
        for content in ["", "\r\n\r\n"] {
            let dir = TempDir::new();
            fs::write(dir.file(), content).unwrap();
            let store = CalendarStore::new(dir.file());
            assert!(store.list_upcoming(FROM).unwrap().is_empty());
            store.add(&NOW, &reminder("first", "2026-11-05", None)).unwrap();
            assert_eq!(store.list_upcoming(FROM).unwrap().len(), 1);
            assert!(raw(&dir).starts_with("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n"));
        }
    }

    #[test]
    fn malformed_files_give_an_error_and_are_left_alone() {
        let cases: [&[u8]; 6] = [
            b"this is not a calendar\r\n",
            b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:x\r\n",
            b"BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:x\r\nEND:VTODO\r\nEND:VCALENDAR\r\n",
            b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\n",
            b"BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\nstray line\r\n",
            b"BEGIN:VCALENDAR\r\nSUMMARY:\xff\xfe\r\nEND:VCALENDAR\r\n",
        ];
        for content in cases {
            let dir = TempDir::new();
            fs::write(dir.file(), content).unwrap();
            let store = CalendarStore::new(dir.file());
            assert!(matches!(store.list_upcoming(FROM), Err(CalendarError::Malformed(_))), "{content:?}");
            assert!(matches!(store.add(&NOW, &reminder("x", "2026-11-05", None)), Err(CalendarError::Malformed(_))));
            assert!(matches!(store.delete("x"), Err(CalendarError::Malformed(_))));
            assert_eq!(fs::read(dir.file()).unwrap(), content, "file was modified");
        }
    }

    #[test]
    fn write_leaves_no_temporary_files() {
        let dir = TempDir::new();
        let store = CalendarStore::new(dir.file());
        let a = store.add(&NOW, &reminder("a", "2026-11-05", None)).unwrap();
        store.delete(&a.uid).unwrap();
        let names: Vec<_> = fs::read_dir(&dir.0).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["calendar.ics"]);
    }

    #[test]
    fn failed_write_keeps_the_old_file_and_cleans_up() {
        let dir = TempDir::new();
        let store = CalendarStore::new(dir.file());
        store.add(&NOW, &reminder("a", "2026-11-05", None)).unwrap();
        let before = fs::read(dir.file()).unwrap();
        // A directory in the way makes the final rename fail.
        let blocked = TempDir::new();
        fs::create_dir(blocked.file()).unwrap();
        let result = CalendarStore::new(blocked.file()).save(&fresh_items());
        assert!(matches!(result, Err(CalendarError::Io(_))));
        assert_eq!(fs::read_dir(&blocked.0).unwrap().count(), 1);
        assert_eq!(fs::read(dir.file()).unwrap(), before);
    }

    #[test]
    fn uids_are_unique_even_with_a_frozen_clock() {
        let dir = TempDir::new();
        let store = CalendarStore::new(dir.file());
        let mut uids: Vec<String> =
            (0..50).map(|_| store.add(&NOW, &reminder("x", "2026-11-05", None)).unwrap().uid).collect();
        uids.sort();
        uids.dedup();
        assert_eq!(uids.len(), 50);
        assert!(uids[0].starts_with("20261030T101500Z-") && uids[0].ends_with("@scarfeeos"));
    }

    #[test]
    fn bad_calls_are_rejected_before_touching_the_file() {
        let dir = TempDir::new();
        let store = CalendarStore::new(dir.file());
        assert_eq!(store.add(&NOW, &ToolCall::OpenApp { name: "x".into() }), Err(CalendarError::NotAnEvent));
        assert_eq!(store.add(&NOW, &reminder("  ", "2026-11-05", None)), Err(CalendarError::EmptyText));
        for time in ["", "tomorrow", "2026-02-30", "2026-13-01", "2026-11-05T25:00:00", "2026-11-05T10:00:00:00"] {
            assert!(
                matches!(store.add(&NOW, &reminder("x", time, None)), Err(CalendarError::InvalidTime(_))),
                "{time}"
            );
        }
        assert!(!dir.file().exists());
    }

    #[test]
    fn concurrent_writers_lose_no_events() {
        let dir = TempDir::new();
        let path = dir.file();
        let handles: Vec<_> = (0..8)
            .map(|t| {
                let path = path.clone();
                thread::spawn(move || {
                    let store = CalendarStore::new(path);
                    (0..10)
                        .map(|i| store.add(&NOW, &reminder(&format!("t{t}-{i}"), "2026-11-05", None)).unwrap().uid)
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        let mut uids: Vec<String> = handles.into_iter().flat_map(|h| h.join().unwrap()).collect();
        let stored = CalendarStore::new(&path).list_upcoming(FROM).unwrap();
        assert_eq!(stored.len(), 80);
        let mut stored_uids: Vec<String> = stored.into_iter().map(|o| o.event.uid).collect();
        uids.sort();
        stored_uids.sort();
        assert_eq!(stored_uids, uids);
        let names: Vec<_> = fs::read_dir(&dir.0).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["calendar.ics"], "lock or temporary file left behind");
    }

    #[test]
    fn concurrent_adds_and_deletes_do_not_resurrect_or_lose_events() {
        let dir = TempDir::new();
        let path = dir.file();
        let store = CalendarStore::new(&path);
        let doomed: Vec<String> =
            (0..20).map(|i| store.add(&NOW, &reminder(&format!("d{i}"), "2026-11-05", None)).unwrap().uid).collect();
        let deleter = {
            let path = path.clone();
            thread::spawn(move || {
                let store = CalendarStore::new(path);
                for uid in &doomed {
                    assert!(store.delete(uid).unwrap());
                }
            })
        };
        let adder = {
            let path = path.clone();
            thread::spawn(move || {
                let store = CalendarStore::new(path);
                (0..20).map(|i| store.add(&NOW, &reminder(&format!("k{i}"), "2026-11-06", None)).unwrap().uid).collect::<Vec<_>>()
            })
        };
        deleter.join().unwrap();
        let mut kept = adder.join().unwrap();
        let mut left: Vec<String> = store.list_upcoming(FROM).unwrap().into_iter().map(|o| o.event.uid).collect();
        kept.sort();
        left.sort();
        assert_eq!(left, kept);
    }

    #[test]
    fn a_stale_lock_from_a_crashed_process_is_broken() {
        let dir = TempDir::new();
        let store = CalendarStore::new(dir.file());
        let lock = sibling(&dir.file(), "lock").unwrap();
        let file = fs::File::create(&lock).unwrap();
        file.set_modified(SystemTime::now() - LOCK_STALE - Duration::from_secs(1)).unwrap();
        drop(file);
        store.add(&NOW, &reminder("a", "2026-11-05", None)).unwrap();
        assert!(!lock.exists());
        assert_eq!(store.list_upcoming(FROM).unwrap().len(), 1);
    }

    #[test]
    fn a_lock_dated_in_the_future_is_broken() {
        let dir = TempDir::new();
        let store = CalendarStore::new(dir.file());
        let lock = sibling(&dir.file(), "lock").unwrap();
        let file = fs::File::create(&lock).unwrap();
        file.set_modified(SystemTime::now() + Duration::from_secs(3600)).unwrap();
        drop(file);
        store.add(&NOW, &reminder("a", "2026-11-05", None)).unwrap();
        assert!(!lock.exists());
        assert_eq!(store.list_upcoming(FROM).unwrap().len(), 1);
    }

    #[test]
    fn a_waiter_that_saw_a_stale_lock_cannot_break_the_fresh_one_that_replaced_it() {
        let dir = TempDir::new();
        let path = dir.file();
        let lock = sibling(&path, "lock").unwrap();
        let file = fs::File::create(&lock).unwrap();
        file.set_modified(SystemTime::now() - LOCK_STALE - Duration::from_secs(1)).unwrap();
        drop(file);
        // Waiter X saw the stale lock. Waiter Z breaks it and takes the lock
        // before X gets as far as removing anything.
        assert!(is_stale(&lock));
        let z = Lock::acquire(&path).unwrap();
        assert!(!Lock::break_if_stale(&lock).unwrap());
        assert_eq!(fs::read_to_string(&lock).unwrap(), z.token);
        // Waiter W still cannot take it.
        let w = {
            let path = path.clone();
            thread::spawn(move || Lock::acquire(&path).map(drop))
        };
        thread::sleep(Duration::from_millis(100));
        assert_eq!(fs::read_to_string(&lock).unwrap(), z.token);
        drop(z);
        w.join().unwrap().unwrap();
    }

    #[test]
    fn two_writers_never_hold_the_lock_together_while_breaking_a_stale_one() {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;
        for _ in 0..20 {
            let dir = TempDir::new();
            let path = dir.file();
            let lock = sibling(&path, "lock").unwrap();
            let file = fs::File::create(&lock).unwrap();
            file.set_modified(SystemTime::now() - LOCK_STALE - Duration::from_secs(1)).unwrap();
            drop(file);
            let inside = Arc::new(AtomicBool::new(false));
            let handles: Vec<_> = (0..6)
                .map(|_| {
                    let (path, inside) = (path.clone(), inside.clone());
                    thread::spawn(move || {
                        let held = Lock::acquire(&path).unwrap();
                        assert!(!inside.swap(true, Ordering::SeqCst), "two writers hold the lock");
                        thread::sleep(Duration::from_millis(2));
                        inside.store(false, Ordering::SeqCst);
                        drop(held);
                    })
                })
                .collect();
            for h in handles {
                h.join().unwrap();
            }
        }
    }

    #[test]
    fn a_fresh_lock_blocks_until_released() {
        let dir = TempDir::new();
        let path = dir.file();
        let held = Lock::acquire(&path).unwrap();
        let writer = {
            let path = path.clone();
            thread::spawn(move || CalendarStore::new(path).add(&NOW, &reminder("a", "2026-11-05", None)).unwrap())
        };
        thread::sleep(Duration::from_millis(100));
        assert!(!path.exists(), "writer got past a held lock");
        drop(held);
        writer.join().unwrap();
        assert_eq!(CalendarStore::new(&path).list_upcoming(FROM).unwrap().len(), 1);
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[cfg(unix)]
    #[test]
    fn new_calendar_file_is_private() {
        let dir = TempDir::new();
        let store = CalendarStore::new(dir.file());
        store.add(&NOW, &reminder("a", "2026-11-05", None)).unwrap();
        assert_eq!(mode(&dir.file()), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn existing_wide_file_is_tightened_on_the_next_write() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new();
        let store = CalendarStore::new(dir.file());
        let a = store.add(&NOW, &reminder("a", "2026-11-05", None)).unwrap();
        fs::set_permissions(dir.file(), fs::Permissions::from_mode(0o644)).unwrap();
        store.add(&NOW, &reminder("b", "2026-11-06", None)).unwrap();
        assert_eq!(mode(&dir.file()), 0o600);
        fs::set_permissions(dir.file(), fs::Permissions::from_mode(0o666)).unwrap();
        assert!(store.delete(&a.uid).unwrap());
        assert_eq!(mode(&dir.file()), 0o600);
    }
}
