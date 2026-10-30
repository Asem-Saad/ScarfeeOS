//! Text to speech through an external program. Standard library only.
//!
//! The only verified permissive Arabic and English voice is SILMA TTS (see
//! docs/models.md), which is a Python program, so this module does not link a
//! model: `CommandSpeaker` runs a configurable program that turns text into a
//! WAV file, then plays the file with `pw-play` if found, else `aplay`.
//!
//! Programs always run as an argument list, never through a shell. The text
//! never goes into an argument: it goes to the program's stdin, or to a private
//! file whose path replaces `{in}` in the arguments. The other placeholders are
//! `{out}` (the WAV the program must write, required) and `{lang}` (`en` or
//! `ar`). The text is the prepared sentences, one per line, so a program can
//! load its model once and still read sentence by sentence.
//!
//! The text and the WAV live in a private directory (mode 0700, files 0600)
//! that is removed when the call ends, on success, on error and on panic.
//!
//! No real voice has been run through this module: the tests use fake programs.
//! Speech to text is in `speech.rs` and recording is in `voice.rs`.

use std::fmt;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{ErrorKind, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::lang::Language;

/// Longest single spoken sentence, in characters.
pub const MAX_SENTENCE_CHARS: usize = 200;
/// Most text spoken in one reply, in characters. The rest is dropped at a sentence end.
pub const MAX_SPOKEN_CHARS: usize = 1200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TtsError {
    /// After cleaning, the text has nothing to say. No program was run.
    NothingToSay,
    /// The arguments are unusable (for example no `{out}`). No program was run.
    InvalidConfig(String),
    /// The speech program does not exist or is not on `PATH`.
    SynthesizerNotFound(PathBuf),
    /// Neither `pw-play` nor `aplay` was found.
    NoPlayer,
    /// A program could not be started or read, or a temporary file could not be made.
    Io(String),
    /// The speech program ended with a failure status (`None` when killed by a signal).
    SynthesizerFailed { code: Option<i32>, stderr: String },
    /// The speech program was still running at the time limit and was killed.
    SynthesizerTimeout(Duration),
    /// The speech program succeeded but wrote no audio.
    NoAudio,
    /// The speech program succeeded but its output file is not a WAV file.
    NotWav,
    /// The player ended with a failure status (`None` when killed by a signal).
    PlayerFailed { code: Option<i32>, stderr: String },
    /// The player was still running at the time limit and was killed.
    PlayerTimeout(Duration),
}

impl fmt::Display for TtsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TtsError::NothingToSay => write!(f, "nothing to say after cleaning the text"),
            TtsError::InvalidConfig(why) => write!(f, "speech program setup is invalid: {why}"),
            TtsError::SynthesizerNotFound(p) => write!(f, "text-to-speech program not found: {}", p.display()),
            TtsError::NoPlayer => write!(f, "no audio player found (tried pw-play and aplay)"),
            TtsError::Io(why) => write!(f, "text to speech failed to run: {why}"),
            TtsError::SynthesizerFailed { code: Some(code), stderr } => {
                write!(f, "text-to-speech program exited with status {code}: {stderr}")
            }
            TtsError::SynthesizerFailed { code: None, stderr } => {
                write!(f, "text-to-speech program was killed by a signal: {stderr}")
            }
            TtsError::SynthesizerTimeout(limit) => {
                write!(f, "text-to-speech program did not finish within {limit:?}")
            }
            TtsError::NoAudio => write!(f, "text-to-speech program wrote no audio"),
            TtsError::NotWav => write!(f, "text-to-speech program did not write a WAV file"),
            TtsError::PlayerFailed { code: Some(code), stderr } => {
                write!(f, "audio player exited with status {code}: {stderr}")
            }
            TtsError::PlayerFailed { code: None, stderr } => {
                write!(f, "audio player was killed by a signal: {stderr}")
            }
            TtsError::PlayerTimeout(limit) => write!(f, "audio player did not finish within {limit:?}"),
        }
    }
}

impl std::error::Error for TtsError {}

pub trait Speaker {
    /// Says `text` aloud in `language`. Returns when the audio has finished playing.
    fn speak(&self, text: &str, language: Language) -> Result<(), TtsError>;
}

/// Runs a text-to-speech program, then plays the WAV it wrote.
#[derive(Debug, Clone)]
pub struct CommandSpeaker {
    program: PathBuf,
    args: Vec<String>,
    timeout: Duration,
    play_timeout: Duration,
    /// Directories to look for the players in. None means the `PATH` variable.
    search: Option<Vec<PathBuf>>,
}

impl CommandSpeaker {
    /// Loading a model and synthesizing on a laptop CPU can be slow.
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
    pub const DEFAULT_PLAY_TIMEOUT: Duration = Duration::from_secs(300);

    /// `program` is a path, or a bare name looked up in `PATH`. `args` is the
    /// argument list, with `{out}` (required), `{in}` and `{lang}` replaced
    /// inside each argument. Without `{in}` the text goes to stdin.
    pub fn new(program: impl Into<PathBuf>, args: Vec<String>) -> Self {
        CommandSpeaker {
            program: program.into(),
            args,
            timeout: Self::DEFAULT_TIMEOUT,
            play_timeout: Self::DEFAULT_PLAY_TIMEOUT,
            search: None,
        }
    }

    /// Time limit for the speech program.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Time limit for playing the audio.
    pub fn with_play_timeout(mut self, timeout: Duration) -> Self {
        self.play_timeout = timeout;
        self
    }

    /// Where to look for `pw-play` and `aplay`, instead of `PATH`.
    pub fn with_search_path(mut self, dirs: Vec<PathBuf>) -> Self {
        self.search = Some(dirs);
        self
    }

    fn find_player(&self) -> Option<PathBuf> {
        let dirs = match &self.search {
            Some(dirs) => dirs.clone(),
            None => std::env::split_paths(&std::env::var_os("PATH")?).collect(),
        };
        ["pw-play", "aplay"]
            .iter()
            .find_map(|name| dirs.iter().map(|d| d.join(name)).find(|p| is_executable(p)))
    }
}

impl Speaker for CommandSpeaker {
    fn speak(&self, text: &str, language: Language) -> Result<(), TtsError> {
        if !self.args.iter().any(|a| a.contains("{out}")) {
            return Err(TtsError::InvalidConfig("the arguments need an {out} for the WAV file".to_string()));
        }
        let sentences = prepare_for_speech(text, language);
        if sentences.is_empty() {
            return Err(TtsError::NothingToSay);
        }
        let player = self.find_player().ok_or(TtsError::NoPlayer)?;
        let mut body = sentences.join("\n");
        body.push('\n');

        let temp = TempDir::new()?;
        let uses_file = self.args.iter().any(|a| a.contains("{in}"));
        let text_file = temp.dir.join("text.txt");
        let wav = temp.dir.join("speech.wav");
        create_private(&wav)?;
        if uses_file {
            create_private(&text_file)?;
            fs::write(&text_file, &body).map_err(|e| TtsError::Io(format!("temporary file: {e}")))?;
        }

        let (in_path, out_path) = (path_str(&text_file)?, path_str(&wav)?);
        let mut command = Command::new(&self.program);
        for arg in &self.args {
            command.arg(arg.replace("{in}", &in_path).replace("{out}", &out_path).replace("{lang}", language.code()));
        }
        let stdin = if uses_file { None } else { Some(body.into_bytes()) };
        match run(command, stdin, self.timeout) {
            Ok(Ran::Exited { status, .. }) if status.success() => {}
            Ok(Ran::Exited { status, stderr }) => {
                return Err(TtsError::SynthesizerFailed { code: status.code(), stderr })
            }
            Ok(Ran::TimedOut) => return Err(TtsError::SynthesizerTimeout(self.timeout)),
            Err(e) if e.kind() == ErrorKind::NotFound => {
                return Err(TtsError::SynthesizerNotFound(self.program.clone()))
            }
            Err(e) => return Err(TtsError::Io(format!("{}: {e}", self.program.display()))),
        }
        check_wav(&wav)?;

        let mut command = Command::new(&player);
        command.arg(&wav);
        match run(command, None, self.play_timeout) {
            Ok(Ran::Exited { status, .. }) if status.success() => Ok(()),
            Ok(Ran::Exited { status, stderr }) => Err(TtsError::PlayerFailed { code: status.code(), stderr }),
            Ok(Ran::TimedOut) => Err(TtsError::PlayerTimeout(self.play_timeout)),
            Err(e) => Err(TtsError::Io(format!("{}: {e}", player.display()))),
        }
    }
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn path_str(path: &Path) -> Result<String, TtsError> {
    path.to_str().map(str::to_string).ok_or_else(|| TtsError::Io("temporary path is not valid UTF-8".to_string()))
}

fn create_private(path: &Path) -> Result<(), TtsError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    options
        .open(path)
        .map(|_| ())
        .map_err(|e| TtsError::Io(format!("temporary file: {e}")))
}

/// The file must hold a RIFF/WAVE header.
fn check_wav(path: &Path) -> Result<(), TtsError> {
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::NotFound => return Err(TtsError::NoAudio),
        Err(e) => return Err(TtsError::Io(format!("reading the WAV file: {e}"))),
    };
    if file.metadata().map(|m| m.len()).unwrap_or(0) == 0 {
        return Err(TtsError::NoAudio);
    }
    let mut head = [0u8; 12];
    if file.read_exact(&mut head).is_err() || &head[..4] != b"RIFF" || &head[8..] != b"WAVE" {
        return Err(TtsError::NotWav);
    }
    Ok(())
}

/// A private directory, removed with everything in it on drop.
struct TempDir {
    dir: PathBuf,
}

static NEXT_TEMP: AtomicU32 = AtomicU32::new(0);

impl TempDir {
    /// Prefers `XDG_RUNTIME_DIR` (a per-user directory in memory) over the shared temp directory.
    fn new() -> Result<TempDir, TtsError> {
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .unwrap_or_else(std::env::temp_dir);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let dir = base.join(format!(
            "scarfeeos-tts-{}-{}-{nanos}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::SeqCst)
        ));
        // Fails if the name exists, so another user cannot pre-create it.
        let mut builder = DirBuilder::new();
        #[cfg(unix)]
        builder.mode(0o700);
        builder.create(&dir).map_err(|e| TtsError::Io(format!("temporary directory: {e}")))?;
        Ok(TempDir { dir })
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

enum Ran {
    Exited { status: ExitStatus, stderr: String },
    TimedOut,
}

/// Runs `command` with `stdin` (or an empty stdin), discarding stdout, and kills it at `timeout`.
fn run(mut command: Command, stdin: Option<Vec<u8>>, timeout: Duration) -> std::io::Result<Ran> {
    command.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
    command.stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = spawn(&mut command)?;
    // Written on its own thread so a program that never reads cannot block the time limit.
    if let (Some(data), Some(mut pipe)) = (stdin, child.stdin.take()) {
        thread::spawn(move || {
            let _ = pipe.write_all(&data);
        });
    }
    let err_reader = drain(child.stderr.take());

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                // Not joining the reader: a child of the killed program could
                // still hold the pipe open.
                let _ = child.kill();
                let _ = child.wait();
                return Ok(Ran::TimedOut);
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        }
    };
    let stderr = stderr_tail(&err_reader.join().unwrap_or_default());
    Ok(Ran::Exited { status, stderr })
}

/// Starts the program, trying again briefly on "text file busy": a program that
/// was only just written can still be open for writing in a forked process.
fn spawn(command: &mut Command) -> std::io::Result<std::process::Child> {
    const ETXTBSY: i32 = 26;
    let mut tries = 0;
    loop {
        match command.spawn() {
            Err(e) if e.raw_os_error() == Some(ETXTBSY) && tries < 20 => {
                tries += 1;
                thread::sleep(Duration::from_millis(10));
            }
            result => return result,
        }
    }
}

fn drain<R: Read + Send + 'static>(stream: Option<R>) -> JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut stream) = stream {
            let _ = stream.read_to_end(&mut bytes);
        }
        bytes
    })
}

fn stderr_tail(bytes: &[u8]) -> String {
    const KEEP: usize = 500;
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim();
    let skip = text.chars().count().saturating_sub(KEEP);
    text.chars().skip(skip).collect()
}

// ---- preparing text for speech ----

/// Spoken in place of a link. Arabic: "link" (for native review).
fn link_word(language: Language) -> &'static str {
    match language {
        Language::English => "link",
        Language::Arabic => "رابط",
    }
}

/// Spoken in place of code. Arabic: "code snippet" (for native review).
fn code_word(language: Language) -> &'static str {
    match language {
        Language::English => "code snippet",
        Language::Arabic => "مقطع برمجي",
    }
}

/// Turns a reply written for the screen into sentences to be spoken: Markdown
/// markers are removed, links and code become a short word or are dropped, and
/// the text is split into sentences on Latin and Arabic punctuation and at line
/// ends. Digits, in either script, and the marks inside numbers (`3.14`,
/// `10:30`, `1,000`, `٣٫١٤`) are left as they are. No sentence is longer than
/// `MAX_SENTENCE_CHARS`, and the whole is cut at a sentence end once it passes
/// `MAX_SPOKEN_CHARS`. Empty when there is nothing to say.
pub fn prepare_for_speech(text: &str, language: Language) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut in_fence = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            let fence = &line[..3];
            if in_fence {
                in_fence = false;
            } else {
                lines.push(code_word(language).to_string());
                // A block opened and closed on one line stays closed.
                in_fence = !line[3..].contains(fence);
            }
            continue;
        }
        if in_fence || is_rule_or_table_divider(line) {
            continue;
        }
        lines.push(clean_line(line, language));
    }

    let mut sentences: Vec<String> = Vec::new();
    let mut total = 0;
    'lines: for line in &lines {
        for sentence in split_sentences(line) {
            for piece in split_long(&sentence, MAX_SENTENCE_CHARS) {
                let len = piece.chars().count();
                if !sentences.is_empty() && total + len > MAX_SPOKEN_CHARS {
                    break 'lines;
                }
                total += len;
                sentences.push(piece);
            }
        }
    }
    sentences
}

/// `---`, `***`, `___` and table rows like `|---|:--:|`.
fn is_rule_or_table_divider(line: &str) -> bool {
    line.chars().count() >= 3 && line.chars().all(|c| matches!(c, '-' | '*' | '_' | '|' | ':' | '=' | ' '))
}

fn clean_line(line: &str, language: Language) -> String {
    let line = strip_block_marker(line);
    let line = replace_inline_code(line, language);
    let line = replace_links(&line, language);
    let line = replace_urls(&line, language);
    strip_emphasis(&line)
}

/// Heading hashes, quote marks, bullets and list numbers at the start of a line.
fn strip_block_marker(line: &str) -> &str {
    let mut line = line.trim_start();
    while let Some(rest) = line.strip_prefix('>') {
        line = rest.trim_start();
    }
    let hashes = line.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&hashes) && line[hashes..].starts_with(' ') {
        return line[hashes..].trim_start();
    }
    for bullet in ["- ", "* ", "+ ", "• "] {
        if let Some(rest) = line.strip_prefix(bullet) {
            return rest.trim_start();
        }
    }
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if (1..=3).contains(&digits) {
        let after = &line[digits..];
        for mark in [". ", ") "] {
            if let Some(rest) = after.strip_prefix(mark) {
                return rest.trim_start();
            }
        }
    }
    line
}

/// `` `x` `` keeps a short plain word, anything else becomes the code word.
fn replace_inline_code(line: &str, language: Language) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(start) = rest.find('`') {
        out.push_str(&rest[..start]);
        let ticks = rest[start..].chars().take_while(|&c| c == '`').count();
        let after = &rest[start + ticks..];
        match after.find(&"`".repeat(ticks)) {
            Some(end) => {
                let code = after[..end].trim();
                let plain = !code.is_empty()
                    && code.chars().count() <= 30
                    && code.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '.' | '_' | '-' | '+'));
                out.push_str(if plain { code } else { code_word(language) });
                rest = &after[end + ticks..];
            }
            // An unclosed backtick is just a stray mark.
            None => rest = after,
        }
    }
    out.push_str(rest);
    out
}

/// `[text](url)` becomes `text`, and `![alt](url)` becomes `alt`.
fn replace_links(line: &str, language: Language) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let target = after.find("](").and_then(|close| {
            let url_start = close + 2;
            after[url_start..].find(')').map(|end| (close, url_start + end))
        });
        match target {
            Some((close, url_end)) if !after[..close].contains('[') => {
                let before = &rest[..open];
                out.push_str(before.strip_suffix('!').unwrap_or(before));
                let label = after[..close].trim();
                out.push_str(if label.is_empty() || looks_like_url(label) { link_word(language) } else { label });
                rest = &after[url_end + 1..];
            }
            _ => {
                out.push_str(&rest[..open + 1]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn looks_like_url(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    ["http://", "https://", "ftp://", "www.", "mailto:", "file://"].iter().any(|p| lower.starts_with(p))
}

fn is_sentence_mark(c: char) -> bool {
    matches!(c, '.' | ',' | ';' | '!' | '?' | '،' | '؛' | '؟' | '…')
}

fn is_edge_mark(c: char) -> bool {
    is_sentence_mark(c) || matches!(c, ':' | '(' | ')' | '[' | ']' | '<' | '>' | '"' | '\'' | '«' | '»' | '“' | '”')
}

/// A web address becomes the link word. Quotes and brackets around it are
/// dropped; the sentence marks after it stay.
fn replace_urls(line: &str, language: Language) -> String {
    let mut words = Vec::new();
    for word in line.split_whitespace() {
        if looks_like_url(word.trim_matches(is_edge_mark)) {
            let end = word.trim_end_matches(is_edge_mark).len();
            let tail: String = word[end..].chars().filter(|&c| is_sentence_mark(c)).collect();
            words.push(format!("{}{tail}", link_word(language)));
        } else {
            words.push(word.to_string());
        }
    }
    words.join(" ")
}

/// Removes `*`, `~~`, table bars and `_` where it marks emphasis (not inside
/// words like `snake_case`), along with bidirectional control marks.
fn strip_emphasis(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '*' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' => {}
            '~' if chars.get(i + 1) == Some(&'~') || (i > 0 && chars[i - 1] == '~') => {}
            '_' => {
                let inside_word = i > 0
                    && chars[i - 1].is_alphanumeric()
                    && chars.get(i + 1).is_some_and(|n| n.is_alphanumeric());
                if inside_word {
                    out.push(c);
                }
            }
            '|' => out.push(' '),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn is_terminator(c: char) -> bool {
    matches!(c, '.' | '!' | '?' | ';' | '…' | '؟' | '؛' | '۔')
}

fn is_closer(c: char) -> bool {
    matches!(c, ')' | ']' | '"' | '\'' | '»' | '”' | '’')
}

/// Splits one line at sentence ends. A mark only ends a sentence before a space
/// or the end, so `3.14` and `file.txt` stay whole.
fn split_sentences(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut sentences = Vec::new();
    let mut current = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        current.push(c);
        i += 1;
        if !is_terminator(c) {
            continue;
        }
        while i < chars.len() && (is_terminator(chars[i]) || is_closer(chars[i])) {
            current.push(chars[i]);
            i += 1;
        }
        if i == chars.len() || chars[i].is_whitespace() {
            push_sentence(&mut sentences, &mut current);
        }
    }
    push_sentence(&mut sentences, &mut current);
    sentences
}

fn push_sentence(sentences: &mut Vec<String>, current: &mut String) {
    let sentence = current.split_whitespace().collect::<Vec<_>>().join(" ");
    current.clear();
    if sentence.chars().any(|c| c.is_alphanumeric()) {
        sentences.push(sentence);
    }
}

/// Splits a sentence longer than `max` characters at a clause mark, else at a
/// space, else in the middle of a word.
fn split_long(sentence: &str, max: usize) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut rest: Vec<char> = sentence.chars().collect();
    while rest.len() > max {
        let window = &rest[..max];
        let cut = window
            .iter()
            .rposition(|&c| matches!(c, ',' | '،' | ':'))
            .filter(|&p| rest.get(p + 1).is_some_and(|n| n.is_whitespace()))
            .map(|p| p + 1)
            .or_else(|| window.iter().rposition(|c| c.is_whitespace()))
            .filter(|&p| p > 0)
            .unwrap_or(max);
        let piece: String = rest[..cut].iter().collect();
        let piece = piece.trim();
        if piece.chars().any(|c| c.is_alphanumeric()) {
            pieces.push(piece.to_string());
        }
        rest = rest[cut..].iter().copied().skip_while(|c| c.is_whitespace()).collect();
    }
    let tail: String = rest.iter().collect();
    if tail.chars().any(|c| c.is_alphanumeric()) {
        pieces.push(tail);
    }
    pieces
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;

    fn en(text: &str) -> Vec<String> {
        prepare_for_speech(text, Language::English)
    }

    fn ar(text: &str) -> Vec<String> {
        prepare_for_speech(text, Language::Arabic)
    }

    #[test]
    fn plain_sentences_split_on_latin_punctuation() {
        assert_eq!(en("Hello there. How are you? Fine!"), ["Hello there.", "How are you?", "Fine!"]);
        assert_eq!(en("One... two"), ["One...", "two"]);
        assert_eq!(en("She said \"yes.\" Then left."), ["She said \"yes.\"", "Then left."]);
        assert_eq!(en("First line\nSecond line"), ["First line", "Second line"]);
    }

    #[test]
    fn arabic_punctuation_splits_sentences() {
        assert_eq!(ar("مرحبا بك. كيف حالك؟ بخير؛ شكرا"), ["مرحبا بك.", "كيف حالك؟", "بخير؛", "شكرا"]);
        // The Arabic comma is not a sentence end.
        assert_eq!(ar("أولا، ثانيا"), ["أولا، ثانيا"]);
    }

    #[test]
    fn digits_stay_intact_in_both_scripts() {
        let text = "Pi is 3.14 and the call is at 10:30 for 1,000 people in 2026.";
        assert_eq!(en(text), [text]);
        let text = "العدد ٣٫١٤ والسنة ٢٠٢٦ والموعد 10:30.";
        assert_eq!(ar(text), [text]);
        assert_eq!(en("Open report.pdf now."), ["Open report.pdf now."]);
    }

    #[test]
    fn markdown_markers_are_removed() {
        assert_eq!(
            en("# Title\n\nSome **bold** and *italic* and ~~gone~~ text."),
            ["Title", "Some bold and italic and gone text."]
        );
        assert_eq!(en("- one\n- two\n* three\n1. four\n2) five"), ["one", "two", "three", "four", "five"]);
        assert_eq!(en("> quoted words"), ["quoted words"]);
        assert_eq!(en("Text\n---\nmore\n|---|:--:|"), ["Text", "more"]);
        assert_eq!(en("| a | b |"), ["a b"]);
        assert_eq!(ar("## عنوان\n**مهم** جدا"), ["عنوان", "مهم جدا"]);
    }

    #[test]
    fn underscores_inside_words_are_kept() {
        assert_eq!(en("Use my_file_name and _emphasis_ here"), ["Use my_file_name and emphasis here"]);
    }

    #[test]
    fn links_become_their_text_or_a_short_word() {
        assert_eq!(en("See [the docs](https://example.com/a?b=c) for more."), ["See the docs for more."]);
        assert_eq!(en("![a cat](cat.png) here"), ["a cat here"]);
        assert_eq!(
            en("Go to https://example.com/path?x=1. Then (www.example.org) or <http://a.b/c>, ok"),
            ["Go to link.", "Then link or link, ok"]
        );
        assert_eq!(ar("زر https://example.com/x، ثم عد"), ["زر رابط، ثم عد"]);
        assert_eq!(en("[https://example.com](https://example.com)"), ["link"]);
        // Brackets that are not links are left alone.
        assert_eq!(en("Item [3] and (maybe) done"), ["Item [3] and (maybe) done"]);
    }

    #[test]
    fn code_becomes_a_placeholder_or_a_plain_word() {
        assert_eq!(
            en("Before\n```rust\nfn main() { println!(\"hi\"); }\n```\nAfter"),
            ["Before", "code snippet", "After"]
        );
        assert_eq!(en("Before\n~~~\nunclosed code"), ["Before", "code snippet"]);
        assert_eq!(en("One\n```x = 1```\nTwo"), ["One", "code snippet", "Two"]);
        assert_eq!(en("Run `ls -la /tmp` then open `notes.txt` now."), ["Run code snippet then open notes.txt now."]);
        assert_eq!(en("Call ``a`b`` then `x"), ["Call code snippet then x"]);
        assert_eq!(ar("شغل `rm -rf ~/x` الآن"), ["شغل مقطع برمجي الآن"]);
        assert_eq!(ar("```\ncode\n```"), ["مقطع برمجي"]);
    }

    #[test]
    fn bidi_marks_and_control_characters_are_removed() {
        assert_eq!(ar("\u{202B}مرحبا\u{202C} \u{200F}بك"), ["مرحبا بك"]);
        assert_eq!(en("a\u{7}b"), ["a b"]);
    }

    #[test]
    fn nothing_to_say_gives_nothing() {
        for text in ["", "   \n\n", "---", "...", "* * *", "!!!", "**", "# "] {
            assert!(en(text).is_empty(), "{text:?} gave {:?}", en(text));
        }
    }

    #[test]
    fn long_sentences_are_split_and_never_exceed_the_limit() {
        let words = vec!["word"; 120].join(" ");
        let pieces = en(&words);
        assert!(pieces.len() > 1);
        assert!(pieces.iter().all(|p| p.chars().count() <= MAX_SENTENCE_CHARS));
        assert_eq!(pieces.join(" "), words);

        let clauses = format!("{}, {}", "a".repeat(150), "b".repeat(150));
        assert_eq!(en(&clauses), [format!("{},", "a".repeat(150)), "b".repeat(150)]);

        let no_spaces = "ب".repeat(450);
        let lengths: Vec<usize> = ar(&no_spaces).iter().map(|p| p.chars().count()).collect();
        assert_eq!(lengths, [200, 200, 50]);
    }

    #[test]
    fn total_length_is_limited_at_a_sentence_end() {
        let text = "This is a sentence of moderate length. ".repeat(100);
        let pieces = en(&text);
        let total: usize = pieces.iter().map(|p| p.chars().count()).sum();
        assert!(total <= MAX_SPOKEN_CHARS && total > MAX_SPOKEN_CHARS - 40, "{total}");
        assert!(pieces.iter().all(|p| p == "This is a sentence of moderate length."));
        // The first sentence is always kept.
        assert!(!en(&"x ".repeat(5000)).is_empty());
    }

    // ---- fake programs (NOT a real voice, NOT a real player) ----

    /// Writing and starting scripts from parallel threads can fail with
    /// "text file busy", so the tests that do it run one at a time.
    static SCRIPT_LOCK: Mutex<()> = Mutex::new(());
    static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            let dir = std::env::temp_dir()
                .join(format!("scarfeeos-tts-test-{}-{}", std::process::id(), NEXT_DIR.fetch_add(1, Ordering::SeqCst)));
            fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }

        /// A `/bin/sh` script named `name` in the scratch directory.
        fn script(&self, name: &str, body: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            path
        }

        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }

        fn read(&self, name: &str) -> String {
            fs::read_to_string(self.path(name)).unwrap_or_default()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn q(path: &Path) -> String {
        format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"))
    }

    const WAV_BYTES: &str = "RIFFabcdWAVEfmt ";

    /// Fake synthesizer: logs its arguments, copies stdin, writes a tiny fake WAV to `$1`.
    fn good_synth(dir: &Scratch) -> PathBuf {
        dir.script(
            "fake-synth",
            &format!(
                "printf '%s\\n' \"$@\" > {args}\ncat > {stdin}\ndirname \"$1\" > {tempdir}\nstat -c %a \"$(dirname \"$1\")\" > {dirmode}\nstat -c %a \"$1\" > {filemode}\nprintf '{WAV_BYTES}' > \"$1\"",
                args = q(&dir.path("args")),
                stdin = q(&dir.path("stdin")),
                tempdir = q(&dir.path("tempdir")),
                dirmode = q(&dir.path("dirmode")),
                filemode = q(&dir.path("filemode")),
            ),
        )
    }

    /// Fake player: logs its arguments and keeps a copy of the file it was given.
    fn good_player(dir: &Scratch, name: &str) -> PathBuf {
        dir.script(
            name,
            &format!(
                "printf '%s\\n' \"$@\" > {log}\ncat \"$1\" > {copy}",
                log = q(&dir.path(&format!("{name}.log"))),
                copy = q(&dir.path(&format!("{name}.wav"))),
            ),
        )
    }

    fn speaker(dir: &Scratch) -> CommandSpeaker {
        CommandSpeaker::new(good_synth(dir), vec!["{out}".into(), "{lang}".into()]).with_search_path(vec![dir.0.clone()])
    }

    #[test]
    fn text_goes_through_stdin_and_the_wav_is_played_then_removed() {
        let _guard = SCRIPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Scratch::new();
        good_player(&dir, "pw-play");
        speaker(&dir).speak("مرحبا بك. **كيف** حالك؟ See https://example.com now.", Language::Arabic).unwrap();

        let args = dir.read("args");
        let args: Vec<&str> = args.lines().collect();
        assert_eq!(args.len(), 2);
        assert!(args[0].ends_with("/speech.wav"), "{args:?}");
        assert_eq!(args[1], "ar");
        assert!(!args.iter().any(|a| a.contains("مرحبا")), "the text must not be an argument");
        assert_eq!(dir.read("stdin"), "مرحبا بك.\nكيف حالك؟\nSee رابط now.\n");

        // Played the same file the synthesizer wrote.
        assert_eq!(dir.read("pw-play.log").trim(), args[0]);
        assert_eq!(dir.read("pw-play.wav"), WAV_BYTES);

        // Private while it existed, gone afterwards.
        assert_eq!(dir.read("dirmode").trim(), "700");
        assert_eq!(dir.read("filemode").trim(), "600");
        assert!(!Path::new(dir.read("tempdir").trim()).exists());
    }

    #[test]
    fn text_can_go_through_a_private_file_instead() {
        let _guard = SCRIPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Scratch::new();
        good_player(&dir, "pw-play");
        let synth = dir.script(
            "file-synth",
            &format!(
                "printf '%s\\n' \"$@\" > {args}\ncat \"$2\" > {copy}\nstat -c %a \"$2\" > {mode}\ncat > {stdin}\ndirname \"$2\" > {tempdir}\nprintf '{WAV_BYTES}' > \"$4\"",
                args = q(&dir.path("args")),
                copy = q(&dir.path("copy")),
                mode = q(&dir.path("mode")),
                stdin = q(&dir.path("stdin")),
                tempdir = q(&dir.path("tempdir")),
            ),
        );
        let args = ["--text-file", "{in}", "--output", "{out}", "--lang={lang}"].map(String::from).to_vec();
        CommandSpeaker::new(synth, args)
            .with_search_path(vec![dir.0.clone()])
            .speak("Hello world. Bye.", Language::English)
            .unwrap();

        assert_eq!(dir.read("copy"), "Hello world.\nBye.\n");
        assert_eq!(dir.read("stdin"), "", "with the file nothing goes to stdin");
        assert_eq!(dir.read("mode").trim(), "600");
        assert!(dir.read("args").contains("--lang=en"));
        assert!(!dir.read("args").contains("Hello"));
        assert!(!Path::new(dir.read("tempdir").trim()).exists());
    }

    #[test]
    fn shell_characters_in_text_are_never_run() {
        let _guard = SCRIPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Scratch::new();
        good_player(&dir, "pw-play");
        let marker = dir.path("pwned");
        let text = format!("Hi $(touch {m}); && touch {m} | touch {m} \"'", m = q(&marker));
        speaker(&dir).speak(&text, Language::English).unwrap();
        assert!(!marker.exists());
        assert!(dir.read("stdin").contains("$(touch"));
    }

    #[test]
    fn pw_play_is_preferred_and_aplay_is_the_fallback() {
        let _guard = SCRIPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Scratch::new();
        good_player(&dir, "aplay");
        speaker(&dir).speak("Hello.", Language::English).unwrap();
        assert!(!dir.read("aplay.log").is_empty());
        fs::remove_file(dir.path("aplay.log")).unwrap();

        good_player(&dir, "pw-play");
        speaker(&dir).speak("Hello.", Language::English).unwrap();
        assert!(!dir.read("pw-play.log").is_empty());
        assert!(dir.read("aplay.log").is_empty(), "aplay was used although pw-play exists");
    }

    #[test]
    fn no_player_is_its_own_error_and_no_program_runs() {
        let _guard = SCRIPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Scratch::new();
        assert_eq!(speaker(&dir).speak("Hello.", Language::English), Err(TtsError::NoPlayer));
        assert!(!dir.path("args").exists());
    }

    #[test]
    fn nothing_to_say_runs_nothing() {
        let _guard = SCRIPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Scratch::new();
        good_player(&dir, "pw-play");
        assert_eq!(speaker(&dir).speak("  ---  ", Language::English), Err(TtsError::NothingToSay));
        assert!(!dir.path("args").exists());
    }

    #[test]
    fn arguments_without_an_output_place_are_rejected() {
        let s = CommandSpeaker::new("/nonexistent/synth", vec!["--lang".into(), "{lang}".into()]);
        assert!(matches!(s.speak("Hello.", Language::English), Err(TtsError::InvalidConfig(_))));
    }

    #[test]
    fn missing_synthesizer_is_its_own_error() {
        let _guard = SCRIPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Scratch::new();
        good_player(&dir, "pw-play");
        let missing = dir.path("no-such-program");
        let s = CommandSpeaker::new(&missing, vec!["{out}".into()]).with_search_path(vec![dir.0.clone()]);
        assert_eq!(s.speak("Hello.", Language::English), Err(TtsError::SynthesizerNotFound(missing)));
    }

    #[test]
    fn synthesizer_failure_carries_status_and_stderr() {
        let _guard = SCRIPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Scratch::new();
        good_player(&dir, "pw-play");
        let synth = dir.script("bad-synth", "echo 'model weights not found' >&2\nexit 3");
        let s = CommandSpeaker::new(synth, vec!["{out}".into()]).with_search_path(vec![dir.0.clone()]);
        assert_eq!(
            s.speak("Hello.", Language::English),
            Err(TtsError::SynthesizerFailed { code: Some(3), stderr: "model weights not found".to_string() })
        );
        assert!(dir.read("pw-play.log").is_empty(), "nothing is played after a failure");
    }

    #[test]
    fn missing_empty_or_wrong_audio_are_distinct_errors() {
        let _guard = SCRIPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Scratch::new();
        good_player(&dir, "pw-play");
        let run = |body: &str| {
            let synth = dir.script("odd-synth", body);
            CommandSpeaker::new(synth, vec!["{out}".into()])
                .with_search_path(vec![dir.0.clone()])
                .speak("Hello.", Language::English)
        };
        assert_eq!(run("exit 0"), Err(TtsError::NoAudio));
        assert_eq!(run("rm -f \"$1\""), Err(TtsError::NoAudio));
        assert_eq!(run("printf 'ID3 not a wav file' > \"$1\""), Err(TtsError::NotWav));
        assert_eq!(run("printf 'RIFF' > \"$1\""), Err(TtsError::NotWav));
        assert!(dir.read("pw-play.log").is_empty());
    }

    #[test]
    fn a_synthesizer_that_runs_too_long_is_killed_and_cleaned_up() {
        let _guard = SCRIPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Scratch::new();
        good_player(&dir, "pw-play");
        let synth = dir.script(
            "slow-synth",
            &format!(
                "echo $$ > {pid}\ndirname \"$1\" > {tempdir}\nexec sleep 30",
                pid = q(&dir.path("pid")),
                tempdir = q(&dir.path("tempdir"))
            ),
        );
        let s = CommandSpeaker::new(synth, vec!["{out}".into()])
            .with_timeout(Duration::from_millis(300))
            .with_search_path(vec![dir.0.clone()]);
        let started = Instant::now();
        assert_eq!(s.speak("Hello.", Language::English), Err(TtsError::SynthesizerTimeout(Duration::from_millis(300))));
        assert!(started.elapsed() < Duration::from_secs(10));
        let pid = dir.read("pid");
        assert!(!Path::new("/proc").join(pid.trim()).exists(), "process {} still exists", pid.trim());
        assert!(!Path::new(dir.read("tempdir").trim()).exists());
        assert!(dir.read("pw-play.log").is_empty());
    }

    #[test]
    fn player_failure_and_timeout_are_distinct_errors() {
        let _guard = SCRIPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Scratch::new();
        dir.script("pw-play", "echo 'no sound server' >&2\nexit 1");
        assert_eq!(
            speaker(&dir).speak("Hello.", Language::English),
            Err(TtsError::PlayerFailed { code: Some(1), stderr: "no sound server".to_string() })
        );

        dir.script(
            "pw-play",
            &format!("echo $$ > {pid}\ndirname \"$1\" > {tempdir}\nexec sleep 30", pid = q(&dir.path("player-pid")), tempdir = q(&dir.path("player-tempdir"))),
        );
        let s = speaker(&dir).with_play_timeout(Duration::from_millis(300));
        let started = Instant::now();
        assert_eq!(s.speak("Hello.", Language::English), Err(TtsError::PlayerTimeout(Duration::from_millis(300))));
        assert!(started.elapsed() < Duration::from_secs(10));
        let pid = dir.read("player-pid");
        assert!(!Path::new("/proc").join(pid.trim()).exists(), "process {} still exists", pid.trim());
        assert!(!Path::new(dir.read("player-tempdir").trim()).exists());
    }

    #[test]
    fn errors_have_readable_messages() {
        assert_eq!(TtsError::NoPlayer.to_string(), "no audio player found (tried pw-play and aplay)");
        assert!(TtsError::SynthesizerTimeout(Duration::from_secs(2)).to_string().contains("2s"));
        assert!(TtsError::PlayerFailed { code: None, stderr: "x".into() }.to_string().contains("signal"));
    }
}
