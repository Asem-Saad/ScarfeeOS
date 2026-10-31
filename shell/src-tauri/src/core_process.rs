//! Owns one `scarfeeos-core serve` child and talks to it in JSON lines (see
//! core/src/ipc.rs). Requests go one at a time, in order. A child that stops, or
//! that does not answer in time, is killed and started again for the next request.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const MAX_LINE_BYTES: usize = 64 * 1024;

pub struct Core {
    program: PathBuf,
    args: Vec<OsString>,
    timeout: Duration,
    session: Mutex<Option<Session>>,
}

struct Session {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Core {
    pub fn new(program: impl Into<PathBuf>, args: Vec<OsString>, timeout: Duration) -> Self {
        Core { program: program.into(), args, timeout, session: Mutex::new(None) }
    }

    /// The real core: `$SCARFEEOS_CORE`, else `scarfeeos-core` next to this program,
    /// else `scarfeeos-core` on the PATH.
    pub fn serve_command() -> Self {
        let beside = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join("scarfeeos-core")))
            .filter(|path| path.is_file());
        let program = std::env::var_os("SCARFEEOS_CORE")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or(beside)
            .unwrap_or_else(|| PathBuf::from("scarfeeos-core"));
        Core::new(program, vec!["serve".into()], DEFAULT_TIMEOUT)
    }

    /// Sends one request line and returns the response line with the same `id`.
    /// A request the core cannot read is answered with `id` null, and that
    /// answer is returned too: the core sends exactly one line per line it gets.
    pub fn ask(&self, line: &str) -> Result<String, String> {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.trim().is_empty() || line.contains(['\r', '\n']) {
            return Err("a request is exactly one non-empty line".into());
        }
        if line.len() > MAX_LINE_BYTES {
            return Err(format!("the request is larger than {} bytes", MAX_LINE_BYTES));
        }
        let expected = serde_json::from_str::<Value>(line)
            .ok()
            .and_then(|request| request.get("id").filter(|id| id.is_number()).cloned())
            .unwrap_or(Value::Null);

        let mut guard = self.session.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(session) = guard.as_mut() {
            if !matches!(session.child.try_wait(), Ok(None)) {
                *guard = None;
            }
        }
        // A child that is still here has answered before. It may have died since without
        // anyone noticing, and then it never read this request: that is replaced once and
        // the request is sent again. A child started for this request is not given a second try.
        let mut retry = guard.is_some();
        'attempt: loop {
            if guard.is_none() {
                *guard = Some(self.start()?);
            }
            let session = guard.as_mut().expect("session was just started");
            let sent = writeln!(session.stdin, "{line}").and_then(|()| session.stdin.flush());
            if sent.is_err() {
                *guard = None;
                if !retry {
                    return Err(stopped("the core stopped before it read the request"));
                }
                retry = false;
                continue;
            }
            let deadline = Instant::now() + self.timeout;
            let mut heard = false;
            loop {
                let wait = deadline.saturating_duration_since(Instant::now());
                match session.lines.recv_timeout(wait) {
                    Ok(reply) => {
                        heard = true;
                        match serde_json::from_str::<Value>(&reply) {
                            Ok(value) if value["id"].is_null() || value["id"] == expected => return Ok(reply),
                            Ok(_) => eprintln!("core: ignoring a response for another request"),
                            Err(_) => eprintln!("core: ignoring a line that is not JSON"),
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        *guard = None;
                        return Err(format!(
                            "the core did not answer within {} seconds; it was stopped and will be restarted for the next request",
                            self.timeout.as_secs()
                        ));
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        *guard = None;
                        // The pipe took the request, but a reused child that ends without a
                        // line of output most likely ended before it read it. That cannot be
                        // known: a core that acted and then died before answering runs it twice.
                        if retry && !heard {
                            retry = false;
                            continue 'attempt;
                        }
                        return Err(stopped("the core stopped before it answered"));
                    }
                }
            }
        }
    }

    /// Stops the child, if one is running.
    pub fn shutdown(&self) {
        *self.session.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    fn start(&self) -> Result<Session, String> {
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("cannot start the core ({}): {e}", self.program.display()))?;
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        let (sender, lines) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {
                        let text = String::from_utf8_lossy(&buf);
                        let text = text.trim();
                        if !text.is_empty() && sender.send(text.to_owned()).is_err() {
                            return;
                        }
                    }
                }
            }
        });
        Ok(Session { child, stdin, lines })
    }
}

fn stopped(what: &str) -> String {
    format!("{what}; it will be restarted for the next request")
}

// The fake cores are `sh` scripts and the checks read /proc, so these tests run on Unix only.
#[cfg(all(test, unix))]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    use super::*;

    const SHORT: Duration = Duration::from_secs(10);

    /// A directory that is removed when the test ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let dir = std::env::temp_dir()
                .join(format!("scarfeeos-shell-test-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }

        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }

        /// A fake core: a shell script run by `sh`, given the scratch directory as `$1`.
        fn core(&self, script: &str, timeout: Duration) -> Core {
            let path = self.path("fake-core.sh");
            fs::write(&path, script).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            Core::new("/bin/sh", vec![path.into(), self.0.clone().into()], timeout)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    // Answers each line with the id it carries, and the process id of the script.
    const ECHO_ID: &str = r#"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9.]*\).*/\1/p')
  printf '{"id":%s,"kind":"reply","text":"ok","pid":%s}\n' "${id:-null}" "$$"
done
"#;

    fn req(id: u32, text: &str) -> String {
        serde_json::json!({"id": id, "text": text, "language": "en"}).to_string()
    }

    fn parse(line: &str) -> Value {
        serde_json::from_str(line).unwrap()
    }

    fn alive(pid: u64) -> bool {
        Path::new(&format!("/proc/{pid}")).exists()
    }

    /// Gone, or finished and waiting to be collected.
    fn exited(pid: u64) -> bool {
        match fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => stat.rsplit(')').next().is_some_and(|rest| rest.trim_start().starts_with('Z')),
            Err(_) => true,
        }
    }

    #[test]
    fn returns_the_response_with_the_same_id() {
        let dir = Scratch::new();
        let core = dir.core(ECHO_ID, SHORT);
        for id in [1, 2, 3] {
            let reply = parse(&core.ask(&req(id, "hi")).unwrap());
            assert_eq!((reply["id"].clone(), reply["kind"].clone()), (id.into(), "reply".into()));
        }
    }

    #[test]
    fn keeps_one_child_across_requests() {
        let dir = Scratch::new();
        let core = dir.core(ECHO_ID, SHORT);
        let pid = |id| parse(&core.ask(&req(id, "hi")).unwrap())["pid"].as_u64().unwrap();
        assert_eq!(pid(1), pid(2));
    }

    #[test]
    fn skips_responses_for_other_requests() {
        let dir = Scratch::new();
        let core = dir.core(
            r#"
while IFS= read -r line; do
  printf 'not json\n\n{"id":99,"kind":"reply"}\n{"id":5,"kind":"reply","text":"mine"}\n'
done
"#,
            SHORT,
        );
        assert_eq!(parse(&core.ask(&req(5, "hi")).unwrap())["text"], "mine");
    }

    #[test]
    fn a_null_id_answer_belongs_to_the_request_the_core_could_not_read() {
        let dir = Scratch::new();
        let core = dir.core(
            r#"
while IFS= read -r line; do
  printf '{"id":null,"kind":"error","message":"malformed JSON"}\n'
done
"#,
            SHORT,
        );
        let reply = parse(&core.ask("{not json").unwrap());
        assert_eq!((reply["id"].clone(), reply["kind"].clone()), (Value::Null, "error".into()));
    }

    #[test]
    fn arabic_text_passes_through_unchanged() {
        let dir = Scratch::new();
        let core = dir.core("exec cat\n", SHORT);
        let line = r#"{"id":1,"text":"مرحبا، كيف حالك؟","language":"ar"}"#;
        assert_eq!(core.ask(line).unwrap(), line);
    }

    #[test]
    fn a_trailing_newline_is_not_part_of_the_request() {
        let dir = Scratch::new();
        let core = dir.core("exec cat\n", SHORT);
        let line = req(1, "hi");
        assert_eq!(core.ask(&format!("{line}\r\n")).unwrap(), line);
    }

    #[test]
    fn rejects_requests_that_are_not_one_line_without_starting_the_core() {
        let core = Core::new("/nonexistent/scarfeeos-core", vec![], SHORT);
        for bad in ["", "  ", "\n", "{\"id\":1}\n{\"id\":2}", "a\rb"] {
            let error = core.ask(bad).unwrap_err();
            assert!(error.contains("one non-empty line"), "{bad:?}: {error}");
        }
        let error = core.ask(&"x".repeat(MAX_LINE_BYTES + 1)).unwrap_err();
        assert!(error.contains("larger than"), "{error}");
    }

    #[test]
    fn a_missing_program_is_an_error_not_a_panic() {
        let core = Core::new("/nonexistent/scarfeeos-core", vec!["serve".into()], SHORT);
        let error = core.ask(&req(1, "hi")).unwrap_err();
        assert!(error.contains("cannot start the core"), "{error}");
    }

    #[test]
    fn a_child_that_exited_is_started_again() {
        let dir = Scratch::new();
        // Answers one line and exits.
        let core = dir.core(
            r#"
IFS= read -r line
id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9.]*\).*/\1/p')
printf '{"id":%s,"kind":"reply","pid":%s}\n' "$id" "$$"
"#,
            SHORT,
        );
        let first = parse(&core.ask(&req(1, "hi")).unwrap())["pid"].as_u64().unwrap();
        let waited = Instant::now();
        while !exited(first) && waited.elapsed() < Duration::from_secs(10) {
            thread::sleep(Duration::from_millis(10));
        }
        let second = parse(&core.ask(&req(2, "hi")).unwrap())["pid"].as_u64().unwrap();
        assert_ne!(first, second);
        assert_eq!(parse(&core.ask(&req(3, "hi")).unwrap())["id"], 3);
    }

    #[test]
    fn a_child_that_ends_without_reading_the_next_request_is_replaced_and_the_request_sent_again() {
        let dir = Scratch::new();
        // The first run answers one line, then lingers a moment and ends without reading another,
        // so the next request is written to a child that is alive but about to exit. Later runs answer.
        let core = dir.core(
            r#"
echo "$$" >> "$1/pids"
if [ "$(wc -l < "$1/pids")" -eq 1 ]; then
  IFS= read -r line
  printf '{"id":1,"kind":"reply","pid":%s}\n' "$$"
  sleep 1
  exit 0
fi
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9.]*\).*/\1/p')
  printf '{"id":%s,"kind":"reply","pid":%s}\n' "$id" "$$"
done
"#,
            SHORT,
        );
        let first = parse(&core.ask(&req(1, "hi")).unwrap())["pid"].as_u64().unwrap();
        assert!(alive(first), "the first child must still be running for this test to mean anything");
        let second = parse(&core.ask(&req(2, "hi")).unwrap());
        assert_eq!(second["id"], 2);
        assert_ne!(second["pid"].as_u64().unwrap(), first);
    }

    #[test]
    fn a_request_is_sent_again_only_once() {
        let dir = Scratch::new();
        // The first run answers one line and ends without reading another. Every later run reads a line and dies.
        let core = dir.core(
            r#"
echo "$$" >> "$1/pids"
IFS= read -r line
if [ "$(wc -l < "$1/pids")" -eq 1 ]; then
  printf '{"id":1,"kind":"reply","pid":%s}\n' "$$"
  sleep 1
fi
exit 3
"#,
            SHORT,
        );
        assert_eq!(parse(&core.ask(&req(1, "hi")).unwrap())["id"], 1);
        let error = core.ask(&req(2, "hi")).unwrap_err();
        assert!(error.contains("stopped") && error.contains("restarted"), "{error}");
        assert_eq!(fs::read_to_string(dir.path("pids")).unwrap().lines().count(), 2);
    }

    #[test]
    fn a_child_that_dies_mid_request_is_an_error_and_the_next_request_works() {
        let dir = Scratch::new();
        // The first run reads a line and dies without answering. Later runs answer.
        let core = dir.core(
            r#"
if [ ! -e "$1/seen" ]; then
  : > "$1/seen"
  IFS= read -r line
  exit 3
fi
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9.]*\).*/\1/p')
  printf '{"id":%s,"kind":"reply"}\n' "$id"
done
"#,
            SHORT,
        );
        let error = core.ask(&req(1, "hi")).unwrap_err();
        assert!(error.contains("stopped") && error.contains("restarted"), "{error}");
        assert_eq!(parse(&core.ask(&req(2, "hi")).unwrap())["id"], 2);
    }

    #[test]
    fn a_child_that_does_not_answer_is_killed_and_replaced() {
        let dir = Scratch::new();
        // The first run hangs after reading a line. Later runs answer.
        let core = dir.core(
            r#"
echo "$$" >> "$1/pids"
if [ "$(wc -l < "$1/pids")" -eq 1 ]; then
  IFS= read -r line
  sleep 60
  exit 0
fi
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9.]*\).*/\1/p')
  printf '{"id":%s,"kind":"reply"}\n' "$id"
done
"#,
            Duration::from_millis(500),
        );
        let started = Instant::now();
        let error = core.ask(&req(1, "hi")).unwrap_err();
        assert!(error.contains("did not answer") && error.contains("stopped"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(30));
        let hung: u64 = fs::read_to_string(dir.path("pids")).unwrap().lines().next().unwrap().parse().unwrap();
        assert!(!alive(hung), "the hung core is still running");
        assert_eq!(parse(&core.ask(&req(2, "hi")).unwrap())["id"], 2);
    }

    #[test]
    fn shutdown_stops_the_child() {
        let dir = Scratch::new();
        let core = dir.core(ECHO_ID, SHORT);
        let pid = parse(&core.ask(&req(1, "hi")).unwrap())["pid"].as_u64().unwrap();
        assert!(alive(pid));
        core.shutdown();
        assert!(!alive(pid));
        // And the next request starts a new one.
        assert_ne!(parse(&core.ask(&req(2, "hi")).unwrap())["pid"].as_u64().unwrap(), pid);
    }

    #[test]
    fn dropping_the_core_stops_the_child() {
        let dir = Scratch::new();
        let core = dir.core(ECHO_ID, SHORT);
        let pid = parse(&core.ask(&req(1, "hi")).unwrap())["pid"].as_u64().unwrap();
        drop(core);
        assert!(!alive(pid));
    }

    #[test]
    fn requests_from_several_threads_each_get_their_own_answer() {
        let dir = Scratch::new();
        let core = Arc::new(dir.core(ECHO_ID, SHORT));
        let threads: Vec<_> = (1..=8)
            .map(|id| {
                let core = Arc::clone(&core);
                thread::spawn(move || (id, parse(&core.ask(&req(id, "hi")).unwrap())["id"].clone()))
            })
            .collect();
        for thread in threads {
            let (id, answered) = thread.join().unwrap();
            assert_eq!(answered, id);
        }
    }

    #[test]
    fn serve_command_prefers_the_environment_variable() {
        // The only test that touches the environment.
        std::env::set_var("SCARFEEOS_CORE", "/opt/scarfeeos/core");
        let core = Core::serve_command();
        std::env::remove_var("SCARFEEOS_CORE");
        assert_eq!(core.program, PathBuf::from("/opt/scarfeeos/core"));
        assert_eq!(core.args, vec![OsString::from("serve")]);
    }
}
