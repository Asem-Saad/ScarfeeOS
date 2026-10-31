//! Runs the tools the assistant proposed, after the shell has asked the user
//! where that is needed. A call that has to wait is held here under a random,
//! one-time action id. Only that id comes back from the shell, never a call,
//! so text from the model cannot name what runs.
//!
//! The only program ever started is the one in the argument list of an app
//! from the system's own app table, and it is started without a shell. Nothing
//! here deletes, moves or overwrites a user file, except the calendar store.

use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::fs::DirBuilder;
#[cfg(unix)]
use std::os::unix::fs::DirBuilderExt;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

use crate::actions::{
    self, ActionError, App, AppTable, EntryKind, FileEntry, FileSandbox, FilesConfig, LaunchError, Launcher,
};
use crate::calendar::{CalendarError, CalendarStore, Event, Start, SystemUtc, UtcClock};
use crate::datetime::{Date, Repeat};
use crate::tools::{Clock, SystemClock, ToolCall};

/// Held calls waiting for an answer. The oldest is dropped past this.
const MAX_HELD: usize = 32;

/// Why a held call is waiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Held {
    /// The time could mean two things. The user may pick another.
    Clarification,
    /// The call changes a setting. The user must say yes.
    Confirmation,
}

struct Pending {
    id: String,
    kind: Held,
    call: ToolCall,
}

pub struct Executor {
    files: FileSandbox,
    /// The real folder behind the file root, or `None` when it cannot be read.
    root: Option<PathBuf>,
    apps: AppTable,
    launcher: Box<dyn Launcher>,
    calendar: CalendarStore,
    calendar_dir: Option<PathBuf>,
    utc: Box<dyn UtcClock>,
    clock: Box<dyn Clock>,
    pending: VecDeque<Pending>,
}

impl Executor {
    pub fn new(
        files: FileSandbox,
        apps: AppTable,
        launcher: Box<dyn Launcher>,
        calendar_file: PathBuf,
        utc: Box<dyn UtcClock>,
        clock: Box<dyn Clock>,
    ) -> Self {
        let calendar_dir = calendar_file.parent().filter(|d| !d.as_os_str().is_empty()).map(Path::to_path_buf);
        let calendar = CalendarStore::new(calendar_file);
        let root = files.list_files("").ok().map(|listing| listing.path);
        Executor { files, root, apps, launcher, calendar, calendar_dir, utc, clock, pending: VecDeque::new() }
    }

    /// The real system: the user's home folder as the file root, the installed
    /// apps, and `scarfeeos/calendar.ics` in the user's data directory.
    pub fn from_env() -> Result<Self, String> {
        let home = home_dir().ok_or("HOME is not set")?;
        let config_dir = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|c| c.is_absolute())
            .unwrap_or_else(|| home.join(".config"));
        let files = FileSandbox::new(FilesConfig::for_home(&home, &config_dir)).map_err(|e| e.to_string())?;
        let paths = SystemPaths::from_vars(
            &home,
            std::env::var_os("XDG_DATA_HOME").as_deref(),
            std::env::var_os("XDG_DATA_DIRS").as_deref(),
        );
        Ok(Executor::new(
            files,
            AppTable::load(&paths.app_dirs),
            Box::new(CommandLauncher),
            paths.calendar,
            Box::new(SystemUtc),
            Box::new(SystemClock),
        ))
    }

    /// Runs a call now and describes what happened. The description always has
    /// `ok`; on success it has a `type`, on failure an `error` with a `code`
    /// and a `message`.
    pub fn run(&self, call: &ToolCall) -> Value {
        match self.try_run(call) {
            Ok(mut result) => {
                result["ok"] = json!(true);
                result
            }
            Err(failure) => json!({"ok": false, "error": failure.to_json()}),
        }
    }

    /// Keeps `call` until `confirm` or `cancel` names the id this returns.
    pub fn hold(&mut self, kind: Held, call: ToolCall) -> Result<String, String> {
        let id = random_id()?;
        if self.pending.len() >= MAX_HELD {
            self.pending.pop_front();
        }
        self.pending.push_back(Pending { id: id.clone(), kind, call });
        Ok(id)
    }

    /// Runs the call held under `id`, once. For a clarification, `time` may
    /// replace the time the assistant guessed. A request that is refused leaves
    /// the call held, so the user can answer again.
    pub fn confirm(&mut self, id: &str, time: Option<&str>) -> Result<(ToolCall, Value), String> {
        let at = self.position(id)?;
        let held = &self.pending[at];
        let call = match (held.kind, time) {
            (_, None) => held.call.clone(),
            (Held::Confirmation, Some(_)) => return Err("a time can only be chosen when the time was unclear".into()),
            (Held::Clarification, Some(time)) => {
                let time = time.trim();
                if !is_iso_8601(time) {
                    return Err(format!("invalid time `{time}`: use YYYY-MM-DD or YYYY-MM-DDTHH:MM[:SS], local time"));
                }
                with_time(&held.call, time)
            }
        };
        self.pending.remove(at);
        let result = self.run(&call);
        Ok((call, result))
    }

    /// Drops the call held under `id` without running it.
    pub fn cancel(&mut self, id: &str) -> Result<(), String> {
        let at = self.position(id)?;
        self.pending.remove(at);
        Ok(())
    }

    /// Events on or after `from` (a date, default today), soonest first.
    pub fn upcoming(&self, from: Option<&str>) -> Result<Value, String> {
        let from = match from {
            Some(text) => parse_date(text.trim()).ok_or_else(|| format!("invalid date `{text}`: use YYYY-MM-DD"))?,
            None => self.clock.now().map_err(|e| e.to_string())?.date,
        };
        let events: Vec<Value> = self
            .calendar
            .list_upcoming(from)
            .map_err(|e| e.to_string())?
            .iter()
            .map(|o| {
                let mut event = event_json(&o.event);
                event["date"] = json!(format_date(o.date));
                event
            })
            .collect();
        Ok(json!({"from": format_date(from), "events": events}))
    }

    /// Removes the event with this UID from the calendar store.
    pub fn delete_event(&self, uid: &str) -> Result<(), String> {
        match self.calendar.delete(uid) {
            Ok(true) => Ok(()),
            Ok(false) => Err("no event with that id".into()),
            Err(e) => Err(e.to_string()),
        }
    }

    fn position(&self, id: &str) -> Result<usize, String> {
        self.pending.iter().position(|p| p.id == id).ok_or_else(|| "unknown or already used action id".to_string())
    }

    fn try_run(&self, call: &ToolCall) -> Result<Value, Failure> {
        match call {
            ToolCall::CreateReminder { .. } | ToolCall::ScheduleMeeting { .. } => {
                if let Some(dir) = &self.calendar_dir {
                    let mut builder = DirBuilder::new();
                    builder.recursive(true);
                    #[cfg(unix)]
                    builder.mode(0o700);
                    builder.create(dir).map_err(|e| CalendarError::Io(e.to_string()))?;
                }
                let event = self.calendar.add(&*self.utc, call)?;
                Ok(json!({"type": "event", "event": event_json(&event)}))
            }
            ToolCall::SetLanguage { lang } => Ok(json!({"type": "language", "lang": lang.code()})),
            ToolCall::CurrentTime { .. } => Err(ActionError::Unsupported.into()),
            ToolCall::ListFiles { .. } | ToolCall::SearchFiles { .. } | ToolCall::OpenApp { .. } => {
                if let ToolCall::ListFiles { path } = call {
                    let root = self.root.as_deref().ok_or_else(|| {
                        Failure::new("unreadable", "cannot read folder: the file root is not readable".into())
                    })?;
                    if names_hidden_part(path, root) {
                        return Err(hidden_path());
                    }
                }
                Ok(match actions::execute(call, &self.files, &self.apps, &*self.launcher)? {
                    actions::Outcome::Listing(l) => {
                        // A symlink to a hidden folder is only seen here, once it is resolved.
                        let inside = self.root.as_deref().and_then(|root| l.path.strip_prefix(root).ok());
                        if inside.map_or(true, has_hidden_component) {
                            return Err(hidden_path());
                        }
                        json!({
                            "type": "listing",
                            "path": path_text(&l.path),
                            "display_path": display_text(&path_text(&l.path)),
                            "entries": entries_json(&l.entries),
                            "truncated": l.truncated,
                        })
                    }
                    actions::Outcome::Search(s) => json!({
                        "type": "search",
                        "matches": entries_json(&s.matches),
                        "truncated": s.truncated,
                        "unreadable": s.unreadable,
                    }),
                    actions::Outcome::Opened(app) => json!({"type": "opened", "app": {"id": app.id, "name": app.name}}),
                })
            }
        }
    }
}

struct Failure {
    code: &'static str,
    message: String,
    /// Similar app names, when an app was not found.
    closest: Vec<String>,
}

impl Failure {
    fn new(code: &'static str, message: String) -> Self {
        Failure { code, message, closest: Vec::new() }
    }

    fn to_json(&self) -> Value {
        let mut error = json!({"code": self.code, "message": self.message});
        if !self.closest.is_empty() {
            error["closest"] = json!(self.closest);
        }
        error
    }
}

impl From<ActionError> for Failure {
    fn from(e: ActionError) -> Self {
        use crate::actions::{FileError, OpenError};
        let code = match &e {
            ActionError::File(FileError::Root(_)) => "root_unusable",
            ActionError::File(FileError::NotFound(_)) => "not_found",
            ActionError::File(FileError::OutsideRoot) => "outside_root",
            ActionError::File(FileError::NotADirectory) => "not_a_directory",
            ActionError::File(FileError::Unreadable(_)) => "unreadable",
            ActionError::File(FileError::EmptyQuery) => "empty_query",
            ActionError::Open(OpenError::EmptyName) => "empty_name",
            ActionError::Open(OpenError::NotFound { .. }) => "app_not_found",
            ActionError::Open(OpenError::Launch(_)) => "launch_failed",
            ActionError::Unsupported => "unsupported",
        };
        let closest = match &e {
            ActionError::Open(OpenError::NotFound { closest }) => closest.clone(),
            _ => Vec::new(),
        };
        Failure { code, message: e.to_string(), closest }
    }
}

impl From<CalendarError> for Failure {
    fn from(e: CalendarError) -> Self {
        let code = match &e {
            CalendarError::Io(_) => "calendar_io",
            CalendarError::Malformed(_) => "calendar_malformed",
            CalendarError::NotAnEvent => "not_an_event",
            CalendarError::InvalidTime(_) => "invalid_time",
            CalendarError::EmptyText => "empty_text",
        };
        Failure { code, message: e.to_string(), closest: Vec::new() }
    }
}

/// Starts an app by running its argument list with `Command`. No shell is
/// involved, so nothing in the list is split, expanded or interpreted again.
/// The app gets no terminal and, on Unix, its own process group, and cannot
/// write to the channel the shell reads from.
pub struct CommandLauncher;

impl Launcher for CommandLauncher {
    fn launch(&self, app: &App) -> Result<(), LaunchError> {
        if app.terminal {
            return Err(LaunchError("the app needs a terminal, which is not supported yet".into()));
        }
        let (program, args) = app.exec.split_first().ok_or_else(|| LaunchError("the app has no command".into()))?;
        let mut command = Command::new(program);
        command.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn().map_err(|e| LaunchError(format!("{program}: {e}")))?;
        // Reaps the app when it quits, so it does not stay as a zombie.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
}

/// Where the app table and the calendar come from on this system.
#[derive(Debug, PartialEq, Eq)]
pub struct SystemPaths {
    /// The user's folder first, so a user's copy of an app wins.
    pub app_dirs: Vec<PathBuf>,
    pub calendar: PathBuf,
}

impl SystemPaths {
    /// Only absolute paths are used from the two XDG variables, as the XDG
    /// specification says.
    pub fn from_vars(home: &Path, data_home: Option<&OsStr>, data_dirs: Option<&OsStr>) -> Self {
        let absolute = |v: &OsStr| Some(PathBuf::from(v)).filter(|p| p.is_absolute());
        let data_home = data_home.and_then(absolute).unwrap_or_else(|| home.join(".local/share"));
        let default_dirs = OsString::from("/usr/local/share:/usr/share");
        let dirs = data_dirs.filter(|v| !v.is_empty()).unwrap_or(&default_dirs);
        let app_dirs = std::iter::once(data_home.clone())
            .chain(std::env::split_paths(dirs).filter(|p| p.is_absolute()))
            .map(|d| d.join("applications"))
            .collect();
        SystemPaths { app_dirs, calendar: data_home.join("scarfeeos/calendar.ics") }
    }
}

/// The user's home folder: `HOME`, and on Windows `USERPROFILE` when `HOME` is
/// not set. Only an absolute path counts.
fn home_dir() -> Option<PathBuf> {
    let var = std::env::var_os("HOME").or_else(|| if cfg!(windows) { std::env::var_os("USERPROFILE") } else { None });
    var.map(PathBuf::from).filter(|h| h.is_absolute())
}

/// 128 bits from the system's random source. There is no weaker fallback: an
/// id someone could guess would let them confirm an action.
#[cfg(unix)]
fn random_id() -> Result<String, String> {
    use std::fs::File;
    use std::io::Read;

    let mut bytes = [0u8; 16];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| format!("cannot make an action id: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// DEVELOPMENT ONLY, compiled under `cfg(windows)` so that the crate can be
/// built and run on a developer's Windows machine. ScarfeeOS ships on Linux,
/// which uses `/dev/urandom` above. The standard library has no documented
/// secure random source and no dependency may be added, so this takes the
/// randomly keyed hashers that the standard library seeds from the operating
/// system, and mixes in the clock and a counter so that ids differ. That is
/// unpredictable in practice but not a vetted CSPRNG: do not ship it.
#[cfg(windows)]
fn random_id() -> Result<String, String> {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .map_err(|e| format!("cannot make an action id: {e}"))?;
    let part = |salt: u64| {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(salt ^ nanos);
        hasher.write_u64(COUNTER.fetch_add(1, Ordering::SeqCst));
        hasher.finish()
    };
    Ok(format!("{:016x}{:016x}", part(1), part(2)))
}

fn with_time(call: &ToolCall, time: &str) -> ToolCall {
    match call {
        ToolCall::CreateReminder { text, repeat, .. } => {
            ToolCall::CreateReminder { text: text.clone(), time: time.into(), repeat: *repeat }
        }
        ToolCall::ScheduleMeeting { title, attendees, repeat, .. } => ToolCall::ScheduleMeeting {
            title: title.clone(),
            time: time.into(),
            attendees: attendees.clone(),
            repeat: *repeat,
        },
        other => other.clone(),
    }
}

/// `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM[:SS]`: local time, no offset, because the
/// calendar stores floating times.
fn is_iso_8601(s: &str) -> bool {
    let (day, time) = match s.split_once('T') {
        Some((day, time)) => (day, Some(time)),
        None => (s, None),
    };
    if parse_date(day).is_none() {
        return false;
    }
    let Some(time) = time else { return true };
    let parts: Vec<&str> = time.split(':').collect();
    let limits = [24, 60, 60];
    (2..=3).contains(&parts.len())
        && parts.iter().zip(limits).all(|(p, limit)| {
            p.len() == 2 && p.bytes().all(|b| b.is_ascii_digit()) && p.parse::<u32>().is_ok_and(|n| n < limit)
        })
}

fn parse_date(s: &str) -> Option<Date> {
    let parts: Vec<&str> = s.split('-').collect();
    let [year, month, day] = parts[..] else { return None };
    let digits = |p: &str, len: usize| p.len() == len && p.bytes().all(|b| b.is_ascii_digit());
    if !(digits(year, 4) && digits(month, 2) && digits(day, 2)) {
        return None;
    }
    let date = Date::new(year.parse().ok()?, month.parse().ok()?, day.parse().ok()?);
    // A day past the end of the month rolls over to the next month.
    ((1..=12).contains(&date.month) && date.add_days(0) == date).then_some(date)
}

fn format_date(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year, d.month, d.day)
}

fn event_json(event: &Event) -> Value {
    let start = match event.start {
        Start::Day(date) => format_date(date),
        Start::Time { date, hour, minute, second } => format!("{}T{hour:02}:{minute:02}:{second:02}", format_date(date)),
    };
    json!({
        "uid": event.uid,
        "summary": event.summary,
        "start": start,
        "attendees": event.attendees,
        "repeat": repeat_name(event.repeat),
    })
}

pub fn repeat_name(repeat: Option<Repeat>) -> Option<&'static str> {
    repeat.map(|r| match r {
        Repeat::Daily => "daily",
        Repeat::Weekly => "weekly",
    })
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn hidden_path() -> Failure {
    Failure::new("hidden_path", "hidden folders are not listed".into())
}

/// A dot name hides a folder, as in `ls`. Windows hidden attributes are not read.
fn has_hidden_component(path: &Path) -> bool {
    path.components().any(|c| matches!(c, Component::Normal(name) if name.to_string_lossy().starts_with('.')))
}

/// Whether the path as written goes through a hidden folder below the root.
/// A path outside the root is left to the sandbox, which refuses it.
fn names_hidden_part(input: &str, root: &Path) -> bool {
    let path = Path::new(input.trim());
    let below_root = if let Ok(rest) = path.strip_prefix("~") {
        rest
    } else if path.is_absolute() {
        match path.strip_prefix(root) {
            Ok(rest) => rest,
            Err(_) => return false,
        }
    } else {
        path
    };
    has_hidden_component(below_root)
}

/// Bidirectional controls can reorder a name so that an .exe looks like a
/// .txt. They are written out as `<U+202E>` instead. Arabic letters and
/// everything else stay as they are.
fn display_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' | '\u{061C}') {
            out.push_str(&format!("<U+{:04X}>", c as u32));
        } else {
            out.push(c);
        }
    }
    out
}

fn entries_json(entries: &[FileEntry]) -> Vec<Value> {
    entries
        .iter()
        .map(|e| {
            let kind = match e.kind {
                EntryKind::File => "file",
                EntryKind::Dir => "dir",
                EntryKind::Symlink => "symlink",
                EntryKind::Other => "other",
            };
            let name = e.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            // `path` is the real name for any later operation; show `display_*` to the user.
            json!({
                "path": path_text(&e.path),
                "display_path": display_text(&path_text(&e.path)),
                "display_name": display_text(&name),
                "kind": kind,
            })
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod testing {
    use std::cell::RefCell;
    use std::fs;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;
    use crate::calendar::FixedUtc;
    use crate::tools::{FixedClock, LocalTime};

    /// 2026-10-30 09:05:30 local, 2026-10-30 10:15:00 UTC.
    pub const NOW: LocalTime = LocalTime { date: Date { year: 2026, month: 10, day: 30 }, hour: 9, minute: 5, second: 30 };

    pub struct TempDir(pub PathBuf);

    impl TempDir {
        pub fn new() -> TempDir {
            static COUNT: AtomicU32 = AtomicU32::new(0);
            let dir = std::env::temp_dir()
                .join(format!("scarfeeos-executor-{}-{}", std::process::id(), COUNT.fetch_add(1, Ordering::SeqCst)));
            fs::create_dir_all(&dir).unwrap();
            TempDir(fs::canonicalize(dir).unwrap())
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Remembers which apps were started instead of starting them.
    #[derive(Clone, Default)]
    pub struct FakeLauncher(pub Rc<RefCell<Vec<String>>>);

    impl Launcher for FakeLauncher {
        fn launch(&self, app: &App) -> Result<(), LaunchError> {
            self.0.borrow_mut().push(app.id.clone());
            Ok(())
        }
    }

    pub fn app(id: &str, name: &str, exec: &[&str]) -> App {
        App {
            id: id.into(),
            name: name.into(),
            arabic_names: Vec::new(),
            exec: exec.iter().map(|s| s.to_string()).collect(),
            terminal: false,
            desktop_file: PathBuf::from(id),
        }
    }

    /// A home folder with a few files, a calendar file that does not exist yet
    /// in a data folder that does not exist yet, and a fake launcher.
    pub struct Env {
        pub tmp: TempDir,
        pub calendar: PathBuf,
        /// What the executor started, when it uses the fake launcher.
        pub launched: FakeLauncher,
        pub executor: Executor,
    }

    impl Env {
        pub fn new() -> Env {
            let launched = FakeLauncher::default();
            let apps = vec![app("calc.desktop", "Calculator", &["calc"])];
            Env::build(apps, Box::new(launched.clone()), launched)
        }

        /// `launched` is only filled when `launcher` is a clone of it.
        pub fn build(apps: Vec<App>, launcher: Box<dyn Launcher>, launched: FakeLauncher) -> Env {
            let tmp = TempDir::new();
            let home = tmp.0.join("home");
            fs::create_dir_all(home.join("Documents")).unwrap();
            fs::write(home.join("Documents/budget 2026.txt"), "x").unwrap();
            fs::write(home.join("notes.txt"), "x").unwrap();
            let calendar = tmp.0.join("data/scarfeeos/calendar.ics");
            let files = FileSandbox::new(FilesConfig::for_home(&home, &home.join(".config"))).unwrap();
            let executor = Executor::new(
                files,
                AppTable::new(apps),
                launcher,
                calendar.clone(),
                Box::new(FixedUtc(1_793_355_300)),
                Box::new(FixedClock(NOW)),
            );
            Env { tmp, calendar, launched, executor }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fs;

    use super::testing::*;
    use super::*;
    use crate::lang::Language;

    fn reminder(time: &str) -> ToolCall {
        ToolCall::CreateReminder { text: "call the bank".into(), time: time.into(), repeat: None }
    }

    fn meeting(time: &str) -> ToolCall {
        ToolCall::ScheduleMeeting {
            title: "Review".into(),
            time: time.into(),
            attendees: vec!["Sara".into()],
            repeat: Some(Repeat::Weekly),
        }
    }

    #[test]
    fn lists_and_searches_inside_the_root() {
        let env = Env::new();
        let r = env.executor.run(&ToolCall::ListFiles { path: "~".into() });
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["type"], "listing");
        let kinds: Vec<(String, String)> = r["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                let name = Path::new(e["path"].as_str().unwrap()).file_name().unwrap().to_string_lossy().into_owned();
                (name, e["kind"].as_str().unwrap().to_string())
            })
            .collect();
        assert_eq!(kinds, [("Documents".into(), "dir".into()), ("notes.txt".into(), "file".into())]);
        assert_eq!(r["truncated"], false);

        let r = env.executor.run(&ToolCall::SearchFiles { query: "budget".into() });
        assert_eq!((&r["ok"], &r["type"]), (&json!(true), &json!("search")), "{r}");
        assert_eq!(r["matches"].as_array().unwrap().len(), 1, "{r}");
        assert!(r["matches"][0]["path"].as_str().unwrap().ends_with("budget 2026.txt"));
        assert_eq!(r["unreadable"], 0);
    }

    #[test]
    fn file_errors_are_clear_and_stay_inside_the_root() {
        let env = Env::new();
        let r = env.executor.run(&ToolCall::ListFiles { path: "../../..".into() });
        assert_eq!(r["ok"], false, "{r}");
        assert!(matches!(r["error"]["code"].as_str(), Some("outside_root" | "not_found")), "{r}");
        assert!(!r["error"]["message"].as_str().unwrap().is_empty());
        // A real folder above the root; "/etc" is not an absolute path on Windows.
        let outside = env.tmp.0.display().to_string();
        let r = env.executor.run(&ToolCall::ListFiles { path: outside });
        assert_eq!(r["error"]["code"], "outside_root", "{r}");
        let r = env.executor.run(&ToolCall::SearchFiles { query: "  ".into() });
        assert_eq!(r["error"]["code"], "empty_query", "{r}");
    }

    fn error_code(r: &Value) -> Option<&str> {
        r["error"]["code"].as_str()
    }

    fn display_names(r: &Value) -> Vec<String> {
        r["entries"].as_array().unwrap().iter().map(|e| e["display_name"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn hidden_folders_are_refused_by_path_and_left_out_of_results() {
        let env = Env::new();
        let home = env.tmp.0.join("home");
        fs::create_dir_all(home.join(".ssh")).unwrap();
        fs::write(home.join(".ssh/id_secret"), "x").unwrap();
        fs::create_dir_all(home.join("Documents/.private")).unwrap();
        fs::write(home.join("Documents/.private/id_secret2"), "x").unwrap();
        let by_absolute_path = home.join(".ssh").display().to_string();
        for path in ["~/.ssh", ".ssh", "~/Documents/.private", "Documents/.private", "~/.ssh/..", "~/./.ssh/", &by_absolute_path] {
            let r = env.executor.run(&ToolCall::ListFiles { path: path.into() });
            assert_eq!((r["ok"].as_bool(), error_code(&r)), (Some(false), Some("hidden_path")), "{path}: {r}");
            assert!(r["error"]["message"].as_str().unwrap().contains("hidden"), "{r}");
            assert!(r.get("entries").is_none(), "{r}");
        }

        let r = env.executor.run(&ToolCall::ListFiles { path: "~".into() });
        assert_eq!(display_names(&r), ["Documents", "notes.txt"], "{r}");
        let r = env.executor.run(&ToolCall::ListFiles { path: "Documents".into() });
        assert_eq!(display_names(&r), ["budget 2026.txt"], "{r}");
        for query in ["id_secret", "ssh", "private"] {
            let r = env.executor.run(&ToolCall::SearchFiles { query: query.into() });
            assert_eq!(r["matches"].as_array().unwrap().len(), 0, "{query}: {r}");
        }
    }

    // A symlink is judged by where it leads: one into a hidden folder is refused
    // like the folder itself, one into a visible folder lists normally, and
    // neither is followed by a search.
    #[cfg(unix)]
    #[test]
    fn symlinks_into_hidden_folders_are_refused() {
        use std::os::unix::fs::symlink;
        let env = Env::new();
        let home = env.tmp.0.join("home");
        fs::create_dir_all(home.join(".ssh")).unwrap();
        fs::write(home.join(".ssh/id_secret"), "x").unwrap();
        symlink(home.join(".ssh"), home.join("innocent")).unwrap();
        symlink("Documents", home.join("shortcut")).unwrap();
        symlink("../.ssh", home.join("Documents/relative")).unwrap();

        for path in ["innocent", "~/innocent", "Documents/relative", "~/Documents/relative"] {
            let r = env.executor.run(&ToolCall::ListFiles { path: path.into() });
            assert_eq!(error_code(&r), Some("hidden_path"), "{path}: {r}");
        }
        let r = env.executor.run(&ToolCall::ListFiles { path: "shortcut".into() });
        assert_eq!(display_names(&r), ["budget 2026.txt", "relative"], "{r}");
        let r = env.executor.run(&ToolCall::SearchFiles { query: "id_secret".into() });
        assert_eq!(r["matches"].as_array().unwrap().len(), 0, "{r}");
        // The link itself is listed by name, as a symlink.
        let r = env.executor.run(&ToolCall::ListFiles { path: "~".into() });
        let link = r["entries"].as_array().unwrap().iter().find(|e| e["display_name"] == "innocent").unwrap();
        assert_eq!(link["kind"], "symlink", "{r}");
    }

    #[test]
    fn bidi_controls_are_shown_as_text_but_the_real_name_is_kept() {
        let env = Env::new();
        let home = env.tmp.0.join("home");
        // Reads as "reportexe.txt" on screen, but is an .exe.
        let real = "report\u{202E}txt.exe";
        fs::write(home.join(real), "x").unwrap();
        let r = env.executor.run(&ToolCall::ListFiles { path: "~".into() });
        assert_eq!(r["ok"], true, "{r}");
        let entry = r["entries"].as_array().unwrap().iter().find(|e| e["display_name"] == "report<U+202E>txt.exe").unwrap();
        let path = entry["path"].as_str().unwrap();
        assert!(path.ends_with(real), "{path:?}");
        assert!(entry["display_path"].as_str().unwrap().ends_with("report<U+202E>txt.exe"));
        assert!(Path::new(path).is_file(), "the real name must still open the file");

        let r = env.executor.run(&ToolCall::SearchFiles { query: "report".into() });
        assert_eq!(r["matches"][0]["display_name"], "report<U+202E>txt.exe", "{r}");
        assert_eq!(r["matches"][0]["path"], entry["path"], "{r}");

        // A folder with a control in its name: the listing path shows it safely too.
        fs::create_dir(home.join("dir\u{2067}x")).unwrap();
        let r = env.executor.run(&ToolCall::ListFiles { path: "dir\u{2067}x".into() });
        assert_eq!(r["ok"], true, "{r}");
        assert!(r["display_path"].as_str().unwrap().ends_with("dir<U+2067>x"), "{r}");
        assert!(r["path"].as_str().unwrap().ends_with("dir\u{2067}x"), "{r}");
    }

    #[test]
    fn every_bidi_control_is_replaced_and_nothing_else() {
        let controls = ('\u{202A}'..='\u{202E}').chain('\u{2066}'..='\u{2069}').chain(['\u{200E}', '\u{200F}', '\u{061C}']);
        for c in controls {
            let shown = display_text(&format!("a{c}b"));
            assert_eq!(shown, format!("a<U+{:04X}>b", c as u32));
        }
        // Neighbours of the ranges, the zero width joiners used in Arabic and ordinary text.
        for text in ["a\u{2029}b", "a\u{202F}b", "a\u{2065}b", "a\u{206A}b", "a\u{200D}b", "a\u{200C}b", "a\u{200B}b", "plain.txt"] {
            assert_eq!(display_text(text), text);
        }
    }

    #[test]
    fn arabic_file_names_are_not_changed() {
        let env = Env::new();
        let home = env.tmp.0.join("home");
        let name = "تقرير الميزانية ٢٠٢٦ ـ نهائي.txt";
        fs::write(home.join(name), "x").unwrap();
        fs::write(home.join("ملف\u{200F}.txt"), "x").unwrap();
        let r = env.executor.run(&ToolCall::ListFiles { path: "~".into() });
        let entry = r["entries"].as_array().unwrap().iter().find(|e| e["path"].as_str().unwrap().ends_with(name)).unwrap();
        assert_eq!(entry["display_name"], name, "{r}");
        assert_eq!(Path::new(entry["path"].as_str().unwrap()).file_name().unwrap(), name);
        // Only the control mark goes; the Arabic letters around it stay.
        assert!(display_names(&r).contains(&"ملف<U+200F>.txt".to_string()), "{r}");

        let r = env.executor.run(&ToolCall::SearchFiles { query: "الميزانية".into() });
        assert_eq!(r["matches"][0]["display_name"], name, "{r}");
    }

    #[test]
    fn open_app_starts_only_an_app_from_the_table() {
        let env = Env::new();
        let r = env.executor.run(&ToolCall::OpenApp { name: "calculator".into() });
        assert_eq!(r, json!({"ok": true, "type": "opened", "app": {"id": "calc.desktop", "name": "Calculator"}}));
        assert_eq!(*env.launched.0.borrow(), ["calc.desktop"]);

        let r = env.executor.run(&ToolCall::OpenApp { name: "calculater".into() });
        assert_eq!((&r["ok"], &r["error"]["code"]), (&json!(false), &json!("app_not_found")), "{r}");
        assert_eq!(r["error"]["closest"], json!(["Calculator"]));
        assert_eq!(env.launched.0.borrow().len(), 1);
    }

    #[test]
    fn the_command_launcher_refuses_what_it_cannot_start() {
        assert!(CommandLauncher.launch(&app("x.desktop", "X", &["/nonexistent/scarfeeos-test-program"])).is_err());
        let mut terminal = app("t.desktop", "T", &["true"]);
        terminal.terminal = true;
        assert!(CommandLauncher.launch(&terminal).is_err());
        assert!(CommandLauncher.launch(&app("e.desktop", "E", &[])).is_err());
    }

    #[test]
    fn reminders_and_meetings_go_to_the_calendar_file() {
        let env = Env::new();
        assert!(!env.calendar.exists());
        let r = env.executor.run(&reminder("2026-10-31T10:00:00"));
        assert_eq!((&r["ok"], &r["type"]), (&json!(true), &json!("event")), "{r}");
        let event = &r["event"];
        assert_eq!(event["summary"], "call the bank");
        assert_eq!(event["start"], "2026-10-31T10:00:00");
        assert_eq!(event["repeat"], Value::Null);
        let uid = event["uid"].as_str().unwrap().to_string();
        assert!(!uid.is_empty());

        let r = env.executor.run(&meeting("2026-11-01"));
        assert_eq!(r["event"]["attendees"], json!(["Sara"]), "{r}");
        assert_eq!(r["event"]["repeat"], "weekly");
        assert_eq!(r["event"]["start"], "2026-11-01");
        assert_ne!(r["event"]["uid"].as_str().unwrap(), uid);

        let file = fs::read_to_string(&env.calendar).unwrap();
        assert!(file.contains(&format!("UID:{uid}")) && file.contains("SUMMARY:Review"), "{file}");
    }

    #[test]
    fn calendar_errors_are_results_and_a_bad_file_is_left_alone() {
        let env = Env::new();
        let r = env.executor.run(&reminder("tomorrow"));
        assert_eq!((&r["ok"], &r["error"]["code"]), (&json!(false), &json!("invalid_time")), "{r}");
        let r = env.executor.run(&ToolCall::CreateReminder { text: " ".into(), time: "2026-10-31".into(), repeat: None });
        assert_eq!(r["error"]["code"], "empty_text", "{r}");

        fs::create_dir_all(env.calendar.parent().unwrap()).unwrap();
        fs::write(&env.calendar, "this is not a calendar").unwrap();
        let r = env.executor.run(&reminder("2026-10-31T10:00:00"));
        assert_eq!(r["error"]["code"], "calendar_malformed", "{r}");
        assert_eq!(fs::read_to_string(&env.calendar).unwrap(), "this is not a calendar");
    }

    #[test]
    fn set_language_returns_the_language_and_writes_nothing() {
        let env = Env::new();
        let before = fs::read_dir(&env.tmp.0).unwrap().count();
        let r = env.executor.run(&ToolCall::SetLanguage { lang: Language::Arabic });
        assert_eq!(r, json!({"ok": true, "type": "language", "lang": "ar"}));
        assert!(!env.calendar.exists() && !env.calendar.parent().unwrap().exists());
        assert_eq!(fs::read_dir(&env.tmp.0).unwrap().count(), before);
        assert_eq!(env.executor.run(&ToolCall::CurrentTime { timezone: None })["error"]["code"], "unsupported");
    }

    #[test]
    fn a_held_call_runs_once_by_its_id() {
        let mut env = Env::new();
        let call = ToolCall::SetLanguage { lang: Language::Arabic };
        let id = env.executor.hold(Held::Confirmation, call.clone()).unwrap();
        let (ran, result) = env.executor.confirm(&id, None).unwrap();
        assert_eq!(ran, call);
        assert_eq!(result["lang"], "ar");
        assert!(env.executor.confirm(&id, None).is_err());
        assert!(env.executor.cancel(&id).is_err());
    }

    #[test]
    fn an_unknown_id_runs_nothing() {
        let mut env = Env::new();
        let _held = env.executor.hold(Held::Clarification, reminder("2026-10-30T11:00:00")).unwrap();
        for id in ["", "0", "nope", "../etc", "0123456789abcdef0123456789abcdef"] {
            assert!(env.executor.confirm(id, None).is_err(), "{id}");
            assert!(env.executor.confirm(id, Some("2026-10-30T12:00:00")).is_err(), "{id}");
            assert!(env.executor.cancel(id).is_err(), "{id}");
        }
        assert!(!env.calendar.exists());
        assert!(env.launched.0.borrow().is_empty());
    }

    #[test]
    fn cancel_drops_the_call_for_good() {
        let mut env = Env::new();
        let id = env.executor.hold(Held::Clarification, reminder("2026-10-30T11:00:00")).unwrap();
        env.executor.cancel(&id).unwrap();
        assert!(env.executor.confirm(&id, None).is_err());
        assert!(!env.calendar.exists());
    }

    #[test]
    fn clarification_runs_the_guess_or_the_chosen_time() {
        let mut env = Env::new();
        let id = env.executor.hold(Held::Clarification, reminder("2026-10-30T11:00:00")).unwrap();
        let (_, result) = env.executor.confirm(&id, None).unwrap();
        assert_eq!(result["event"]["start"], "2026-10-30T11:00:00");

        let id = env.executor.hold(Held::Clarification, meeting("2026-10-30T11:00:00")).unwrap();
        let (call, result) = env.executor.confirm(&id, Some(" 2026-10-30T23:00 ")).unwrap();
        assert_eq!(result["event"]["start"], "2026-10-30T23:00:00", "{result}");
        assert_eq!(result["event"]["repeat"], "weekly");
        assert_eq!(result["event"]["attendees"], json!(["Sara"]));
        assert!(matches!(call, ToolCall::ScheduleMeeting { ref time, .. } if time == "2026-10-30T23:00"));
    }

    #[test]
    fn a_bad_chosen_time_is_refused_and_the_call_stays_held() {
        let mut env = Env::new();
        let id = env.executor.hold(Held::Clarification, reminder("2026-10-30T11:00:00")).unwrap();
        for bad in [
            "", "tomorrow", "11pm", "2026-13-01", "2026-02-30", "2026-10-30T25:00", "2026-10-30T10:60", "2026-10-30T10",
            "2026-10-30T10:00:00Z", "2026-10-30T10:00:00+03:00", "2026-10-30 10:00", "26-10-30", "2026-1-3",
            "2026-10-30T1:00", "2026-10-30T10:00:00:00", "2026-10-30T-1:00", "+026-10-30",
        ] {
            let error = env.executor.confirm(&id, Some(bad)).unwrap_err();
            assert!(error.contains("invalid time"), "{bad}: {error}");
        }
        assert!(!env.calendar.exists());
        for good in ["2026-10-30", "2026-02-28", "2028-02-29", "2026-10-30T00:00", "2026-10-30T23:59:59"] {
            assert!(is_iso_8601(good), "{good}");
        }
        assert!(env.executor.confirm(&id, Some("2026-10-30T12:00:00")).is_ok());
    }

    #[test]
    fn a_confirmation_takes_no_time() {
        let mut env = Env::new();
        let id = env.executor.hold(Held::Confirmation, ToolCall::SetLanguage { lang: Language::English }).unwrap();
        assert!(env.executor.confirm(&id, Some("2026-10-30T12:00:00")).is_err());
        assert!(env.executor.confirm(&id, None).is_ok());
    }

    #[test]
    fn ids_are_random_and_the_oldest_expire() {
        let mut env = Env::new();
        let call = ToolCall::SetLanguage { lang: Language::English };
        let ids: Vec<String> = (0..MAX_HELD + 8).map(|_| env.executor.hold(Held::Confirmation, call.clone()).unwrap()).collect();
        assert_eq!(ids.iter().collect::<HashSet<_>>().len(), ids.len());
        assert!(ids.iter().all(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())));
        assert!(env.executor.confirm(&ids[0], None).is_err());
        assert!(env.executor.confirm(&ids[7], None).is_err());
        assert!(env.executor.confirm(&ids[8], None).is_ok());
        assert!(env.executor.confirm(ids.last().unwrap(), None).is_ok());
    }

    #[test]
    fn upcoming_lists_from_a_date_and_delete_removes() {
        let env = Env::new();
        assert_eq!(env.executor.upcoming(None).unwrap(), json!({"from": "2026-10-30", "events": []}));
        let a = env.executor.run(&reminder("2026-11-05T09:00:00"))["event"]["uid"].as_str().unwrap().to_string();
        let b = env.executor.run(&meeting("2026-11-02T14:00:00"))["event"]["uid"].as_str().unwrap().to_string();
        let listed = env.executor.upcoming(None).unwrap();
        let uids: Vec<&str> = listed["events"].as_array().unwrap().iter().map(|e| e["uid"].as_str().unwrap()).collect();
        assert_eq!(uids, [b.as_str(), a.as_str()]);
        assert_eq!(listed["events"][0]["date"], "2026-11-02");
        assert_eq!(listed["events"][0]["summary"], "Review");

        let later = env.executor.upcoming(Some("2026-11-03")).unwrap();
        let uids: Vec<&str> = later["events"].as_array().unwrap().iter().map(|e| e["uid"].as_str().unwrap()).collect();
        assert_eq!(uids, [a.as_str(), b.as_str()], "the weekly meeting comes again: {later}");
        assert_eq!(later["events"][1]["date"], "2026-11-09");
        assert!(env.executor.upcoming(Some("soon")).is_err());

        env.executor.delete_event(&b).unwrap();
        let listed = env.executor.upcoming(None).unwrap();
        assert_eq!(listed["events"].as_array().unwrap().len(), 1);
        assert!(env.executor.delete_event(&b).unwrap_err().contains("no event"));
        assert!(env.executor.delete_event("nope").is_err());
    }

    // The XDG rules are Unix: "/home/u" and ":" separated lists are not absolute paths or separators on Windows.
    #[cfg(unix)]
    #[test]
    fn paths_follow_the_xdg_rules() {
        let home = Path::new("/home/u");
        let p = SystemPaths::from_vars(home, None, None);
        assert_eq!(p.calendar, Path::new("/home/u/.local/share/scarfeeos/calendar.ics"));
        assert_eq!(
            p.app_dirs,
            [
                "/home/u/.local/share/applications",
                "/usr/local/share/applications",
                "/usr/share/applications"
            ]
            .map(PathBuf::from)
        );
        let p = SystemPaths::from_vars(home, Some(OsStr::new("/data")), Some(OsStr::new("/opt/a:rel/b:/opt/c")));
        assert_eq!(p.calendar, Path::new("/data/scarfeeos/calendar.ics"));
        assert_eq!(p.app_dirs, ["/data/applications", "/opt/a/applications", "/opt/c/applications"].map(PathBuf::from));
        // Relative or empty values are ignored.
        let p = SystemPaths::from_vars(home, Some(OsStr::new("relative")), Some(OsStr::new("")));
        assert_eq!(p.calendar, Path::new("/home/u/.local/share/scarfeeos/calendar.ics"));
        assert_eq!(p.app_dirs.len(), 3);
        let p = SystemPaths::from_vars(home, Some(OsStr::new("")), None);
        assert_eq!(p.calendar, Path::new("/home/u/.local/share/scarfeeos/calendar.ics"));
    }
}
