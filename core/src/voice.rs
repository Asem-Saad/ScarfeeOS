//! One voice turn: record from the microphone, transcribe, hand the text to the
//! assistant. Standard library only.
//!
//! Recording runs `pw-record` if it is on `PATH`, else `arecord`, as an argument
//! list and never through a shell. The audio goes to a private temporary file
//! (a mode 0700 directory, a mode 0600 file) that is removed when the turn ends,
//! on success, on error and on panic. Audio is not kept, not sent anywhere and
//! never written to a log. Only the transcript text leaves this module.
//!
//! The real capture path (`CommandRecorder` with a real `pw-record` or `arecord`
//! and a microphone) has not been run: the tests use fake programs. Wake word and
//! text to speech are not part of this module.

use std::fmt;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{ErrorKind, Read};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::assistant::{Assistant, Outcome};
use crate::lang::Language;
use crate::speech::{SpeechError, SttLanguage, Transcriber, Transcript};

/// A canonical WAV header is 44 bytes, so a file this small holds no samples.
const WAV_HEADER_BYTES: u64 = 44;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordError {
    /// Neither `pw-record` nor `arecord` is on the search path.
    NoRecorder,
    /// Recording is not supported on this platform (only Unix systems for now).
    Unsupported,
    /// The recorder could not be started or read, or the file could not be prepared.
    Io(String),
    /// The recorder ended with a failure status (`None` when killed by a signal).
    NonZeroExit { code: Option<i32>, stderr: String },
    /// The recorder did not stop within the grace period after being asked to, and was killed.
    Timeout(Duration),
    /// The recorder ended but left no audio.
    NoAudio,
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecordError::NoRecorder => write!(f, "no recording program found: install pw-record or arecord"),
            RecordError::Unsupported => write!(f, "recording is not supported on this platform"),
            RecordError::Io(why) => write!(f, "recording failed: {why}"),
            RecordError::NonZeroExit { code: Some(code), stderr } => {
                write!(f, "recording program exited with status {code}: {stderr}")
            }
            RecordError::NonZeroExit { code: None, stderr } => {
                write!(f, "recording program was killed by a signal: {stderr}")
            }
            RecordError::Timeout(grace) => write!(f, "recording program did not stop within {grace:?} and was killed"),
            RecordError::NoAudio => write!(f, "recording program produced no audio"),
        }
    }
}

impl std::error::Error for RecordError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceError {
    Record(RecordError),
    Speech(SpeechError),
}

impl fmt::Display for VoiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VoiceError::Record(e) => e.fmt(f),
            VoiceError::Speech(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for VoiceError {}

impl From<RecordError> for VoiceError {
    fn from(e: RecordError) -> Self {
        VoiceError::Record(e)
    }
}

impl From<SpeechError> for VoiceError {
    fn from(e: SpeechError) -> Self {
        VoiceError::Speech(e)
    }
}

pub trait Recorder {
    /// Records 16 kHz mono WAV into `wav` and returns when recording has ended:
    /// at the maximum length, or soon after `stop` becomes true.
    fn record(&self, wav: &Path, stop: &AtomicBool) -> Result<(), RecordError>;
}

/// Records with `pw-record` if found, else `arecord`.
#[derive(Debug, Clone)]
pub struct CommandRecorder {
    max_length: Duration,
    grace: Duration,
    /// Directories to look in. None means the `PATH` variable.
    search: Option<Vec<PathBuf>>,
}

impl CommandRecorder {
    pub const DEFAULT_MAX_LENGTH: Duration = Duration::from_secs(30);
    /// How long the recorder gets to finish the WAV file after it is asked to stop.
    pub const DEFAULT_GRACE: Duration = Duration::from_secs(3);

    pub fn new(max_length: Duration) -> Self {
        CommandRecorder { max_length, grace: Self::DEFAULT_GRACE, search: None }
    }

    pub fn with_grace(mut self, grace: Duration) -> Self {
        self.grace = grace;
        self
    }

    pub fn with_search_path(mut self, dirs: Vec<PathBuf>) -> Self {
        self.search = Some(dirs);
        self
    }

    fn find(&self, name: &str) -> Option<PathBuf> {
        let dirs = match &self.search {
            Some(dirs) => dirs.clone(),
            None => std::env::split_paths(&std::env::var_os("PATH")?).collect(),
        };
        dirs.into_iter().map(|d| d.join(name)).find(|p| is_executable(p))
    }

    fn command(&self, wav: &Path) -> Result<Command, RecordError> {
        if let Some(program) = self.find("pw-record") {
            let mut c = Command::new(program);
            c.args(["--rate", "16000", "--channels", "1", "--format", "s16"]).arg(wav);
            Ok(c)
        } else if let Some(program) = self.find("arecord") {
            let mut c = Command::new(program);
            c.args(["-q", "-t", "wav", "-f", "S16_LE", "-r", "16000", "-c", "1"]).arg(wav);
            Ok(c)
        } else {
            Err(RecordError::NoRecorder)
        }
    }
}

impl Default for CommandRecorder {
    fn default() -> Self {
        CommandRecorder::new(Self::DEFAULT_MAX_LENGTH)
    }
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    fs::metadata(path).map(|m| m.is_file()).unwrap_or(false)
}

#[cfg(unix)]
fn exit_signal(status: &ExitStatus) -> Option<i32> {
    status.signal()
}

#[cfg(not(unix))]
fn exit_signal(_status: &ExitStatus) -> Option<i32> {
    None
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    DirBuilder::new().mode(0o700).create(dir)
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    DirBuilder::new().create(dir)
}

#[cfg(unix)]
fn create_private_file(file: &Path) -> std::io::Result<fs::File> {
    OpenOptions::new().write(true).create_new(true).mode(0o600).open(file)
}

#[cfg(not(unix))]
fn create_private_file(file: &Path) -> std::io::Result<fs::File> {
    OpenOptions::new().write(true).create_new(true).open(file)
}

impl Recorder for CommandRecorder {
    fn record(&self, wav: &Path, stop: &AtomicBool) -> Result<(), RecordError> {
        // Stopping the recorder cleanly needs SIGTERM, which only Unix has.
        if cfg!(not(unix)) {
            return Err(RecordError::Unsupported);
        }
        let mut command = self.command(wav)?;
        command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|e| match e.kind() {
            ErrorKind::NotFound => RecordError::NoRecorder,
            _ => RecordError::Io(e.to_string()),
        })?;
        let stderr_reader = drain(child.stderr.take());

        let started = Instant::now();
        let mut stop_asked_at: Option<Instant> = None;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(RecordError::Io(e.to_string()));
                }
            }
            match stop_asked_at {
                None if stop.load(Ordering::SeqCst) || started.elapsed() >= self.max_length => {
                    // pw-record and arecord finish the WAV header on SIGTERM; a
                    // kill would leave the sizes in it wrong.
                    if !send_term(child.id()) {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(RecordError::Io("could not ask the recording program to stop".to_string()));
                    }
                    stop_asked_at = Some(Instant::now());
                }
                Some(at) if at.elapsed() >= self.grace => {
                    // Readers are not joined: a child of the program could hold the pipe open.
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(RecordError::Timeout(self.grace));
                }
                _ => {}
            }
            thread::sleep(Duration::from_millis(10));
        };
        let stderr = stderr_reader.join().unwrap_or_default();
        const SIGINT: i32 = 2;
        const SIGTERM: i32 = 15;
        let stopped_by_us = stop_asked_at.is_some() && matches!(exit_signal(&status), Some(SIGINT | SIGTERM));
        if !status.success() && !stopped_by_us {
            return Err(RecordError::NonZeroExit { code: status.code(), stderr: stderr_tail(&stderr) });
        }
        match fs::metadata(wav) {
            Ok(m) if m.len() > WAV_HEADER_BYTES => Ok(()),
            _ => Err(RecordError::NoAudio),
        }
    }
}

/// Sends SIGTERM through the `kill` program, as the standard library has no
/// way to send a signal other than SIGKILL.
fn send_term(pid: u32) -> bool {
    Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
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

/// A WAV file in a private directory, both removed on drop.
struct TempWav {
    dir: PathBuf,
    wav: PathBuf,
}

static NEXT_TEMP: AtomicU32 = AtomicU32::new(0);

impl TempWav {
    /// Prefers `XDG_RUNTIME_DIR` (a per-user directory in memory) over the shared temp directory.
    fn new() -> Result<TempWav, RecordError> {
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .unwrap_or_else(std::env::temp_dir);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let dir = base.join(format!(
            "scarfeeos-voice-{}-{}-{nanos}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::SeqCst)
        ));
        // Fails if the name exists, so another user cannot pre-create it.
        create_private_dir(&dir).map_err(|e| RecordError::Io(format!("temporary directory: {e}")))?;
        let temp = TempWav { wav: dir.join("turn.wav"), dir };
        create_private_file(&temp.wav).map_err(|e| RecordError::Io(format!("temporary file: {e}")))?;
        Ok(temp)
    }
}

impl Drop for TempWav {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceTurn {
    /// Nothing was said: the transcript was empty or only whisper markers such as `[BLANK_AUDIO]`.
    /// The model was not called.
    NothingHeard,
    Heard { transcript: Transcript, language: Language, outcome: Outcome },
}

/// Records, transcribes, and passes the text to the assistant in the detected
/// language: the one whisper reports if it is `ar` or `en`, else `fallback`.
/// The audio file is gone when this returns.
pub fn voice_turn(
    recorder: &dyn Recorder,
    transcriber: &dyn Transcriber,
    assistant: &Assistant,
    stop: &AtomicBool,
    stt: SttLanguage,
    fallback: Language,
) -> Result<VoiceTurn, VoiceError> {
    let temp = TempWav::new()?;
    recorder.record(&temp.wav, stop)?;
    let transcript = transcriber.transcribe(&temp.wav, stt)?;
    drop(temp);

    if is_nothing_heard(&transcript.text) {
        return Ok(VoiceTurn::NothingHeard);
    }
    let language = transcript.language.as_deref().and_then(Language::from_code).unwrap_or(fallback);
    let outcome = assistant.handle(transcript.text.trim(), language);
    Ok(VoiceTurn::Heard { transcript, language, outcome })
}

/// True when the text is empty or holds only whisper markers (`[BLANK_AUDIO]`,
/// `(silence)`, `*music*`), music notes and punctuation: nothing with a letter or digit.
pub fn is_nothing_heard(text: &str) -> bool {
    !strip_markers(text).chars().any(char::is_alphanumeric)
}

/// Removes `[...]`, `(...)` and `*...*` spans. An opener with no closer stays.
fn strip_markers(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let close = match chars[i] {
            '[' => Some(']'),
            '(' => Some(')'),
            '*' => Some('*'),
            _ => None,
        };
        match close.and_then(|c| chars[i + 1..].iter().position(|&x| x == c)) {
            Some(offset) => i += offset + 2,
            None => {
                out.push(chars[i]);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::Model;
    use crate::datetime::Date;
    use crate::provider::ModelReply;
    use crate::tools::{Clock, ClockError, LocalTime};
    use serde_json::Value;
    use std::cell::{Cell, RefCell};
    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> Result<LocalTime, ClockError> {
            Ok(LocalTime { date: Date { year: 2026, month: 3, day: 4 }, hour: 9, minute: 30, second: 0 })
        }
    }

    /// Records what the assistant asked, and answers with a fixed text.
    struct FakeModel {
        asked: RefCell<Vec<String>>,
    }
    impl FakeModel {
        fn new() -> Self {
            FakeModel { asked: RefCell::new(Vec::new()) }
        }
    }
    impl Model for FakeModel {
        fn chat(&self, _system: &str, user: &str, _tools: &Value) -> Result<ModelReply, String> {
            self.asked.borrow_mut().push(user.to_string());
            Ok(ModelReply::Text("ok".to_string()))
        }
    }

    /// Always true off Unix, where there are no modes to check.
    fn is_private(wav: &Path) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let dir_mode = fs::metadata(wav.parent().unwrap()).unwrap().permissions().mode() & 0o777;
            let file_mode = fs::metadata(wav).unwrap().permissions().mode() & 0o777;
            dir_mode == 0o700 && file_mode == 0o600
        }
        #[cfg(not(unix))]
        {
            let _ = wav;
            true
        }
    }

    struct FakeRecorder {
        result: Result<(), RecordError>,
        seen: RefCell<Option<PathBuf>>,
        saw_private: Cell<bool>,
    }
    impl FakeRecorder {
        fn ok() -> Self {
            FakeRecorder { result: Ok(()), seen: RefCell::new(None), saw_private: Cell::new(false) }
        }
        fn failing(e: RecordError) -> Self {
            FakeRecorder { result: Err(e), ..Self::ok() }
        }
    }
    impl Recorder for FakeRecorder {
        fn record(&self, wav: &Path, _stop: &AtomicBool) -> Result<(), RecordError> {
            *self.seen.borrow_mut() = Some(wav.to_path_buf());
            self.saw_private.set(is_private(wav));
            fs::write(wav, b"fake audio").unwrap();
            self.result.clone()
        }
    }

    struct FakeTranscriber {
        result: Result<Transcript, SpeechError>,
        language_asked: Cell<Option<SttLanguage>>,
    }
    impl FakeTranscriber {
        fn says(text: &str, language: Option<&str>) -> Self {
            FakeTranscriber {
                result: Ok(Transcript { text: text.to_string(), language: language.map(String::from), segments: vec![] }),
                language_asked: Cell::new(None),
            }
        }
        fn failing(e: SpeechError) -> Self {
            FakeTranscriber { result: Err(e), language_asked: Cell::new(None) }
        }
    }
    impl Transcriber for FakeTranscriber {
        fn transcribe(&self, wav: &Path, language: SttLanguage) -> Result<Transcript, SpeechError> {
            assert!(wav.is_file(), "the audio must exist while transcribing");
            self.language_asked.set(Some(language));
            self.result.clone()
        }
    }

    fn run(
        recorder: &FakeRecorder,
        transcriber: &FakeTranscriber,
        model: &FakeModel,
        fallback: Language,
    ) -> Result<VoiceTurn, VoiceError> {
        let assistant = Assistant::new(model, &FixedClock);
        voice_turn(recorder, transcriber, &assistant, &AtomicBool::new(false), SttLanguage::Auto, fallback)
    }

    #[test]
    fn transcript_goes_to_the_assistant_in_the_detected_language() {
        let (recorder, model) = (FakeRecorder::ok(), FakeModel::new());
        let transcriber = FakeTranscriber::says(" مرحبا كيف حالك ", Some("ar"));
        let turn = run(&recorder, &transcriber, &model, Language::English).unwrap();
        match turn {
            VoiceTurn::Heard { transcript, language, outcome } => {
                assert_eq!(transcript.text, " مرحبا كيف حالك ");
                assert_eq!(language, Language::Arabic);
                assert!(matches!(outcome, Outcome::Reply { .. }), "{outcome:?}");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(*model.asked.borrow(), ["مرحبا كيف حالك"]);
        assert_eq!(transcriber.language_asked.get(), Some(SttLanguage::Auto));
        assert!(recorder.saw_private.get());
    }

    #[test]
    fn english_is_detected_and_unknown_language_uses_the_fallback() {
        let model = FakeModel::new();
        for (reported, fallback, want) in [
            (Some("en"), Language::Arabic, Language::English),
            (Some("fr"), Language::Arabic, Language::Arabic),
            (None, Language::English, Language::English),
        ] {
            let turn = run(&FakeRecorder::ok(), &FakeTranscriber::says("hello there", reported), &model, fallback).unwrap();
            match turn {
                VoiceTurn::Heard { language, .. } => assert_eq!(language, want, "{reported:?}"),
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn empty_and_marker_only_transcripts_are_nothing_heard_and_skip_the_model() {
        let model = FakeModel::new();
        for text in [
            "", "  \n", "[BLANK_AUDIO]", " [BLANK_AUDIO] ", "(silence)", "[ Silence ]", "*music* ♪", "[مجالة]",
            "[MUSIC] [BLANK_AUDIO]", "...",
        ] {
            let turn = run(&FakeRecorder::ok(), &FakeTranscriber::says(text, Some("en")), &model, Language::English).unwrap();
            assert_eq!(turn, VoiceTurn::NothingHeard, "{text:?}");
        }
        assert!(model.asked.borrow().is_empty());
    }

    #[test]
    fn speech_next_to_a_marker_is_still_heard() {
        assert!(!is_nothing_heard("[BLANK_AUDIO] what time is it"));
        assert!(!is_nothing_heard("ما الساعة (الآن)"));
        assert!(!is_nothing_heard("call 5"));
        assert!(!is_nothing_heard("[unclosed bracket"));
        assert!(is_nothing_heard("[]"));
    }

    #[test]
    fn audio_is_removed_after_success_nothing_heard_and_errors() {
        let model = FakeModel::new();
        let cases = [
            (FakeRecorder::ok(), FakeTranscriber::says("hello", Some("en"))),
            (FakeRecorder::ok(), FakeTranscriber::says("[BLANK_AUDIO]", Some("en"))),
            (FakeRecorder::failing(RecordError::NoAudio), FakeTranscriber::says("hello", None)),
            (FakeRecorder::ok(), FakeTranscriber::failing(SpeechError::Timeout(Duration::from_secs(1)))),
        ];
        for (recorder, transcriber) in &cases {
            let _ = run(recorder, transcriber, &model, Language::English);
            let wav = recorder.seen.borrow().clone().expect("recorder was called");
            assert!(!wav.exists(), "{wav:?} still exists");
            assert!(!wav.parent().unwrap().exists(), "{:?} still exists", wav.parent());
        }
    }

    #[test]
    fn recorder_and_speech_errors_are_returned_and_the_model_is_not_called() {
        let model = FakeModel::new();
        let err = run(
            &FakeRecorder::failing(RecordError::NoRecorder),
            &FakeTranscriber::says("hello", None),
            &model,
            Language::English,
        )
        .unwrap_err();
        assert_eq!(err, VoiceError::Record(RecordError::NoRecorder));

        let transcriber = FakeTranscriber::failing(SpeechError::ModelNotFound(PathBuf::from("m.bin")));
        let err = run(&FakeRecorder::ok(), &transcriber, &model, Language::English).unwrap_err();
        assert_eq!(err, VoiceError::Speech(SpeechError::ModelNotFound(PathBuf::from("m.bin"))));
        assert!(model.asked.borrow().is_empty());
    }

    #[test]
    fn temp_wav_is_private_unique_and_removed_on_drop() {
        let a = TempWav::new().unwrap();
        let b = TempWav::new().unwrap();
        assert_ne!(a.dir, b.dir);
        assert!(is_private(&a.wav));
        let (dir, wav) = (a.dir.clone(), a.wav.clone());
        drop(a);
        assert!(!dir.exists() && !wav.exists());
        assert!(b.dir.exists());
    }

    #[cfg(unix)]
    mod fake_programs {
        use super::*;
        use std::os::unix::fs::PermissionsExt;
        use std::sync::Mutex;

        // The tests below run fake recording programs from a scratch directory.

        /// Writing and starting scripts from parallel threads can fail with
        /// "text file busy", so the tests that do it run one at a time.
        static SCRIPT_LOCK: Mutex<()> = Mutex::new(());

        struct Bin(TempWav);

        impl Bin {
            fn new() -> Bin {
                Bin(TempWav::new().unwrap())
            }

            fn dir(&self) -> PathBuf {
                self.0.dir.clone()
            }

            fn script(&self, name: &str, body: &str) {
                let path = self.0.dir.join(name);
                fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            }

            fn recorder(&self, max: Duration) -> CommandRecorder {
                CommandRecorder::new(max).with_grace(Duration::from_millis(500)).with_search_path(vec![self.dir()])
            }
        }

        /// Writes its arguments next to the script, makes some audio in the last
        /// argument, then waits to be stopped.
        const RECORDING_BODY: &str =
            "for last; do :; done\nprintf '%s\\n' \"$@\" > \"$0.args\"\nhead -c 200 /dev/zero > \"$last\"\nexec sleep 30";

        #[test]
        fn pw_record_is_preferred_and_gets_an_argument_list() {
            let _guard = SCRIPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let bin = Bin::new();
            bin.script("pw-record", RECORDING_BODY);
            bin.script("arecord", "exit 9");
            let out = TempWav::new().unwrap();
            let odd = out.dir.join("odd; $(touch pwned) `id` * -x.wav");

            let started = Instant::now();
            bin.recorder(Duration::from_millis(300)).record(&odd, &AtomicBool::new(false)).unwrap();
            assert!(started.elapsed() < Duration::from_secs(5));

            let args = fs::read_to_string(bin.dir().join("pw-record.args")).unwrap();
            let args: Vec<&str> = args.lines().collect();
            assert_eq!(args[..6], ["--rate", "16000", "--channels", "1", "--format", "s16"]);
            assert_eq!(args[6], odd.to_str().unwrap());
            assert!(!out.dir.join("pwned").exists());
        }

        #[test]
        fn arecord_is_used_when_pw_record_is_missing() {
            let _guard = SCRIPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let bin = Bin::new();
            bin.script("arecord", RECORDING_BODY);
            let out = TempWav::new().unwrap();

            bin.recorder(Duration::from_millis(300)).record(&out.wav, &AtomicBool::new(false)).unwrap();

            let args = fs::read_to_string(bin.dir().join("arecord.args")).unwrap();
            let args: Vec<&str> = args.lines().collect();
            assert_eq!(args, ["-q", "-t", "wav", "-f", "S16_LE", "-r", "16000", "-c", "1", out.wav.to_str().unwrap()]);
        }

        #[test]
        fn no_recorder_is_reported() {
            let bin = Bin::new();
            let out = TempWav::new().unwrap();
            let err = bin.recorder(Duration::from_secs(1)).record(&out.wav, &AtomicBool::new(false)).unwrap_err();
            assert_eq!(err, RecordError::NoRecorder);
        }

        #[test]
        fn stop_request_ends_recording_long_before_the_maximum() {
            let _guard = SCRIPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let bin = Bin::new();
            bin.script("pw-record", RECORDING_BODY);
            let out = TempWav::new().unwrap();
            let stop = AtomicBool::new(false);

            let started = Instant::now();
            thread::scope(|s| {
                s.spawn(|| {
                    thread::sleep(Duration::from_millis(300));
                    stop.store(true, Ordering::SeqCst);
                });
                bin.recorder(Duration::from_secs(60)).record(&out.wav, &stop).unwrap();
            });
            assert!(started.elapsed() < Duration::from_secs(10));
        }

        #[test]
        fn a_recorder_that_ignores_the_stop_is_killed() {
            let _guard = SCRIPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let bin = Bin::new();
            bin.script("pw-record", &format!("trap '' TERM\n{RECORDING_BODY}"));
            let out = TempWav::new().unwrap();

            let started = Instant::now();
            let err = bin.recorder(Duration::from_millis(200)).record(&out.wav, &AtomicBool::new(false)).unwrap_err();
            assert_eq!(err, RecordError::Timeout(Duration::from_millis(500)));
            assert!(started.elapsed() < Duration::from_secs(10));
        }

        #[test]
        fn a_failing_recorder_reports_its_stderr() {
            let _guard = SCRIPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let bin = Bin::new();
            bin.script("arecord", "echo 'no such device' >&2\nexit 1");
            let out = TempWav::new().unwrap();
            let err = bin.recorder(Duration::from_secs(5)).record(&out.wav, &AtomicBool::new(false)).unwrap_err();
            assert_eq!(err, RecordError::NonZeroExit { code: Some(1), stderr: "no such device".to_string() });
        }

        #[test]
        fn a_recorder_that_leaves_no_audio_is_reported() {
            let _guard = SCRIPT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let bin = Bin::new();
            bin.script("arecord", "exit 0");
            let out = TempWav::new().unwrap();
            let err = bin.recorder(Duration::from_secs(5)).record(&out.wav, &AtomicBool::new(false)).unwrap_err();
            assert_eq!(err, RecordError::NoAudio);
        }
    }
}
