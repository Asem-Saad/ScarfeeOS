use std::net::IpAddr;
use std::time::Duration;

use crate::lang::Language;

pub struct Request {
    pub prompt: String,
    pub language: Language,
    /// True for long or complex tasks that benefit from a larger model.
    pub heavy: bool,
}

pub trait Provider {
    fn name(&self) -> &'static str;
    fn complete(&self, req: &Request) -> Result<String, String>;
}

/// Reply limit for a turn with tools, which is a short tool call or a short
/// answer. Without a limit a runaway reply (one that repeats itself until the
/// context is full) kept the command model busy for two minutes. 768 tokens is
/// a long answer in Arabic, which takes about 2 to 3 tokens per word, and at
/// 10 to 20 tokens per second on a CPU it stays well inside the request timeout.
/// The writer is asked without tools, so it is not limited here and keeps its
/// longer timeout for long texts.
const COMMAND_MAX_TOKENS: u32 = 768;

/// Settings for the local OpenAI-compatible server (llama.cpp, vLLM).
#[derive(Clone, Debug)]
pub struct LocalConfig {
    pub base_url: String,
    /// Sent as `model` when set; llama.cpp ignores it, vLLM needs it.
    pub model: Option<String>,
    pub timeout: Duration,
    /// Allow a base URL that is not a loopback address. Off by default so
    /// local-only mode cannot send data over the network by accident.
    pub allow_non_loopback: bool,
    /// Ask the server who it is before the first request (llama.cpp `/props`,
    /// then `/v1/models`) and refuse if it does not look like a llama.cpp
    /// server, or if it has loaded a different model file than
    /// `expected_model_file`. Turn off for development or for servers without
    /// those endpoints (vLLM).
    pub check_server: bool,
    /// File name of the model the server should have loaded, for example
    /// `Qwen3-4B-Q4_K_M.gguf`. Only the last path component is compared, and
    /// case is ignored. With `None` the server must still identify itself,
    /// but any model is accepted.
    pub expected_model_file: Option<String>,
}

impl Default for LocalConfig {
    fn default() -> Self {
        LocalConfig {
            base_url: "http://127.0.0.1:8080".into(),
            model: None,
            timeout: Duration::from_secs(120),
            allow_non_loopback: false,
            check_server: false,
            expected_model_file: None,
        }
    }
}

/// On-device model, reached over an OpenAI-compatible local API.
pub struct LocalProvider {
    config: LocalConfig,
    endpoint: String,
    agent: ureq::Agent,
    verified: std::sync::OnceLock<()>,
}

impl LocalProvider {
    pub fn new(config: LocalConfig) -> Result<Self, String> {
        let host = http_host(&config.base_url)?;
        if !config.allow_non_loopback && !is_loopback(host) {
            return Err(format!(
                "local provider: {host} is not a loopback IP address (use 127.0.0.1 or [::1], or opt in to non-loopback URLs)"
            ));
        }
        let endpoint = format!("{}/v1/chat/completions", config.base_url.trim_end_matches('/'));
        // No proxy and no redirects: a request must go only where the
        // configured URL says.
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(config.timeout))
            .http_status_as_error(false)
            .max_redirects(0)
            .proxy(None)
            .build()
            .into();
        Ok(LocalProvider { config, endpoint, agent, verified: std::sync::OnceLock::new() })
    }
}

impl Default for LocalProvider {
    fn default() -> Self {
        LocalProvider::new(LocalConfig::default()).expect("default config is valid")
    }
}

/// Host part of an `http://` URL. Rejects other schemes, since TLS is not
/// built in, and user info, which can disguise the real host.
fn http_host(url: &str) -> Result<&str, String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("local provider: base URL must start with http://: {url}"))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return Err(format!("local provider: user info is not allowed in base URL: {url}"));
    }
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => authority.split(':').next().unwrap_or(""),
    };
    if host.is_empty() {
        return Err(format!("local provider: base URL has no host: {url}"));
    }
    Ok(host)
}

/// Last component of a path written with `/` or `\\`, since the server may run
/// on Windows.
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// The model file name a llama.cpp server reports, from `/props` or, when that
/// has no `model_path`, from `/v1/models`. Both are documented in
/// tools/server/README.md of llama.cpp: `/props` has `model_path` ("the path
/// to model file, same with `-m`"), and `/v1/models` has `data[].id`, which is
/// the `-m` path unless `--alias` is set. `/props` comes first because an alias
/// does not change it.
fn loaded_model_file(props: Option<&serde_json::Value>, models: Option<&serde_json::Value>) -> Option<String> {
    let from_props = props.and_then(|p| p["model_path"].as_str());
    let from_models = models.and_then(|m| m["data"][0]["id"].as_str());
    from_props.or(from_models).filter(|p| !p.is_empty()).map(|p| file_name(p).to_string())
}

fn is_loopback(host: &str) -> bool {
    host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

impl Provider for LocalProvider {
    fn name(&self) -> &'static str {
        "local"
    }

    fn complete(&self, req: &Request) -> Result<String, String> {
        let json = self.post(serde_json::json!({
            "messages": [{"role": "user", "content": req.prompt}],
            "stream": false,
        }))?;
        json["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| "local provider: reply has no choices[0].message.content".to_string())
    }
}

/// A tool call as the model wrote it, before any checking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawToolCall {
    pub name: String,
    /// JSON text of the arguments, exactly as received.
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelReply {
    Text(String),
    ToolCalls(Vec<RawToolCall>),
}

impl LocalProvider {
    /// Sends one chat turn with the OpenAI `tools` field. `tools` is a JSON array
    /// of function definitions. Returns the tool calls when the model made any,
    /// otherwise its text.
    pub fn complete_with_tools(
        &self,
        system: &str,
        user: &str,
        tools: &serde_json::Value,
    ) -> Result<ModelReply, String> {
        let mut body = serde_json::json!({
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "stream": false,
        });
        // Some servers reject an empty `tools` array.
        if tools.as_array().is_none_or(|t| !t.is_empty()) {
            body["tools"] = tools.clone();
            body["max_tokens"] = COMMAND_MAX_TOKENS.into();
        }
        let json = self.post(body)?;
        parse_reply(&json)
    }

    /// Checks once, before the first request, that the server on this port is a
    /// llama.cpp server with the expected model file loaded.
    ///
    /// This guards against accidents and stale servers: another program that
    /// took the port, or an old server still running with a different model.
    /// It does not stop a determined attacker. A process of the same user can
    /// run a fake server that answers `/props` with any model name, and can
    /// take the port again after this check. Loopback and this check make the
    /// mistake visible; they are not authentication.
    fn verify_server(&self) -> Result<(), String> {
        if !self.config.check_server || self.verified.get().is_some() {
            return Ok(());
        }
        let base = self.config.base_url.trim_end_matches('/');
        let get = |path: &str| -> Option<serde_json::Value> {
            let mut resp = self.agent.get(&format!("{base}{path}")).call().ok()?;
            if resp.status().as_u16() != 200 {
                return None;
            }
            serde_json::from_str(&resp.body_mut().read_to_string().ok()?).ok()
        };
        let props = get("/props");
        let models = if props.as_ref().is_some_and(|p| p["model_path"].is_string()) { None } else { get("/v1/models") };
        let loaded = loaded_model_file(props.as_ref(), models.as_ref()).ok_or_else(|| {
            format!(
                "local provider: the server at {base} did not report a loaded model (no model_path in /props, no id in /v1/models); it may not be a llama.cpp server, so no request was sent"
            )
        })?;
        if let Some(expected) = &self.config.expected_model_file {
            if !file_name(expected).eq_ignore_ascii_case(&loaded) {
                return Err(format!(
                    "local provider: the server at {base} has loaded {loaded}, but {} was expected; stop the other server or check the model setting, no request was sent",
                    file_name(expected)
                ));
            }
        }
        let _ = self.verified.set(());
        Ok(())
    }

    fn post(&self, mut body: serde_json::Value) -> Result<serde_json::Value, String> {
        self.verify_server()?;
        if let Some(model) = &self.config.model {
            body["model"] = model.clone().into();
        }
        // Thinking made replies several times slower and worse in our eval.
        body["chat_template_kwargs"] = serde_json::json!({"enable_thinking": false});
        body["temperature"] = 0.into();
        let mut resp = self
            .agent
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .send(body.to_string())
            .map_err(|e| format!("local provider: request to {} failed: {e}", self.endpoint))?;
        let status = resp.status().as_u16();
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("local provider: could not read reply: {e}"))?;
        if status != 200 {
            let detail: String = text.chars().take(200).collect();
            return Err(format!("local provider: server returned HTTP {status}: {detail}"));
        }
        serde_json::from_str(&text).map_err(|e| format!("local provider: malformed JSON in reply: {e}"))
    }
}

fn parse_reply(json: &serde_json::Value) -> Result<ModelReply, String> {
    let message = &json["choices"][0]["message"];
    if let Some(calls) = message["tool_calls"].as_array().filter(|c| !c.is_empty()) {
        let calls = calls
            .iter()
            .map(|call| {
                let function = &call["function"];
                let name = function["name"]
                    .as_str()
                    .ok_or_else(|| "local provider: tool call has no function.name".to_string())?;
                // OpenAI sends arguments as a JSON string; some servers send an object.
                let arguments = match &function["arguments"] {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Null => "{}".to_string(),
                    other => other.to_string(),
                };
                Ok(RawToolCall { name: name.to_string(), arguments })
            })
            .collect::<Result<Vec<_>, String>>()?;
        return Ok(ModelReply::ToolCalls(calls));
    }
    message["content"]
        .as_str()
        .map(|text| ModelReply::Text(text.to_string()))
        .ok_or_else(|| "local provider: reply has no tool_calls and no choices[0].message.content".to_string())
}

/// Optional Claude API provider. Not implemented yet.
pub struct ClaudeProvider {
    pub api_key: String,
}

impl Provider for ClaudeProvider {
    fn name(&self) -> &'static str {
        "claude"
    }
    fn complete(&self, _req: &Request) -> Result<String, String> {
        Err("claude provider not implemented yet".into())
    }
}

/// Chooses a provider. Heavy tasks go to Claude only when it is configured
/// and local-only mode is off.
pub struct Router {
    pub local: LocalProvider,
    pub claude: Option<ClaudeProvider>,
    pub local_only: bool,
}

impl Router {
    pub fn select(&self, req: &Request) -> &dyn Provider {
        match (&self.claude, self.local_only, req.heavy) {
            (Some(c), false, true) => c,
            _ => &self.local,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(heavy: bool) -> Request {
        Request { prompt: String::new(), language: Language::Arabic, heavy }
    }

    #[test]
    fn local_only_blocks_claude() {
        let r = Router {
            local: LocalProvider::default(),
            claude: Some(ClaudeProvider { api_key: "k".into() }),
            local_only: true,
        };
        assert_eq!(r.select(&req(true)).name(), "local");
    }

    #[test]
    fn heavy_uses_claude_when_allowed() {
        let r = Router {
            local: LocalProvider::default(),
            claude: Some(ClaudeProvider { api_key: "k".into() }),
            local_only: false,
        };
        assert_eq!(r.select(&req(true)).name(), "claude");
        assert_eq!(r.select(&req(false)).name(), "local");
    }

    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Serves one canned HTTP reply on a random loopback port and returns
    /// the base URL plus a handle yielding the raw request it received.
    fn serve_once(status: &str, body: &str) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let reply = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let handle = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 1024];
            loop {
                let n = conn.read(&mut chunk).unwrap();
                buf.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&buf);
                if let Some(i) = text.find("\r\n\r\n") {
                    let len = text[..i]
                        .lines()
                        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                        .unwrap_or(0);
                    if buf.len() >= i + 4 + len {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            conn.write_all(reply.as_bytes()).unwrap();
            String::from_utf8_lossy(&buf).into_owned()
        });
        (url, handle)
    }

    fn local(url: String, model: Option<&str>) -> LocalProvider {
        LocalProvider::new(LocalConfig {
            base_url: url,
            model: model.map(str::to_string),
            timeout: Duration::from_secs(5),
            ..LocalConfig::default()
        })
        .unwrap()
    }

    fn ask(p: &LocalProvider) -> Result<String, String> {
        p.complete(&Request { prompt: "مرحبا".into(), language: Language::Arabic, heavy: false })
    }

    #[test]
    fn local_success() {
        let (url, server) = serve_once("200 OK", r#"{"choices":[{"message":{"role":"assistant","content":"أهلاً"}}]}"#);
        let p = local(url, Some("tiny"));
        assert_eq!(ask(&p).unwrap(), "أهلاً");
        let raw = server.join().unwrap();
        assert!(raw.starts_with("POST /v1/chat/completions HTTP/1.1"), "{raw}");
        let body: serde_json::Value = serde_json::from_str(raw.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["model"], "tiny");
        assert_eq!(body["messages"][0]["content"], "مرحبا");
    }

    #[test]
    fn local_server_error() {
        let (url, server) = serve_once("500 Internal Server Error", "boom");
        let err = ask(&local(url, None)).unwrap_err();
        server.join().unwrap();
        assert!(err.contains("HTTP 500") && err.contains("boom"), "{err}");
    }

    #[test]
    fn local_malformed_json() {
        let (url, server) = serve_once("200 OK", "{not json");
        let err = ask(&local(url, None)).unwrap_err();
        server.join().unwrap();
        assert!(err.contains("malformed JSON"), "{err}");
    }

    #[test]
    fn local_missing_content() {
        let (url, server) = serve_once("200 OK", r#"{"choices":[]}"#);
        let err = ask(&local(url, None)).unwrap_err();
        server.join().unwrap();
        assert!(err.contains("no choices[0].message.content"), "{err}");
    }

    #[test]
    fn local_connection_refused() {
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let err = ask(&local(format!("http://127.0.0.1:{port}"), None)).unwrap_err();
        assert!(err.contains("request to") && err.contains("failed"), "{err}");
    }

    #[test]
    fn local_rejects_non_loopback() {
        for url in [
            "http://192.168.1.10:8080",
            "http://example.com",
            "http://localhost:8080",
            "http://127.0.0.1@evil.example",
            "https://127.0.0.1:8080",
        ] {
            let cfg = LocalConfig { base_url: url.into(), ..LocalConfig::default() };
            assert!(LocalProvider::new(cfg).is_err(), "{url} should be rejected");
        }
        for url in ["http://127.0.0.1:8080", "http://[::1]:8080/", "http://127.1.2.3"] {
            let cfg = LocalConfig { base_url: url.into(), ..LocalConfig::default() };
            assert!(LocalProvider::new(cfg).is_ok(), "{url} should be allowed");
        }
    }

    #[test]
    fn local_non_loopback_needs_opt_in() {
        let cfg = LocalConfig {
            base_url: "http://192.168.1.10:8080".into(),
            allow_non_loopback: true,
            ..LocalConfig::default()
        };
        assert!(LocalProvider::new(cfg).is_ok());
    }

    /// Fake llama.cpp server for the identity check. Answers `max` connections,
    /// each with the body routed by its request path (404 when none matches),
    /// and returns the request lines it saw, in order.
    fn serve_routes(routes: Vec<(&'static str, &'static str)>, max: usize) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(false).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for _ in 0..max {
                let Ok((mut conn, _)) = listener.accept() else { break };
                let mut buf = Vec::new();
                let mut chunk = [0u8; 1024];
                while !String::from_utf8_lossy(&buf).contains("\r\n\r\n") {
                    let n = conn.read(&mut chunk).unwrap();
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let head = String::from_utf8_lossy(&buf).into_owned();
                let line = head.lines().next().unwrap_or("").to_string();
                let path = line.split(' ').nth(1).unwrap_or("").to_string();
                let (status, body) = match routes.iter().find(|(p, _)| *p == path) {
                    Some((_, body)) => ("200 OK", *body),
                    None => ("404 Not Found", "{}"),
                };
                let reply = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                conn.write_all(reply.as_bytes()).unwrap();
                seen.push(line);
            }
            seen
        });
        (url, handle)
    }

    const CHAT_OK: &str = r#"{"choices":[{"message":{"content":"ok"}}]}"#;
    const EXPECTED: &str = "Qwen3-4B-Q4_K_M.gguf";

    fn checked(url: String, expected: Option<&str>) -> LocalProvider {
        LocalProvider::new(LocalConfig {
            base_url: url,
            timeout: Duration::from_secs(5),
            check_server: true,
            expected_model_file: expected.map(str::to_string),
            ..LocalConfig::default()
        })
        .unwrap()
    }

    #[test]
    fn server_check_passes_on_matching_model_and_runs_once() {
        let routes = vec![
            ("/props", r#"{"model_path":"/models/Qwen3-4B-Q4_K_M.gguf","total_slots":1}"#),
            ("/v1/chat/completions", CHAT_OK),
        ];
        let (url, server) = serve_routes(routes, 3);
        let p = checked(url, Some(EXPECTED));
        assert_eq!(ask(&p).unwrap(), "ok");
        assert_eq!(ask(&p).unwrap(), "ok");
        let seen = server.join().unwrap();
        assert!(seen[0].starts_with("GET /props "), "{seen:?}");
        assert!(seen[1].starts_with("POST /v1/chat/completions "), "{seen:?}");
        assert_eq!(seen.len(), 3, "the identity check must run only once: {seen:?}");
    }

    #[test]
    fn server_check_accepts_a_windows_path_and_other_case() {
        let routes = vec![("/props", r#"{"model_path":"D:\\models\\qwen3-4b-q4_k_m.GGUF"}"#), ("/v1/chat/completions", CHAT_OK)];
        let (url, server) = serve_routes(routes, 2);
        assert_eq!(ask(&checked(url, Some(EXPECTED))).unwrap(), "ok");
        server.join().unwrap();
    }

    #[test]
    fn server_check_refuses_a_different_model_before_any_request() {
        let routes = vec![("/props", r#"{"model_path":"/models/Jais-3-3.7B-Q4_K_M.gguf"}"#), ("/v1/chat/completions", CHAT_OK)];
        let (url, server) = serve_routes(routes, 1);
        let err = ask(&checked(url, Some(EXPECTED))).unwrap_err();
        let seen = server.join().unwrap();
        assert!(err.contains("Jais-3-3.7B-Q4_K_M.gguf") && err.contains(EXPECTED) && err.contains("no request was sent"), "{err}");
        assert_eq!(seen.len(), 1, "only the identity check may reach the server: {seen:?}");
    }

    #[test]
    fn server_check_refuses_a_server_that_reports_no_model() {
        // Neither /props nor /v1/models is usable: a fake or foreign server.
        let (url, server) = serve_routes(vec![("/props", r#"{"total_slots":1}"#), ("/v1/models", r#"{"data":[]}"#)], 2);
        let err = ask(&checked(url, Some(EXPECTED))).unwrap_err();
        let seen = server.join().unwrap();
        assert!(err.contains("did not report a loaded model"), "{err}");
        assert!(seen.iter().all(|l| l.starts_with("GET ")), "{seen:?}");
    }

    #[test]
    fn server_check_falls_back_to_v1_models() {
        let routes = vec![
            ("/v1/models", r#"{"object":"list","data":[{"id":"/models/Qwen3-4B-Q4_K_M.gguf"}]}"#),
            ("/v1/chat/completions", CHAT_OK),
        ];
        let (url, server) = serve_routes(routes, 3);
        assert_eq!(ask(&checked(url, Some(EXPECTED))).unwrap(), "ok");
        server.join().unwrap();
    }

    #[test]
    fn server_check_without_expected_name_still_needs_an_answer() {
        let (url, server) = serve_routes(vec![("/props", r#"{"model_path":"/m/x.gguf"}"#), ("/v1/chat/completions", CHAT_OK)], 2);
        assert_eq!(ask(&checked(url, None)).unwrap(), "ok");
        server.join().unwrap();
        let (url, server) = serve_routes(vec![], 2);
        assert!(ask(&checked(url, None)).is_err());
        server.join().unwrap();
    }

    #[test]
    fn server_check_off_sends_no_identity_request() {
        let (url, server) = serve_routes(vec![("/v1/chat/completions", CHAT_OK)], 1);
        assert_eq!(ask(&local(url, None)).unwrap(), "ok");
        assert_eq!(server.join().unwrap().len(), 1);
    }

    #[test]
    fn server_check_does_not_make_a_remote_url_acceptable() {
        let cfg = LocalConfig { base_url: "http://192.168.1.10:8080".into(), check_server: true, ..LocalConfig::default() };
        assert!(LocalProvider::new(cfg).is_err());
    }

    #[test]
    fn local_tool_call_reply() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"c1","type":"function","function":{"name":"open_app","arguments":"{\"name\":\"calculator\"}"}}]}}]}"#;
        let (url, server) = serve_once("200 OK", body);
        let tools = serde_json::json!([{"type": "function", "function": {"name": "open_app"}}]);
        let reply = local(url, None).complete_with_tools("sys", "افتح الآلة الحاسبة", &tools).unwrap();
        assert_eq!(
            reply,
            ModelReply::ToolCalls(vec![RawToolCall { name: "open_app".into(), arguments: r#"{"name":"calculator"}"#.into() }])
        );
        let raw = server.join().unwrap();
        let sent: serde_json::Value = serde_json::from_str(raw.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(sent["tools"], tools);
        assert_eq!(sent["max_tokens"], COMMAND_MAX_TOKENS);
        assert_eq!(sent["messages"][0]["role"], "system");
        assert_eq!(sent["messages"][1]["content"], "افتح الآلة الحاسبة");
    }

    #[test]
    fn local_text_reply_with_tools() {
        let (url, server) = serve_once("200 OK", r#"{"choices":[{"message":{"content":"مرحبا","tool_calls":[]}}]}"#);
        let reply = local(url, None).complete_with_tools("s", "u", &serde_json::json!([])).unwrap();
        let raw = server.join().unwrap();
        assert_eq!(reply, ModelReply::Text("مرحبا".into()));
        let sent: serde_json::Value = serde_json::from_str(raw.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert!(sent.get("tools").is_none(), "empty tools list must be omitted: {sent}");
        assert!(sent.get("max_tokens").is_none(), "a turn without tools is not limited: {sent}");
        assert_eq!(sent["messages"][1]["content"], "u");
    }

    #[test]
    fn tool_call_arguments_may_be_an_object() {
        let json = serde_json::json!({"choices": [{"message": {"tool_calls": [
            {"function": {"name": "set_language", "arguments": {"lang": "ar"}}}
        ]}}]});
        assert_eq!(
            parse_reply(&json).unwrap(),
            ModelReply::ToolCalls(vec![RawToolCall { name: "set_language".into(), arguments: r#"{"lang":"ar"}"#.into() }])
        );
    }

    #[test]
    fn reply_with_neither_text_nor_calls_is_an_error() {
        let err = parse_reply(&serde_json::json!({"choices": [{"message": {"content": null}}]})).unwrap_err();
        assert!(err.contains("no tool_calls"), "{err}");
    }
}
