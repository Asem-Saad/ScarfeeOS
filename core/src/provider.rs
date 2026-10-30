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
}

impl Default for LocalConfig {
    fn default() -> Self {
        LocalConfig {
            base_url: "http://127.0.0.1:8080".into(),
            model: None,
            timeout: Duration::from_secs(120),
            allow_non_loopback: false,
        }
    }
}

/// On-device model, reached over an OpenAI-compatible local API.
pub struct LocalProvider {
    config: LocalConfig,
    endpoint: String,
    agent: ureq::Agent,
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
        Ok(LocalProvider { config, endpoint, agent })
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

fn is_loopback(host: &str) -> bool {
    host.parse::<IpAddr>().map_or(false, |ip| ip.is_loopback())
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
        }
        let json = self.post(body)?;
        parse_reply(&json)
    }

    fn post(&self, mut body: serde_json::Value) -> Result<serde_json::Value, String> {
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
            allow_non_loopback: false,
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
