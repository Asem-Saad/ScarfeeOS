//! Runs the read-only and low-risk tools: ListFiles, SearchFiles and OpenApp.
//! Nothing here deletes, moves or overwrites a file, and model text is never
//! run as a command: it is only compared with names the system itself lists.

use std::collections::{HashSet, VecDeque};
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::tools::ToolCall;

/// Lowercases and folds Arabic spelling variants, so that إيصال matches ايصال
/// and ٢ matches 2. Same folding as `normalize` in datetime.rs, which is
/// private to that module.
fn normalize(text: &str) -> String {
    text.chars()
        .filter(|c| !('\u{064B}'..='\u{0652}').contains(c))
        .map(|c| match c {
            'أ' | 'إ' | 'آ' => 'ا',
            'ة' => 'ه',
            'ى' => 'ي',
            '٠'..='٩' => char::from_u32(c as u32 - 0x0660 + 0x30).unwrap_or(c),
            _ => c,
        })
        .flat_map(char::to_lowercase)
        .collect()
}

fn words(text: &str) -> Vec<String> {
    normalize(text).split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(String::from).collect()
}

// ---------------------------------------------------------------- files

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileError {
    /// The configured root cannot be used.
    Root(String),
    NotFound(String),
    /// The path leads outside the root, by `..`, a symlink or an absolute path.
    OutsideRoot,
    NotADirectory,
    Unreadable(String),
    EmptyQuery,
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileError::Root(why) => write!(f, "file root unusable: {why}"),
            FileError::NotFound(path) => write!(f, "not found: {path}"),
            FileError::OutsideRoot => write!(f, "path is outside the allowed folder"),
            FileError::NotADirectory => write!(f, "not a folder"),
            FileError::Unreadable(why) => write!(f, "cannot read folder: {why}"),
            FileError::EmptyQuery => write!(f, "empty search"),
        }
    }
}

/// Bounds on one request, so a huge home folder cannot stall the assistant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_results: usize,
    /// Levels below the root that a search enters. 1 means the root only.
    pub max_depth: usize,
    /// Directory entries looked at before a search or listing stops.
    pub max_visited: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_results: 100, max_depth: 4, max_visited: 20_000 }
    }
}

/// The user's Documents, Downloads and Desktop folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserDirs {
    pub documents: PathBuf,
    pub downloads: PathBuf,
    pub desktop: PathBuf,
}

impl UserDirs {
    pub fn defaults(home: &Path) -> Self {
        UserDirs { documents: home.join("Documents"), downloads: home.join("Downloads"), desktop: home.join("Desktop") }
    }

    /// Reads the text of `user-dirs.dirs`. Entries that are missing, relative,
    /// or point at the home folder itself (how XDG marks a folder as unused)
    /// keep their default.
    pub fn parse(text: &str, home: &Path) -> Self {
        let mut dirs = Self::defaults(home);
        for line in text.lines() {
            let Some((key, value)) = line.trim().split_once('=') else { continue };
            let value = value.trim().trim_matches('"');
            let path = match value.strip_prefix("$HOME/") {
                Some(rest) => home.join(rest),
                None if value.starts_with('/') => PathBuf::from(value),
                None => continue,
            };
            if path == home {
                continue;
            }
            match key.trim() {
                "XDG_DOCUMENTS_DIR" => dirs.documents = path,
                "XDG_DOWNLOAD_DIR" => dirs.downloads = path,
                "XDG_DESKTOP_DIR" => dirs.desktop = path,
                _ => {}
            }
        }
        dirs
    }

    /// The folder that an English or Arabic name stands for, ignoring case.
    fn named(&self, name: &OsStr) -> Option<&Path> {
        let name = normalize(name.to_str()?);
        let is = |names: &[&str]| names.iter().any(|n| normalize(n) == name);
        if is(&["Documents", "المستندات"]) {
            Some(&self.documents)
        } else if is(&["Downloads", "التنزيلات"]) {
            Some(&self.downloads)
        } else if is(&["Desktop", "سطح المكتب"]) {
            Some(&self.desktop)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct FilesConfig {
    /// Nothing outside this folder is listed or searched. May start with `~`.
    pub root: PathBuf,
    pub home: PathBuf,
    pub user_dirs: UserDirs,
    pub limits: Limits,
}

impl FilesConfig {
    /// Root is the home folder. `config_dir` is where `user-dirs.dirs` lives,
    /// normally `~/.config`.
    pub fn for_home(home: &Path, config_dir: &Path) -> Self {
        let user_dirs = match fs::read_to_string(config_dir.join("user-dirs.dirs")) {
            Ok(text) => UserDirs::parse(&text, home),
            Err(_) => UserDirs::defaults(home),
        };
        FilesConfig { root: home.to_path_buf(), home: home.to_path_buf(), user_dirs, limits: Limits::default() }
    }

    pub fn from_env() -> Result<Self, FileError> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|h| h.is_absolute())
            .ok_or_else(|| FileError::Root("HOME is not set".into()))?;
        let config_dir = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|c| c.is_absolute())
            .unwrap_or_else(|| home.join(".config"));
        Ok(Self::for_home(&home, &config_dir))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
    /// Listed by name only. Symlinks are never followed.
    Symlink,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub path: PathBuf,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub path: PathBuf,
    pub entries: Vec<FileEntry>,
    /// More entries exist than the limits allow.
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResults {
    pub matches: Vec<FileEntry>,
    pub truncated: bool,
    /// Folders that could not be read and were skipped.
    pub unreadable: usize,
}

pub struct FileSandbox {
    root: PathBuf,
    home: PathBuf,
    user_dirs: UserDirs,
    limits: Limits,
}

fn expand_home(path: &Path, home: &Path) -> PathBuf {
    match path.strip_prefix("~") {
        Ok(rest) => home.join(rest),
        Err(_) => path.to_path_buf(),
    }
}

fn kind_of(file_type: fs::FileType) -> EntryKind {
    if file_type.is_symlink() {
        EntryKind::Symlink
    } else if file_type.is_dir() {
        EntryKind::Dir
    } else if file_type.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    }
}

/// Name and kind of each visible entry. Hidden files are left out: they hold
/// keys and settings, not things a user asks the assistant to find.
fn visible_entries(dir: &Path, limit: usize) -> Result<(Vec<(String, EntryKind)>, bool), std::io::Error> {
    let mut out = Vec::new();
    for item in fs::read_dir(dir)? {
        let Ok(item) = item else { continue };
        let name = item.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if out.len() >= limit {
            return Ok((out, true));
        }
        let Ok(file_type) = item.file_type() else { continue };
        out.push((name, kind_of(file_type)));
    }
    Ok((out, false))
}

impl FileSandbox {
    pub fn new(config: FilesConfig) -> Result<Self, FileError> {
        let root = fs::canonicalize(expand_home(&config.root, &config.home)).map_err(|e| FileError::Root(e.to_string()))?;
        Ok(FileSandbox { root, home: config.home, user_dirs: config.user_dirs, limits: config.limits })
    }

    /// Turns the model's path into a real folder inside the root. `~` is the
    /// home folder, a relative path starts at the root, and a first component
    /// such as المستندات or Documents stands for the matching user folder.
    fn resolve(&self, input: &str) -> Result<PathBuf, FileError> {
        let path = Path::new(input.trim());
        let mut candidates = Vec::new();
        let base = if let Ok(rest) = path.strip_prefix("~") {
            Some((&self.home, rest))
        } else if path.is_absolute() {
            candidates.push(path.to_path_buf());
            None
        } else {
            Some((&self.root, path))
        };
        if let Some((base, rest)) = base {
            candidates.push(base.join(rest));
            let mut parts = rest.components();
            if let Some(dir) = parts.next().and_then(|first| self.user_dirs.named(first.as_os_str())) {
                candidates.push(dir.join(parts.as_path()));
            }
        }
        // The path as written wins over a mapped folder name when both exist.
        let real = candidates
            .iter()
            .find_map(|c| fs::canonicalize(c).ok())
            .ok_or_else(|| FileError::NotFound(input.trim().to_string()))?;
        if real.starts_with(&self.root) {
            Ok(real)
        } else {
            Err(FileError::OutsideRoot)
        }
    }

    pub fn list_files(&self, path: &str) -> Result<Listing, FileError> {
        let dir = self.resolve(path)?;
        if !dir.is_dir() {
            return Err(FileError::NotADirectory);
        }
        let (mut entries, mut truncated) =
            visible_entries(&dir, self.limits.max_visited).map_err(|e| FileError::Unreadable(e.to_string()))?;
        entries.sort_by_cached_key(|(name, kind)| (*kind != EntryKind::Dir, name.to_lowercase(), name.clone()));
        if entries.len() > self.limits.max_results {
            entries.truncate(self.limits.max_results);
            truncated = true;
        }
        let entries = entries.into_iter().map(|(name, kind)| FileEntry { path: dir.join(name), kind }).collect();
        Ok(Listing { path: dir, entries, truncated })
    }

    /// Looks for names that contain every word of `query`, breadth first so
    /// that shallow matches come first. Symlinked folders are not entered.
    pub fn search_files(&self, query: &str) -> Result<SearchResults, FileError> {
        let wanted = words(query);
        if wanted.is_empty() {
            return Err(FileError::EmptyQuery);
        }
        let mut results = SearchResults { matches: Vec::new(), truncated: false, unreadable: 0 };
        let mut budget = self.limits.max_visited;
        let mut queue = VecDeque::from([(self.root.clone(), 1)]);
        while let Some((dir, depth)) = queue.pop_front() {
            let Ok((mut entries, cut)) = visible_entries(&dir, budget) else {
                results.unreadable += 1;
                continue;
            };
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            budget -= entries.len();
            for (name, kind) in entries {
                let folded = normalize(&name);
                if wanted.iter().all(|w| folded.contains(w.as_str())) {
                    if results.matches.len() >= self.limits.max_results {
                        results.truncated = true;
                        return Ok(results);
                    }
                    results.matches.push(FileEntry { path: dir.join(&name), kind });
                }
                if kind == EntryKind::Dir && depth < self.limits.max_depth {
                    queue.push_back((dir.join(&name), depth + 1));
                }
            }
            if cut {
                results.truncated = true;
                break;
            }
        }
        Ok(results)
    }
}

// ----------------------------------------------------------------- apps

/// An application listed by a .desktop file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// Desktop file name, such as `org.gnome.Calculator.desktop`.
    pub id: String,
    pub name: String,
    /// Every `Name[ar]` form, including regional ones such as `Name[ar_EG]`.
    pub arabic_names: Vec<String>,
    /// Program and arguments from the Exec line, split without a shell and
    /// without file and URL placeholders.
    pub exec: Vec<String>,
    pub terminal: bool,
    pub desktop_file: PathBuf,
}

impl App {
    fn names(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.name.as_str()).chain(self.arabic_names.iter().map(String::as_str))
    }

    fn id_stem(&self) -> &str {
        self.id.strip_suffix(".desktop").unwrap_or(&self.id)
    }
}

/// Splits an Exec value into arguments by the Desktop Entry quoting rules. The
/// result goes to `Command` as is, never to a shell. Returns None for an empty
/// line, an unclosed quote or an unknown `%` code.
fn parse_exec(value: &str) -> Option<Vec<String>> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_arg = false;
    let mut quoted = false;
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                in_arg = true;
            }
            '\\' if quoted => match chars.next()? {
                n @ ('"' | '`' | '$' | '\\') => current.push(n),
                n => {
                    current.push('\\');
                    current.push(n);
                }
            },
            ' ' | '\t' if !quoted => {
                if in_arg {
                    args.push(std::mem::take(&mut current));
                    in_arg = false;
                }
            }
            '%' if !quoted => match chars.next()? {
                '%' => {
                    current.push('%');
                    in_arg = true;
                }
                'f' | 'F' | 'u' | 'U' | 'd' | 'D' | 'n' | 'N' | 'i' | 'c' | 'k' | 'v' | 'm' => {}
                _ => return None,
            },
            _ => {
                current.push(c);
                in_arg = true;
            }
        }
    }
    if quoted {
        return None;
    }
    if in_arg {
        args.push(current);
    }
    (!args.is_empty()).then_some(args)
}

fn unescape(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn is_arabic_locale(locale: &str) -> bool {
    locale.split(['_', '@']).next() == Some("ar")
}

/// Reads one .desktop file. Returns None for entries a user cannot launch:
/// not an application, hidden, or without a usable name and Exec line.
fn parse_desktop(id: &str, path: &Path, text: &str) -> Option<App> {
    let (mut name, mut exec, mut kind) = (None, None, None);
    let (mut arabic_names, mut hidden, mut terminal) = (Vec::new(), false, false);
    let mut in_entry = false;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let (key, value) = (key.trim(), value.trim());
        if !in_entry {
            continue;
        }
        match key {
            "Type" => kind = Some(value),
            "Name" => name = Some(unescape(value)),
            "Exec" => exec = parse_exec(value),
            "NoDisplay" | "Hidden" => hidden |= value == "true",
            "Terminal" => terminal = value == "true",
            _ => {
                let locale = key.strip_prefix("Name[").and_then(|k| k.strip_suffix(']'));
                if locale.is_some_and(is_arabic_locale) {
                    arabic_names.push(unescape(value));
                }
            }
        }
    }
    if kind != Some("Application") || hidden {
        return None;
    }
    Some(App { id: id.to_string(), name: name?, arabic_names, exec: exec?, terminal, desktop_file: path.to_path_buf() })
}

/// Names the model may use for an app, and what to look for on this system,
/// best first. Both sides go through `normalize` when compared.
const ALIASES: &[(&[&str], &[&str])] = &[
    (&["calculator", "calc", "الآلة الحاسبة", "آلة حاسبة", "الحاسبة"], &["calculator", "calc", "kcalc"]),
    (
        &["email", "e-mail", "mail", "البريد الإلكتروني", "البريد", "الايميل"],
        &["mail", "email", "thunderbird", "evolution", "geary", "kmail"],
    ),
    (&["music", "الموسيقى", "موسيقى"], &["music", "rhythmbox", "lollypop", "elisa", "amberol", "audacious"]),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    EmptyName,
    /// No app has that name. `closest` holds a few names that look similar.
    NotFound { closest: Vec<String> },
    Launch(LaunchError),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::EmptyName => write!(f, "no app name given"),
            OpenError::NotFound { closest } if closest.is_empty() => write!(f, "app not found"),
            OpenError::NotFound { closest } => write!(f, "app not found, did you mean: {}", closest.join(", ")),
            OpenError::Launch(e) => write!(f, "{e}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchError(pub String);

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot start app: {}", self.0)
    }
}

/// Starts an app that came out of an `AppTable`. Implementations run
/// `app.exec` directly, not through a shell.
pub trait Launcher {
    fn launch(&self, app: &App) -> Result<(), LaunchError>;
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (diagonal + usize::from(ca != cb)).min(above + 1).min(row[j] + 1);
            diagonal = above;
        }
    }
    row[b.len()]
}

pub struct AppTable {
    apps: Vec<App>,
}

impl AppTable {
    pub fn new(apps: Vec<App>) -> Self {
        AppTable { apps }
    }

    /// Reads the .desktop files directly inside each directory. When the same
    /// file name is in several, the first directory wins, even if its copy is
    /// hidden. Unreadable directories and files are skipped.
    pub fn load(dirs: &[PathBuf]) -> Self {
        let mut apps = Vec::new();
        let mut seen = HashSet::new();
        for dir in dirs {
            let Ok(items) = fs::read_dir(dir) else { continue };
            let mut paths: Vec<PathBuf> =
                items.filter_map(Result::ok).map(|i| i.path()).filter(|p| p.extension() == Some(OsStr::new("desktop"))).collect();
            paths.sort();
            for path in paths {
                let Some(id) = path.file_name().and_then(OsStr::to_str).map(String::from) else { continue };
                if !seen.insert(id.clone()) {
                    continue;
                }
                let Ok(text) = fs::read_to_string(&path) else { continue };
                apps.extend(parse_desktop(&id, &path, &text));
            }
        }
        AppTable { apps }
    }

    pub fn apps(&self) -> &[App] {
        &self.apps
    }

    /// Finds the app that `name` stands for: an exact name, an alias such as
    /// الآلة الحاسبة, or words that only one app name contains.
    pub fn find(&self, name: &str) -> Result<&App, OpenError> {
        let query = normalize(name.trim());
        if query.is_empty() {
            return Err(OpenError::EmptyName);
        }
        let exact = |wanted: &str| {
            self.apps.iter().find(|a| a.names().chain([a.id_stem()]).any(|n| normalize(n) == wanted))
        };
        if let Some(app) = exact(&query) {
            return Ok(app);
        }
        let alias = ALIASES.iter().find(|(names, _)| names.iter().any(|n| normalize(n) == query));
        for keyword in alias.map_or(&[][..], |(_, keywords)| keywords) {
            let by_word = || {
                self.apps.iter().find(|a| a.names().chain([a.id_stem()]).any(|n| words(n).contains(&keyword.to_string())))
            };
            if let Some(app) = exact(keyword).or_else(by_word) {
                return Ok(app);
            }
        }
        let wanted = words(name);
        let mut by_words = self.apps.iter().filter(|a| a.names().any(|n| {
            let have = words(n);
            wanted.iter().all(|w| have.contains(w))
        }));
        if let (Some(app), None) = (by_words.next(), by_words.next()) {
            if !wanted.is_empty() {
                return Ok(app);
            }
        }
        Err(OpenError::NotFound { closest: self.closest(&query) })
    }

    fn closest(&self, query: &str) -> Vec<String> {
        let mut scored: Vec<(usize, &str)> = self
            .apps
            .iter()
            .map(|a| {
                // Each word counts too, so "firefx" is close to "Firefox Web Browser".
                let forms = a.names().flat_map(|n| words(n).into_iter().chain([normalize(n)]));
                (forms.map(|f| edit_distance(query, &f)).min().unwrap_or(usize::MAX), a.name.as_str())
            })
            .collect();
        scored.sort();
        scored.dedup_by_key(|(_, name)| *name);
        scored.into_iter().take(3).map(|(_, name)| name.to_string()).collect()
    }

    /// Finds the app and starts it through `launcher`. Returns the app that
    /// was started.
    pub fn open(&self, name: &str, launcher: &dyn Launcher) -> Result<&App, OpenError> {
        let app = self.find(name)?;
        launcher.launch(app).map_err(OpenError::Launch)?;
        Ok(app)
    }
}

// ------------------------------------------------------------- dispatch

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Listing(Listing),
    Search(SearchResults),
    Opened(App),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionError {
    File(FileError),
    Open(OpenError),
    /// The tool is not one of the safe tools handled here.
    Unsupported,
}

impl fmt::Display for ActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActionError::File(e) => write!(f, "{e}"),
            ActionError::Open(e) => write!(f, "{e}"),
            ActionError::Unsupported => write!(f, "tool not supported here"),
        }
    }
}

impl From<FileError> for ActionError {
    fn from(e: FileError) -> Self {
        ActionError::File(e)
    }
}

impl From<OpenError> for ActionError {
    fn from(e: OpenError) -> Self {
        ActionError::Open(e)
    }
}

/// Runs ListFiles, SearchFiles and OpenApp. Every other tool is `Unsupported`.
pub fn execute(
    call: &ToolCall,
    files: &FileSandbox,
    apps: &AppTable,
    launcher: &dyn Launcher,
) -> Result<Outcome, ActionError> {
    match call {
        ToolCall::ListFiles { path } => Ok(Outcome::Listing(files.list_files(path)?)),
        ToolCall::SearchFiles { query } => Ok(Outcome::Search(files.search_files(query)?)),
        ToolCall::OpenApp { name } => Ok(Outcome::Opened(apps.open(name, launcher)?.clone())),
        _ => Err(ActionError::Unsupported),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static COUNT: AtomicUsize = AtomicUsize::new(0);
            let n = COUNT.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!("scarfeeos-actions-{}-{n}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            TempDir(fs::canonicalize(path).unwrap())
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// `home` is the sandbox root; `outside` is a sibling it must never reach.
    struct Fixture {
        _tmp: TempDir,
        home: PathBuf,
        outside: PathBuf,
    }

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "x").unwrap();
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = TempDir::new();
            let home = tmp.0.join("home");
            let outside = tmp.0.join("outside");
            fs::create_dir_all(&home).unwrap();
            fs::create_dir_all(&outside).unwrap();
            touch(&outside.join("secret.txt"));
            Fixture { _tmp: tmp, home, outside }
        }

        fn sandbox(&self) -> FileSandbox {
            self.sandbox_with(Limits::default())
        }

        fn sandbox_with(&self, limits: Limits) -> FileSandbox {
            let mut config = FilesConfig::for_home(&self.home, &self.home.join(".config"));
            config.limits = limits;
            FileSandbox::new(config).unwrap()
        }
    }

    fn names(entries: &[FileEntry]) -> Vec<String> {
        entries.iter().map(|e| e.path.file_name().unwrap().to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn lists_dirs_first_and_hides_dotfiles() {
        let fx = Fixture::new();
        touch(&fx.home.join("b.txt"));
        touch(&fx.home.join("A.txt"));
        touch(&fx.home.join(".ssh/key"));
        fs::create_dir(fx.home.join("zdir")).unwrap();
        let listing = fx.sandbox().list_files("~").unwrap();
        assert_eq!(names(&listing.entries), ["zdir", "A.txt", "b.txt"]);
        assert_eq!(listing.entries[0].kind, EntryKind::Dir);
        assert!(!listing.truncated);
        assert_eq!(fx.sandbox().list_files("").unwrap(), listing);
    }

    #[test]
    fn list_is_limited() {
        let fx = Fixture::new();
        for i in 0..5 {
            touch(&fx.home.join(format!("f{i}")));
        }
        let listing = fx.sandbox_with(Limits { max_results: 3, ..Limits::default() }).list_files("~").unwrap();
        assert_eq!(names(&listing.entries), ["f0", "f1", "f2"]);
        assert!(listing.truncated);
    }

    #[test]
    fn list_errors() {
        let fx = Fixture::new();
        touch(&fx.home.join("a.txt"));
        let sandbox = fx.sandbox();
        assert_eq!(sandbox.list_files("~/missing"), Err(FileError::NotFound("~/missing".into())));
        assert_eq!(sandbox.list_files("a.txt"), Err(FileError::NotADirectory));
        assert!(matches!(
            FileSandbox::new(FilesConfig { root: fx.home.join("nope"), ..FilesConfig::for_home(&fx.home, &fx.home) }),
            Err(FileError::Root(_))
        ));
    }

    #[test]
    fn dot_dot_cannot_leave_the_root() {
        let fx = Fixture::new();
        let sandbox = fx.sandbox();
        for path in ["..", "../outside", "~/..", "~/../outside", "a/../../outside", "../home/../outside"] {
            fs::create_dir_all(fx.home.join("a")).unwrap();
            assert_eq!(sandbox.list_files(path), Err(FileError::OutsideRoot), "{path}");
        }
        // Coming back inside is fine.
        assert!(sandbox.list_files("a/../a").is_ok());
    }

    #[test]
    fn absolute_paths_outside_are_rejected() {
        let fx = Fixture::new();
        let sandbox = fx.sandbox();
        assert_eq!(sandbox.list_files("/"), Err(FileError::OutsideRoot));
        assert_eq!(sandbox.list_files(fx.outside.to_str().unwrap()), Err(FileError::OutsideRoot));
        assert!(sandbox.list_files(fx.home.to_str().unwrap()).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_cannot_leave_the_root() {
        use std::os::unix::fs::symlink;
        let fx = Fixture::new();
        fs::create_dir_all(fx.home.join("real/sub")).unwrap();
        symlink(&fx.outside, fx.home.join("escape")).unwrap();
        symlink(fx.home.join("real"), fx.home.join("inside")).unwrap();
        symlink(fx.outside.join("secret.txt"), fx.home.join("real/link.txt")).unwrap();
        let sandbox = fx.sandbox();

        assert_eq!(sandbox.list_files("escape"), Err(FileError::OutsideRoot));
        assert_eq!(sandbox.list_files("~/escape/"), Err(FileError::OutsideRoot));
        assert_eq!(sandbox.list_files("real/../escape"), Err(FileError::OutsideRoot));
        // A link that stays inside resolves to its real place.
        let listing = sandbox.list_files("inside").unwrap();
        assert_eq!(listing.path, fx.home.join("real"));
        // Links are listed but never followed.
        let kinds: Vec<_> = listing.entries.iter().map(|e| e.kind).collect();
        assert_eq!(kinds, [EntryKind::Dir, EntryKind::Symlink]);

        let found = sandbox.search_files("secret").unwrap();
        assert!(found.matches.is_empty());
        let found = sandbox.search_files("link").unwrap();
        assert_eq!(names(&found.matches), ["link.txt"]);
        assert_eq!(sandbox.search_files("escape").unwrap().matches[0].kind, EntryKind::Symlink);
    }

    #[test]
    fn user_folder_names_map_in_both_languages() {
        let fx = Fixture::new();
        let config_dir = fx.home.join(".config");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(
            config_dir.join("user-dirs.dirs"),
            "# comment\nXDG_DOCUMENTS_DIR=\"$HOME/Docs\"\nXDG_DOWNLOAD_DIR=\"$HOME/Incoming\"\n\
             XDG_DESKTOP_DIR=\"$HOME/\"\nXDG_MUSIC_DIR=\"$HOME/Tunes\"\n",
        )
        .unwrap();
        for dir in ["Docs", "Incoming", "Desktop"] {
            touch(&fx.home.join(dir).join(format!("in-{dir}.txt")));
        }
        touch(&fx.home.join("Docs/reports/q1.txt"));
        let sandbox = fx.sandbox();
        let first = |path: &str| names(&sandbox.list_files(path).unwrap().entries).remove(0);

        assert_eq!(first("المستندات"), "reports");
        assert_eq!(first("~/المستندات"), "reports");
        assert_eq!(first("documents"), "reports");
        assert_eq!(first("~/DOCUMENTS"), "reports");
        assert_eq!(first("المستندات/reports"), "q1.txt");
        assert_eq!(first("التنزيلات"), "in-Incoming.txt");
        assert_eq!(first("Downloads"), "in-Incoming.txt");
        // A folder set to $HOME in the file keeps its default.
        assert_eq!(first("سطح المكتب"), "in-Desktop.txt");
        assert_eq!(first("Desktop"), "in-Desktop.txt");
        // A mapped folder cannot be used to climb out.
        assert_eq!(sandbox.list_files("المستندات/../.."), Err(FileError::OutsideRoot));
    }

    #[test]
    fn folder_names_use_defaults_without_a_config_file() {
        let fx = Fixture::new();
        touch(&fx.home.join("Documents/a.txt"));
        assert_eq!(names(&fx.sandbox().list_files("المستندات").unwrap().entries), ["a.txt"]);
        assert_eq!(fx.sandbox().list_files("التنزيلات"), Err(FileError::NotFound("التنزيلات".into())));
    }

    #[test]
    fn a_real_folder_beats_a_mapped_name() {
        let fx = Fixture::new();
        touch(&fx.home.join("المستندات/real.txt"));
        touch(&fx.home.join("Documents/mapped.txt"));
        assert_eq!(names(&fx.sandbox().list_files("المستندات").unwrap().entries), ["real.txt"]);
    }

    #[test]
    fn root_may_be_written_with_a_tilde() {
        let fx = Fixture::new();
        touch(&fx.home.join("sub/a.txt"));
        let config = FilesConfig { root: PathBuf::from("~/sub"), ..FilesConfig::for_home(&fx.home, &fx.home) };
        let sandbox = FileSandbox::new(config).unwrap();
        assert_eq!(names(&sandbox.list_files("").unwrap().entries), ["a.txt"]);
        assert_eq!(sandbox.list_files("~"), Err(FileError::OutsideRoot));
    }

    #[test]
    fn search_matches_latin_and_arabic_names() {
        let fx = Fixture::new();
        for name in ["Budget-2026.xlsx", "تقرير_المبيعات.txt", "إيصال.pdf", "فاتورة آذار.pdf", "notes.txt", "Docs/Annual Budget.odt"] {
            touch(&fx.home.join(name));
        }
        let sandbox = fx.sandbox();
        let search = |q: &str| names(&sandbox.search_files(q).unwrap().matches);

        assert_eq!(search("budget"), ["Budget-2026.xlsx", "Annual Budget.odt"]);
        assert_eq!(search("BUDGET 2026"), ["Budget-2026.xlsx"]);
        assert_eq!(search("تقرير"), ["تقرير_المبيعات.txt"]);
        // Spelling variants fold: ا for إ, ه for ة, and diacritics are ignored.
        assert_eq!(search("ايصال"), ["إيصال.pdf"]);
        assert_eq!(search("فاتوره"), ["فاتورة آذار.pdf"]);
        assert_eq!(search("فَاتُورَة اذار"), ["فاتورة آذار.pdf"]);
        assert_eq!(search("nothing like this"), Vec::<String>::new());
        assert_eq!(sandbox.search_files("  "), Err(FileError::EmptyQuery));
    }

    #[test]
    fn search_limits_depth_and_results() {
        let fx = Fixture::new();
        touch(&fx.home.join("a/b/c/d/deep.txt"));
        touch(&fx.home.join("a/b/mid.txt"));
        for i in 0..4 {
            touch(&fx.home.join(format!("many{i}.txt")));
        }
        let limit = |max_depth, max_results| Limits { max_depth, max_results, ..Limits::default() };

        let sandbox = fx.sandbox_with(limit(3, 100));
        assert_eq!(names(&sandbox.search_files("mid").unwrap().matches), ["mid.txt"]);
        assert!(sandbox.search_files("deep").unwrap().matches.is_empty());
        assert!(fx.sandbox_with(limit(1, 100)).search_files("mid").unwrap().matches.is_empty());
        assert_eq!(names(&fx.sandbox_with(limit(5, 100)).search_files("deep").unwrap().matches), ["deep.txt"]);

        let capped = fx.sandbox_with(limit(4, 2)).search_files("many").unwrap();
        assert_eq!(names(&capped.matches), ["many0.txt", "many1.txt"]);
        assert!(capped.truncated);
        let all = fx.sandbox_with(limit(4, 4)).search_files("many").unwrap();
        assert_eq!((all.matches.len(), all.truncated), (4, false));
    }

    #[test]
    fn search_stops_after_visiting_too_many_entries() {
        let fx = Fixture::new();
        for i in 0..10 {
            touch(&fx.home.join(format!("f{i}")));
        }
        let found = fx.sandbox_with(Limits { max_visited: 4, ..Limits::default() }).search_files("zzz").unwrap();
        assert!(found.truncated);
    }

    #[test]
    fn search_skips_hidden_folders() {
        let fx = Fixture::new();
        touch(&fx.home.join(".cache/needle.txt"));
        touch(&fx.home.join("visible/needle.txt"));
        let found = fx.sandbox().search_files("needle").unwrap();
        assert_eq!(found.matches.len(), 1);
        assert_eq!(found.matches[0].path, fx.home.join("visible/needle.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_folder_does_not_fail_the_search() {
        use std::os::unix::fs::PermissionsExt;
        let fx = Fixture::new();
        touch(&fx.home.join("locked/needle1.txt"));
        touch(&fx.home.join("open/needle2.txt"));
        let locked = fx.home.join("locked");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let readable_anyway = fs::read_dir(&locked).is_ok();
        let found = fx.sandbox().search_files("needle");
        let listed = fx.sandbox().list_files("locked");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

        if readable_anyway {
            // Running as root: permissions do not apply, so nothing to test.
            return;
        }
        let found = found.unwrap();
        assert_eq!(names(&found.matches), ["needle2.txt"]);
        assert_eq!(found.unreadable, 1);
        assert!(matches!(listed, Err(FileError::Unreadable(_))));
    }

    // ---- apps

    const CALCULATOR: &str = "\
# comment
[Desktop Entry]
Type=Application
Name=Calculator
Name[ar]=الآلة الحاسبة
Name[de]=Taschenrechner
Exec=gnome-calculator
Terminal=false

[Desktop Action new]
Name=New Window
Name[ar]=نافذة جديدة
Exec=gnome-calculator --new
";

    fn app(id: &str, name: &str, arabic: &[&str]) -> App {
        App {
            id: id.into(),
            name: name.into(),
            arabic_names: arabic.iter().map(|s| s.to_string()).collect(),
            exec: vec![name.to_lowercase()],
            terminal: false,
            desktop_file: PathBuf::from(id),
        }
    }

    fn table() -> AppTable {
        AppTable::new(vec![
            app("org.gnome.Calculator.desktop", "Calculator", &[]),
            app("firefox.desktop", "Firefox Web Browser", &["متصفح فايرفوكس"]),
            app("org.mozilla.Thunderbird.desktop", "Thunderbird", &["ثندربيرد"]),
            app("org.gnome.Rhythmbox3.desktop", "Rhythmbox", &[]),
            app("org.gnome.TextEditor.desktop", "Text Editor", &["محرر النصوص"]),
            app("org.gnome.gedit.desktop", "Text Viewer", &[]),
            app("org.gnome.Files.desktop", "Files", &["الملفات"]),
        ])
    }

    #[derive(Default)]
    struct FakeLauncher {
        launched: RefCell<Vec<String>>,
        fail: bool,
    }

    impl Launcher for FakeLauncher {
        fn launch(&self, app: &App) -> Result<(), LaunchError> {
            if self.fail {
                return Err(LaunchError("no display".into()));
            }
            self.launched.borrow_mut().push(app.id.clone());
            Ok(())
        }
    }

    #[test]
    fn parses_a_desktop_file() {
        let app = parse_desktop("calc.desktop", Path::new("/x/calc.desktop"), CALCULATOR).unwrap();
        assert_eq!(app.name, "Calculator");
        assert_eq!(app.arabic_names, ["الآلة الحاسبة"]);
        assert_eq!(app.exec, ["gnome-calculator"]);
        assert!(!app.terminal);
    }

    #[test]
    fn skips_entries_a_user_cannot_launch() {
        let parse = |body: &str| parse_desktop("a.desktop", Path::new("a.desktop"), body);
        assert!(parse("[Desktop Entry]\nType=Application\nName=A\nExec=a\n").is_some());
        assert!(parse("[Desktop Entry]\nType=Application\nName=A\nExec=a\nNoDisplay=true\n").is_none());
        assert!(parse("[Desktop Entry]\nType=Application\nName=A\nExec=a\nHidden=true\n").is_none());
        assert!(parse("[Desktop Entry]\nType=Link\nName=A\nURL=http://x\n").is_none());
        assert!(parse("[Desktop Entry]\nType=Application\nName=A\n").is_none());
        assert!(parse("[Desktop Entry]\nType=Application\nExec=a\n").is_none());
        assert!(parse("[Other]\nType=Application\nName=A\nExec=a\n").is_none());
        let regional = parse("[Desktop Entry]\nType=Application\nName=A\nName[ar_EG]=ا\nName[arz]=x\nExec=a\n").unwrap();
        assert_eq!(regional.arabic_names, ["ا"]);
    }

    #[test]
    fn exec_is_split_without_a_shell() {
        let split = |s: &str| parse_exec(s);
        assert_eq!(split("gedit %U"), Some(vec!["gedit".into()]));
        assert_eq!(split("env A=b  prog --opt=%f x"), Some(vec!["env".into(), "A=b".into(), "prog".into(), "--opt=".into(), "x".into()]));
        assert_eq!(split(r#""/opt/my app/run" "a \"quoted\" \$HOME" 100%%"#), Some(vec!["/opt/my app/run".into(), r#"a "quoted" $HOME"#.into(), "100%".into()]));
        // Shell syntax stays plain text in the arguments.
        assert_eq!(split("sh -c rm;ls"), Some(vec!["sh".into(), "-c".into(), "rm;ls".into()]));
        assert_eq!(split(""), None);
        assert_eq!(split("%f"), None);
        assert_eq!(split("\"open"), None);
        assert_eq!(split("a %z"), None);
    }

    #[test]
    fn loads_desktop_files_from_directories() {
        let tmp = TempDir::new();
        let (user, system) = (tmp.0.join("user"), tmp.0.join("system"));
        fs::create_dir_all(&user).unwrap();
        fs::create_dir_all(&system).unwrap();
        fs::write(system.join("calc.desktop"), CALCULATOR).unwrap();
        fs::write(system.join("edit.desktop"), "[Desktop Entry]\nType=Application\nName=Edit\nExec=edit\n").unwrap();
        fs::write(system.join("notes.txt"), CALCULATOR).unwrap();
        fs::write(system.join("bad.desktop"), [0xff, 0xfe, 0x00]).unwrap();
        // The user's copy hides the system one.
        fs::write(user.join("edit.desktop"), "[Desktop Entry]\nType=Application\nName=Edit\nExec=edit\nNoDisplay=true\n").unwrap();
        fs::write(user.join("mine.desktop"), "[Desktop Entry]\nType=Application\nName=Mine\nExec=mine\n").unwrap();

        let table = AppTable::load(&[user, tmp.0.join("missing"), system.clone()]);
        let ids: Vec<_> = table.apps().iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["mine.desktop", "calc.desktop"]);
        assert_eq!(table.apps()[1].desktop_file, system.join("calc.desktop"));
    }

    #[test]
    fn finds_apps_by_either_language() {
        let table = table();
        let id = |q: &str| table.find(q).map(|a| a.id.clone());
        assert_eq!(id("Calculator").unwrap(), "org.gnome.Calculator.desktop");
        assert_eq!(id("  calculator ").unwrap(), "org.gnome.Calculator.desktop");
        assert_eq!(id("firefox web browser").unwrap(), "firefox.desktop");
        assert_eq!(id("متصفح فايرفوكس").unwrap(), "firefox.desktop");
        assert_eq!(id("محرر النصوص").unwrap(), "org.gnome.TextEditor.desktop");
        assert_eq!(id("محرّر النصوص").unwrap(), "org.gnome.TextEditor.desktop");
        assert_eq!(id("الملفات").unwrap(), "org.gnome.Files.desktop");
        // Words that only one app name has are enough; shared words are not.
        assert_eq!(id("firefox").unwrap(), "firefox.desktop");
        assert!(matches!(id("text"), Err(OpenError::NotFound { .. })));
        assert_eq!(id(" "), Err(OpenError::EmptyName));
    }

    #[test]
    fn finds_apps_through_aliases() {
        let table = table();
        let id = |q: &str| table.find(q).unwrap().id.as_str();
        for q in ["calculator", "calc", "الآلة الحاسبة", "الاله الحاسبه", "Calculator"] {
            assert_eq!(id(q), "org.gnome.Calculator.desktop", "{q}");
        }
        for q in ["email", "Email", "البريد الإلكتروني", "البريد الالكتروني", "mail"] {
            assert_eq!(id(q), "org.mozilla.Thunderbird.desktop", "{q}");
        }
        for q in ["music", "الموسيقى", "الموسيقي"] {
            assert_eq!(id(q), "org.gnome.Rhythmbox3.desktop", "{q}");
        }
        // No such app installed: the alias alone is not a match.
        let bare = AppTable::new(vec![app("firefox.desktop", "Firefox", &[])]);
        assert!(matches!(bare.find("email"), Err(OpenError::NotFound { .. })));
    }

    #[test]
    fn unknown_app_lists_the_closest_names() {
        let table = table();
        match table.find("Firefx") {
            Err(OpenError::NotFound { closest }) => {
                assert_eq!(closest.len(), 3);
                assert_eq!(closest[0], "Firefox Web Browser");
            }
            other => panic!("{other:?}"),
        }
        match table.find("rhythmbx") {
            Err(OpenError::NotFound { closest }) => assert_eq!(closest[0], "Rhythmbox"),
            other => panic!("{other:?}"),
        }
        match AppTable::new(Vec::new()).find("x") {
            Err(OpenError::NotFound { closest }) => assert!(closest.is_empty()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn model_text_is_never_run() {
        let table = table();
        let launcher = FakeLauncher::default();
        for text in ["calc; rm -rf ~", "$(reboot)", "gnome-calculator && curl evil", "/bin/sh", "../../bin/sh", "calculator\n--evil"] {
            assert!(matches!(table.open(text, &launcher), Err(OpenError::NotFound { .. })), "{text}");
        }
        assert!(launcher.launched.borrow().is_empty());
    }

    #[test]
    fn opening_hands_the_matched_app_to_the_launcher() {
        let table = table();
        let launcher = FakeLauncher::default();
        let opened = table.open("الموسيقى", &launcher).unwrap();
        assert_eq!(opened.name, "Rhythmbox");
        assert_eq!(*launcher.launched.borrow(), ["org.gnome.Rhythmbox3.desktop"]);

        let failing = FakeLauncher { fail: true, ..FakeLauncher::default() };
        assert_eq!(table.open("calculator", &failing), Err(OpenError::Launch(LaunchError("no display".into()))));
    }

    #[test]
    fn execute_runs_only_the_safe_tools() {
        let fx = Fixture::new();
        touch(&fx.home.join("Documents/report.txt"));
        let (files, apps, launcher) = (fx.sandbox(), table(), FakeLauncher::default());
        let run = |call: ToolCall| execute(&call, &files, &apps, &launcher);

        match run(ToolCall::ListFiles { path: "المستندات".into() }) {
            Ok(Outcome::Listing(l)) => assert_eq!(names(&l.entries), ["report.txt"]),
            other => panic!("{other:?}"),
        }
        match run(ToolCall::SearchFiles { query: "REPORT".into() }) {
            Ok(Outcome::Search(s)) => assert_eq!(names(&s.matches), ["report.txt"]),
            other => panic!("{other:?}"),
        }
        match run(ToolCall::OpenApp { name: "الآلة الحاسبة".into() }) {
            Ok(Outcome::Opened(a)) => assert_eq!(a.name, "Calculator"),
            other => panic!("{other:?}"),
        }
        assert_eq!(run(ToolCall::ListFiles { path: "/".into() }), Err(ActionError::File(FileError::OutsideRoot)));
        assert_eq!(run(ToolCall::CurrentTime { timezone: None }), Err(ActionError::Unsupported));
        assert_eq!(*launcher.launched.borrow(), ["org.gnome.Calculator.desktop"]);
    }

    #[test]
    fn edit_distance_counts_changes() {
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("firefx", "firefox"), 1);
        assert_eq!(edit_distance("كتاب", "كتب"), 1);
        assert_eq!(edit_distance("same", "same"), 0);
    }
}
