//! Speech to text through whisper.cpp, run as a separate program (MIT, see
//! docs/models.md). Standard library only.
//!
//! Flags and output format were read from whisper.cpp v1.9.5 (`examples/cli/cli.cpp`
//! and its README) and checked against a build of that tag, see the tests.
//! The command run is
//! `whisper-cli -m MODEL -f WAV -l LANG -oj -of - -np`: JSON result on
//! stdout (`-of -`), log lines suppressed (`-np`), progress and errors on stderr.
//! Arguments go to the program as a list, never through a shell, and `-m`/`-f`
//! take the next argument whatever it looks like, so odd file names are safe.
//!
//! Microphone capture, wake word and text to speech are not part of this module.

use std::fmt;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::lang::Language;

/// Spoken language passed to whisper.cpp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SttLanguage {
    Arabic,
    English,
    /// whisper.cpp detects the language.
    Auto,
}

impl SttLanguage {
    pub fn code(self) -> &'static str {
        match self {
            SttLanguage::Arabic => "ar",
            SttLanguage::English => "en",
            SttLanguage::Auto => "auto",
        }
    }
}

impl From<Language> for SttLanguage {
    fn from(lang: Language) -> Self {
        match lang {
            Language::Arabic => SttLanguage::Arabic,
            Language::English => SttLanguage::English,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// Milliseconds from the start of the audio. Both are 0 when the output
    /// carries no timestamps.
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    /// All segment texts, trimmed and joined with a space.
    pub text: String,
    /// Language code reported by whisper.cpp (the detected one for `auto`).
    /// None when the output does not say.
    pub language: Option<String>,
    pub segments: Vec<Segment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpeechError {
    /// The whisper-cli program does not exist or is not on `PATH`.
    BinaryNotFound(PathBuf),
    ModelNotFound(PathBuf),
    AudioNotFound(PathBuf),
    /// The program could not be started or read for another reason.
    Io(String),
    /// The program ended with a failure status (`None` when killed by a signal).
    NonZeroExit { code: Option<i32>, stderr: String },
    /// The program was still running when the time limit passed and was killed.
    Timeout(Duration),
    /// The program succeeded but its output could not be understood.
    MalformedOutput(String),
}

impl fmt::Display for SpeechError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpeechError::BinaryNotFound(p) => write!(f, "speech program not found: {}", p.display()),
            SpeechError::ModelNotFound(p) => write!(f, "speech model not found: {}", p.display()),
            SpeechError::AudioNotFound(p) => write!(f, "audio file not found: {}", p.display()),
            SpeechError::Io(why) => write!(f, "speech program failed to run: {why}"),
            SpeechError::NonZeroExit { code: Some(code), stderr } => {
                write!(f, "speech program exited with status {code}: {stderr}")
            }
            SpeechError::NonZeroExit { code: None, stderr } => {
                write!(f, "speech program was killed by a signal: {stderr}")
            }
            SpeechError::Timeout(limit) => write!(f, "speech program did not finish within {limit:?}"),
            SpeechError::MalformedOutput(why) => write!(f, "speech program output not understood: {why}"),
        }
    }
}

impl std::error::Error for SpeechError {}

pub trait Transcriber {
    /// Transcribes a 16 kHz WAV file.
    fn transcribe(&self, wav: &Path, language: SttLanguage) -> Result<Transcript, SpeechError>;
}

/// Runs the `whisper-cli` program from whisper.cpp.
#[derive(Debug, Clone)]
pub struct WhisperCli {
    binary: PathBuf,
    model: PathBuf,
    timeout: Duration,
}

impl WhisperCli {
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

    /// `binary` is a path, or a bare name looked up in `PATH`. `model` is a
    /// ggml model file such as `ggml-small.bin`.
    pub fn new(binary: impl Into<PathBuf>, model: impl Into<PathBuf>) -> Self {
        WhisperCli { binary: binary.into(), model: model.into(), timeout: Self::DEFAULT_TIMEOUT }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl Transcriber for WhisperCli {
    fn transcribe(&self, wav: &Path, language: SttLanguage) -> Result<Transcript, SpeechError> {
        if !self.model.is_file() {
            return Err(SpeechError::ModelNotFound(self.model.clone()));
        }
        if !wav.is_file() {
            return Err(SpeechError::AudioNotFound(wav.to_path_buf()));
        }
        let mut command = Command::new(&self.binary);
        command
            .arg("-m")
            .arg(&self.model)
            .arg("-f")
            .arg(wav)
            .arg("-l")
            .arg(language.code())
            .args(["-oj", "-of", "-", "-np"]);
        let stdout = run_with_timeout(command, &self.binary, self.timeout)?;
        let text = String::from_utf8(stdout)
            .map_err(|_| SpeechError::MalformedOutput("output is not valid UTF-8".to_string()))?;
        parse_output(&text)
    }
}

fn run_with_timeout(
    mut command: Command,
    binary: &Path,
    timeout: Duration,
) -> Result<Vec<u8>, SpeechError> {
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| match e.kind() {
        ErrorKind::NotFound => SpeechError::BinaryNotFound(binary.to_path_buf()),
        _ => SpeechError::Io(format!("{}: {e}", binary.display())),
    })?;
    let out_reader = drain(child.stdout.take());
    let err_reader = drain(child.stderr.take());

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                // Not joining the readers: a child of the killed program could
                // still hold the pipes open.
                let _ = child.kill();
                let _ = child.wait();
                return Err(SpeechError::Timeout(timeout));
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(SpeechError::Io(e.to_string()));
            }
        }
    };
    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();
    if !status.success() {
        return Err(SpeechError::NonZeroExit { code: status.code(), stderr: stderr_tail(&stderr) });
    }
    Ok(stdout)
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

/// The end of stderr (where whisper.cpp prints its `error:` line), as text.
fn stderr_tail(bytes: &[u8]) -> String {
    const KEEP: usize = 500;
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim();
    let skip = text.chars().count().saturating_sub(KEEP);
    text.chars().skip(skip).collect()
}

/// Parses whisper-cli output: the JSON written by `-oj`, or the plain text
/// printed by default (`[00:00:00.000 --> 00:00:10.500]   text` per line, or
/// bare lines with `-nt`). Text output does not name the language.
pub fn parse_output(text: &str) -> Result<Transcript, SpeechError> {
    let malformed = |why: &str| SpeechError::MalformedOutput(why.to_string());
    match text.trim_start().chars().next() {
        None => Err(malformed("empty output")),
        Some('{') => parse_json_output(text),
        Some(_) => parse_text_output(text),
    }
}

fn parse_json_output(text: &str) -> Result<Transcript, SpeechError> {
    let malformed = |why: String| SpeechError::MalformedOutput(why);
    let root = Json::parse(text).map_err(|e| malformed(format!("invalid JSON: {e}")))?;
    let items = match root.get("transcription") {
        Some(Json::Array(items)) => items,
        _ => return Err(malformed("no \"transcription\" array".to_string())),
    };
    let mut segments = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let Some(Json::String(text)) = item.get("text") else {
            return Err(malformed(format!("segment {i} has no text")));
        };
        let offset = |key: &str| match item.get("offsets").and_then(|o| o.get(key)) {
            Some(Json::Number(n)) if *n >= 0.0 && n.is_finite() => Ok(*n as u64),
            _ => Err(malformed(format!("segment {i} has no valid offsets.{key}"))),
        };
        segments.push(Segment { start_ms: offset("from")?, end_ms: offset("to")?, text: text.trim().to_string() });
    }
    let language = match root.get("result").and_then(|r| r.get("language")) {
        Some(Json::String(code)) if !code.is_empty() => Some(code.clone()),
        _ => None,
    };
    Ok(transcript(segments, language))
}

fn parse_text_output(text: &str) -> Result<Transcript, SpeechError> {
    let mut segments = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let stamped = line.strip_prefix('[').and_then(|rest| rest.split_once(']')).filter(|(span, _)| span.contains("-->"));
        let (start_ms, end_ms, body) = match stamped {
            Some((span, body)) => {
                let (from, to) = span.split_once("-->").unwrap_or_default();
                match (parse_timestamp(from), parse_timestamp(to)) {
                    (Some(from), Some(to)) => (from, to, body),
                    _ => return Err(SpeechError::MalformedOutput(format!("bad timestamp in line: {line}"))),
                }
            }
            None => (0, 0, line),
        };
        segments.push(Segment { start_ms, end_ms, text: body.trim().to_string() });
    }
    Ok(transcript(segments, None))
}

/// `hh:mm:ss.mmm` or `hh:mm:ss,mmm` to milliseconds.
fn parse_timestamp(s: &str) -> Option<u64> {
    let (clock, millis) = s.trim().split_once(['.', ','])?;
    let mut parts = clock.split(':');
    let hours: u64 = parts.next()?.parse().ok()?;
    let minutes: u64 = parts.next()?.parse().ok()?;
    let seconds: u64 = parts.next()?.parse().ok()?;
    let millis: u64 = millis.parse().ok().filter(|_| millis.len() == 3)?;
    if parts.next().is_some() || minutes > 59 || seconds > 59 {
        return None;
    }
    Some(((hours * 60 + minutes) * 60 + seconds) * 1000 + millis)
}

fn transcript(segments: Vec<Segment>, language: Option<String>) -> Transcript {
    let text = segments.iter().map(|s| s.text.as_str()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" ");
    Transcript { text, language, segments }
}

/// Minimal JSON reader, enough for the whisper.cpp output. It accepts raw
/// control characters inside strings, because whisper.cpp escapes only `"` and
/// `\` when it writes text.
#[derive(Debug)]
enum Json {
    /// `true`, `false` or `null`: whisper.cpp output is not checked for these.
    Literal,
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn parse(text: &str) -> Result<Json, String> {
        let mut parser = JsonParser { bytes: text.as_bytes(), pos: 0 };
        let value = parser.value(0)?;
        parser.skip_space();
        if parser.pos != parser.bytes.len() {
            return Err(format!("extra data at byte {}", parser.pos));
        }
        Ok(value)
    }
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

const MAX_JSON_DEPTH: usize = 32;

impl JsonParser<'_> {
    fn skip_space(&mut self) {
        while matches!(self.bytes.get(self.pos), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> Result<(), String> {
        self.skip_space();
        if self.bytes.get(self.pos) == Some(&byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("expected '{}' at byte {}", byte as char, self.pos))
        }
    }

    fn word(&mut self, word: &str) -> Result<Json, String> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(Json::Literal)
        } else {
            Err(format!("unexpected data at byte {}", self.pos))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > MAX_JSON_DEPTH {
            return Err("nested too deeply".to_string());
        }
        self.skip_space();
        match self.bytes.get(self.pos) {
            None => Err("unexpected end".to_string()),
            Some(b'{') => {
                self.pos += 1;
                let mut fields = Vec::new();
                if self.close_if(b'}') {
                    return Ok(Json::Object(fields));
                }
                loop {
                    self.skip_space();
                    let key = self.string()?;
                    self.eat(b':')?;
                    fields.push((key, self.value(depth + 1)?));
                    if self.close_if(b'}') {
                        return Ok(Json::Object(fields));
                    }
                    self.eat(b',')?;
                }
            }
            Some(b'[') => {
                self.pos += 1;
                let mut items = Vec::new();
                if self.close_if(b']') {
                    return Ok(Json::Array(items));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    if self.close_if(b']') {
                        return Ok(Json::Array(items));
                    }
                    self.eat(b',')?;
                }
            }
            Some(b'"') => self.string().map(Json::String),
            Some(b't') => self.word("true"),
            Some(b'f') => self.word("false"),
            Some(b'n') => self.word("null"),
            Some(_) => self.number(),
        }
    }

    fn close_if(&mut self, byte: u8) -> bool {
        self.skip_space();
        if self.bytes.get(self.pos) == Some(&byte) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        while matches!(self.bytes.get(self.pos), Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')) {
            self.pos += 1;
        }
        std::str::from_utf8(&self.bytes[start..self.pos])
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .map(Json::Number)
            .ok_or_else(|| format!("bad number at byte {start}"))
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self.bytes.get(self.pos..self.pos + 4).ok_or("short \\u escape")?;
        let digits = std::str::from_utf8(digits).map_err(|_| "bad \\u escape")?;
        self.pos += 4;
        u32::from_str_radix(digits, 16).map_err(|_| "bad \\u escape".to_string())
    }

    fn string(&mut self) -> Result<String, String> {
        if self.bytes.get(self.pos) != Some(&b'"') {
            return Err(format!("expected string at byte {}", self.pos));
        }
        self.pos += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let byte = *self.bytes.get(self.pos).ok_or("unterminated string")?;
            self.pos += 1;
            match byte {
                b'"' => break,
                b'\\' => {
                    let esc = *self.bytes.get(self.pos).ok_or("unterminated string")?;
                    self.pos += 1;
                    let ch = match esc {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let mut code = self.hex4()?;
                            if (0xD800..0xDC00).contains(&code) {
                                if !self.bytes[self.pos..].starts_with(b"\\u") {
                                    return Err("lone surrogate".to_string());
                                }
                                self.pos += 2;
                                let low = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&low) {
                                    return Err("lone surrogate".to_string());
                                }
                                code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                            }
                            char::from_u32(code).ok_or("lone surrogate")?
                        }
                        _ => return Err(format!("bad escape at byte {}", self.pos - 1)),
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                _ => out.push(byte),
            }
        }
        // The input is a &str and only whole characters were copied.
        String::from_utf8(out).map_err(|_| "invalid UTF-8".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU32, Ordering};
    #[cfg(unix)]
    use std::sync::Mutex;

    /// Output of whisper-cli v1.9.5 built from source, ggml-tiny.bin, `-l ar -oj -of - -np`,
    /// on a generated 440 Hz tone (so the Arabic text is the model's guess, not speech).
    const REAL_AR_JSON: &str = "{
\t\"systeminfo\": \"WHISPER : CPU : SSE3 = 1 | AVX2 = 1 | \",
\t\"model\": {
\t\t\"type\": \"tiny\",
\t\t\"multilingual\": true,
\t\t\"vocab\": 51865,
\t\t\"audio\": {
\t\t\t\"ctx\": 1500,
\t\t\t\"state\": 384,
\t\t\t\"head\": 6,
\t\t\t\"layer\": 4
\t\t},
\t\t\"mels\": 80,
\t\t\"ftype\": 1
\t},
\t\"params\": {
\t\t\"model\": \"ggml-tiny.bin\",
\t\t\"language\": \"ar\",
\t\t\"translate\": false
\t},
\t\"result\": {
\t\t\"language\": \"ar\"
\t},
\t\"transcription\": [
\t\t{
\t\t\t\"timestamps\": {
\t\t\t\t\"from\": \"00:00:00,000\",
\t\t\t\t\"to\": \"00:00:02,000\"
\t\t\t},
\t\t\t\"offsets\": {
\t\t\t\t\"from\": 0,
\t\t\t\t\"to\": 2000
\t\t\t},
\t\t\t\"text\": \" [مجالة]\"
\t\t}
\t]
}
";

    /// Output of the same build on whisper.cpp's samples/jfk.wav, default text output.
    const REAL_EN_TEXT: &str = "\n[00:00:00.000 --> 00:00:10.500]   And so, my fellow Americans, ask not what your country can do for you, ask what you can do for your country.\n";

    fn json_with(segments: &str) -> String {
        format!("{{\"result\": {{\"language\": \"ar\"}}, \"transcription\": [{segments}]}}")
    }

    fn seg(text: &str, from: u64, to: u64) -> String {
        format!("{{\"offsets\": {{\"from\": {from}, \"to\": {to}}}, \"text\": \"{text}\"}}")
    }

    #[test]
    fn parses_real_arabic_json() {
        let t = parse_output(REAL_AR_JSON).unwrap();
        assert_eq!(t.language.as_deref(), Some("ar"));
        assert_eq!(t.text, "[مجالة]");
        assert_eq!(t.segments, vec![Segment { start_ms: 0, end_ms: 2000, text: "[مجالة]".to_string() }]);
    }

    #[test]
    fn parses_real_english_text() {
        let t = parse_output(REAL_EN_TEXT).unwrap();
        assert_eq!(t.language, None);
        assert_eq!(t.segments.len(), 1);
        assert_eq!((t.segments[0].start_ms, t.segments[0].end_ms), (0, 10_500));
        assert!(t.text.starts_with("And so, my fellow Americans"));
        assert!(t.text.ends_with("for your country."));
    }

    #[test]
    fn arabic_survives_escapes_quotes_and_several_segments() {
        let arabic = "مرحبا بالعالم، كيف حالك؟";
        let json = json_with(&format!(
            "{}, {}, {}",
            seg(&format!(" {arabic}"), 0, 1500),
            seg(" قال \\\"نعم\\\" ثم \\\\ ذهب", 1500, 3000),
            seg(" \\u0627\\u0644\\u0639\\u0631\\u0628\\u064a\\u0629 \\ud83d\\ude00", 3000, 61_250),
        ));
        let t = parse_output(&json).unwrap();
        assert_eq!(t.segments[0].text, arabic);
        assert_eq!(t.segments[1].text, "قال \"نعم\" ثم \\ ذهب");
        assert_eq!(t.segments[2].text, "العربية \u{1F600}");
        assert_eq!(t.segments[2].end_ms, 61_250);
        assert_eq!(t.text, format!("{arabic} قال \"نعم\" ثم \\ ذهب العربية \u{1F600}"));
    }

    #[test]
    fn accepts_a_raw_newline_inside_text() {
        let json = json_with("{\"offsets\": {\"from\": 0, \"to\": 10}, \"text\": \"سطر\nثان\"}");
        assert_eq!(parse_output(&json).unwrap().segments[0].text, "سطر\nثان");
    }

    #[test]
    fn silence_is_an_empty_transcript_not_an_error() {
        let t = parse_output(&json_with("")).unwrap();
        assert_eq!(t.text, "");
        assert!(t.segments.is_empty());
        assert_eq!(t.language.as_deref(), Some("ar"));
    }

    #[test]
    fn language_is_optional_in_json() {
        let t = parse_output("{\"transcription\": []}").unwrap();
        assert_eq!(t.language, None);
    }

    #[test]
    fn parses_plain_text_without_timestamps() {
        let t = parse_output("مرحبا\n\n  كيف حالك  \n").unwrap();
        assert_eq!(t.text, "مرحبا كيف حالك");
        assert_eq!(t.segments.len(), 2);
        assert_eq!((t.segments[0].start_ms, t.segments[0].end_ms), (0, 0));
    }

    #[test]
    fn text_in_square_brackets_is_not_a_timestamp() {
        let t = parse_output("[BLANK_AUDIO]\n[00:01:02.003 --> 01:00:00.000]  [موسيقى]").unwrap();
        assert_eq!(t.segments[0].text, "[BLANK_AUDIO]");
        assert_eq!((t.segments[1].start_ms, t.segments[1].end_ms), (62_003, 3_600_000));
        assert_eq!(t.segments[1].text, "[موسيقى]");
    }

    #[test]
    fn malformed_output_is_rejected() {
        let bad = [
            "",
            "   \n",
            "{",
            "{\"transcription\": [}",
            "{\"transcription\": {}}",
            "{\"result\": {}}",
            "{\"transcription\": []} trailing",
            "{\"transcription\": [{\"text\": \"x\"}]}",
            "{\"transcription\": [{\"offsets\": {\"from\": 0, \"to\": 1}}]}",
            "{\"transcription\": [{\"offsets\": {\"from\": -5, \"to\": 1}, \"text\": \"x\"}]}",
            "{\"transcription\": [{\"offsets\": {\"from\": 0, \"to\": 1}, \"text\": \"\\ud800\"}]}",
            "{\"transcription\": [{\"offsets\": {\"from\": 0, \"to\": 1}, \"text\": \"\\q\"}]}",
            "{\"transcription\": [{\"offsets\": {\"from\": 0, \"to\": 1}, \"text\": \"open",
            "[00:00:00.000 --> 00:00:xx.000]  text",
            "[00:00:00 --> 00:00:01.000]  text",
        ];
        for text in bad {
            assert!(matches!(parse_output(text), Err(SpeechError::MalformedOutput(_))), "accepted: {text:?}");
        }
        let deep = format!("{{\"a\": {}{}}}", "[".repeat(100), "]".repeat(100));
        assert!(matches!(parse_output(&deep), Err(SpeechError::MalformedOutput(_))));
    }

    #[test]
    fn language_codes() {
        assert_eq!(SttLanguage::from(Language::Arabic).code(), "ar");
        assert_eq!(SttLanguage::from(Language::English).code(), "en");
        assert_eq!(SttLanguage::Auto.code(), "auto");
    }

    // ---- fake whisper-cli (NOT the real program) ----

    /// Writing and starting scripts from parallel threads can fail with
    /// "text file busy", so the tests that do it run one at a time.
    #[cfg(unix)]
    static SCRIPT_LOCK: Mutex<()> = Mutex::new(());
    static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            let dir = std::env::temp_dir()
                .join(format!("scarfeeos-speech-{}-{}", std::process::id(), NEXT_DIR.fetch_add(1, Ordering::SeqCst)));
            fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }

        fn file(&self, name: &str, content: &[u8]) -> PathBuf {
            let path = self.0.join(name);
            fs::write(&path, content).unwrap();
            path
        }

        /// A `/bin/sh` script, so only where scripts can run.
        #[cfg(unix)]
        fn script(&self, body: &str) -> PathBuf {
            let path = self.file("fake-whisper-cli", format!("#!/bin/sh\n{body}\n").as_bytes());
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(unix)]
    fn sh_quote(path: &Path) -> String {
        format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"))
    }

    #[cfg(unix)]
    #[test]
    fn fake_program_gets_an_argument_list_and_arabic_survives() {
        let _guard = SCRIPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Scratch::new();
        let args_file = dir.0.join("args");
        let reply = dir.file("reply.json", REAL_AR_JSON.as_bytes());
        let bin = dir.script(&format!(
            "printf '%s\\n' \"$@\" > {}\ncat {}",
            sh_quote(&args_file),
            sh_quote(&reply)
        ));
        let odd = "odd name; $(touch pwned) `id` \"q\" 'q' * -x.wav";
        let wav = dir.file(odd, b"RIFF");
        let model = dir.file("model with space.bin", b"m");

        let t = WhisperCli::new(&bin, &model).transcribe(&wav, SttLanguage::Arabic).unwrap();
        assert_eq!(t.text, "[مجالة]");
        assert_eq!(t.language.as_deref(), Some("ar"));

        let args = fs::read_to_string(&args_file).unwrap();
        let expected = format!("-m\n{}\n-f\n{}\n-l\nar\n-oj\n-of\n-\n-np\n", model.display(), wav.display());
        assert_eq!(args, expected);
        assert!(!dir.0.join("pwned").exists());
    }

    #[test]
    fn missing_binary_is_its_own_error() {
        let dir = Scratch::new();
        let wav = dir.file("a.wav", b"RIFF");
        let model = dir.file("m.bin", b"m");
        let missing = dir.0.join("no-such-whisper-cli");
        let err = WhisperCli::new(&missing, &model).transcribe(&wav, SttLanguage::Auto).unwrap_err();
        assert_eq!(err, SpeechError::BinaryNotFound(missing));
    }

    #[test]
    fn missing_model_and_audio_are_their_own_errors() {
        let dir = Scratch::new();
        let wav = dir.file("a.wav", b"RIFF");
        let model = dir.file("m.bin", b"m");
        let bin = dir.0.join("never-started");
        let no_model = dir.0.join("no-model.bin");
        let err = WhisperCli::new(&bin, &no_model).transcribe(&wav, SttLanguage::English).unwrap_err();
        assert_eq!(err, SpeechError::ModelNotFound(no_model));
        let no_wav = dir.0.join("no.wav");
        let err = WhisperCli::new(&bin, &model).transcribe(&no_wav, SttLanguage::English).unwrap_err();
        assert_eq!(err, SpeechError::AudioNotFound(no_wav));
    }

    #[cfg(unix)]
    #[test]
    fn non_zero_exit_carries_the_status_and_stderr() {
        let _guard = SCRIPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Scratch::new();
        let wav = dir.file("a.wav", b"RIFF");
        let model = dir.file("m.bin", b"m");
        let bin = dir.script("echo 'error: failed to initialize whisper context' >&2\necho '{}'\nexit 3");
        let err = WhisperCli::new(&bin, &model).transcribe(&wav, SttLanguage::Auto).unwrap_err();
        assert_eq!(
            err,
            SpeechError::NonZeroExit { code: Some(3), stderr: "error: failed to initialize whisper context".to_string() }
        );
    }

    #[cfg(unix)]
    #[test]
    fn success_status_with_bad_output_is_malformed() {
        let _guard = SCRIPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Scratch::new();
        let wav = dir.file("a.wav", b"RIFF");
        let model = dir.file("m.bin", b"m");
        let bin = dir.script("echo '{\"transcription\": oops'");
        let err = WhisperCli::new(&bin, &model).transcribe(&wav, SttLanguage::Auto).unwrap_err();
        assert!(matches!(err, SpeechError::MalformedOutput(_)), "{err:?}");
        let bin = dir.script("printf '\\377\\376 not utf-8'");
        let err = WhisperCli::new(&bin, &model).transcribe(&wav, SttLanguage::Auto).unwrap_err();
        assert!(matches!(err, SpeechError::MalformedOutput(_)), "{err:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_program_that_runs_too_long_is_killed() {
        let _guard = SCRIPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = Scratch::new();
        let wav = dir.file("a.wav", b"RIFF");
        let model = dir.file("m.bin", b"m");
        let pid_file = dir.0.join("pid");
        let bin = dir.script(&format!("echo $$ > {}\nexec sleep 30", sh_quote(&pid_file)));
        let started = Instant::now();
        let err = WhisperCli::new(&bin, &model)
            .with_timeout(Duration::from_millis(300))
            .transcribe(&wav, SttLanguage::Auto)
            .unwrap_err();
        assert_eq!(err, SpeechError::Timeout(Duration::from_millis(300)));
        assert!(started.elapsed() < Duration::from_secs(10));
        let pid = fs::read_to_string(&pid_file).unwrap();
        assert!(!Path::new("/proc").join(pid.trim()).exists(), "process {} still exists", pid.trim());
    }

    // ---- real whisper.cpp ----

    /// Runs against a real whisper.cpp build when `SCARFEEOS_WHISPER_CLI`,
    /// `SCARFEEOS_WHISPER_MODEL` and `SCARFEEOS_WHISPER_WAV` name one; otherwise
    /// it does nothing (so a passing run without them proves nothing about it).
    #[test]
    fn real_whisper_cli_when_available() {
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        let (Some(bin), Some(model), Some(wav)) =
            (var("SCARFEEOS_WHISPER_CLI"), var("SCARFEEOS_WHISPER_MODEL"), var("SCARFEEOS_WHISPER_WAV"))
        else {
            eprintln!("real_whisper_cli_when_available: SCARFEEOS_WHISPER_* not set, skipped");
            return;
        };
        let cli = WhisperCli::new(&bin, &model);
        let t = cli.transcribe(&wav, SttLanguage::Auto).unwrap();
        eprintln!("real whisper-cli: {t:?}");
        assert!(t.language.is_some());
        assert!(!t.segments.is_empty());

        // An odd file name goes through unchanged. Windows file names cannot hold these characters.
        #[cfg(unix)]
        {
            let odd = std::env::temp_dir().join(format!("odd name; $(id) '\"' -x {}.wav", std::process::id()));
            fs::copy(&wav, &odd).unwrap();
            let again = cli.transcribe(&odd, SttLanguage::Auto);
            fs::remove_file(&odd).unwrap();
            assert_eq!(again.unwrap().text, t.text);
        }

        let dir = Scratch::new();
        let bad_model = dir.file("bad.bin", &[7u8; 4096]);
        let err = WhisperCli::new(&bin, &bad_model).transcribe(&wav, SttLanguage::Auto).unwrap_err();
        assert!(matches!(err, SpeechError::NonZeroExit { code: Some(3), .. }), "{err:?}");
    }
}
