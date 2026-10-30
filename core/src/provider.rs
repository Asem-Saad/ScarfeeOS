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

/// On-device model (llama.cpp). Not implemented yet.
pub struct LocalProvider;

impl Provider for LocalProvider {
    fn name(&self) -> &'static str {
        "local"
    }
    fn complete(&self, _req: &Request) -> Result<String, String> {
        Err("local provider not implemented yet".into())
    }
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
            local: LocalProvider,
            claude: Some(ClaudeProvider { api_key: "k".into() }),
            local_only: true,
        };
        assert_eq!(r.select(&req(true)).name(), "local");
    }

    #[test]
    fn heavy_uses_claude_when_allowed() {
        let r = Router {
            local: LocalProvider,
            claude: Some(ClaudeProvider { api_key: "k".into() }),
            local_only: false,
        };
        assert_eq!(r.select(&req(true)).name(), "claude");
        assert_eq!(r.select(&req(false)).name(), "local");
    }
}
