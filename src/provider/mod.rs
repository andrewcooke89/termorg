//! Terminal provider boundary (D10).
//!
//! UI/domain code depends on this trait — not on Kitty remote-control details.

mod kitty;

pub use kitty::KittyProvider;

use crate::agent::AgentClass;
use crate::attention::Attention;
use crate::error::Result;

/// Live session discovered from a terminal emulator (one tab).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSession {
    /// Provider id, e.g. `"kitty"`.
    pub provider: String,
    /// Stable within the provider while the tab lives.
    pub id: String,
    /// Best-effort human title.
    pub title: String,
    /// Optional working directory if the provider reports it.
    pub cwd: Option<String>,
    /// Whether this tab is focused in its OS window.
    pub is_focused: bool,
    /// OS window id if applicable (display only).
    pub os_window_id: Option<u32>,
    /// Provider endpoint for focus (e.g. `unix:/path/to.sock`). Opaque outside provider.
    pub focus_endpoint: Option<String>,
    /// Kitty tab id (or equivalent) for focus matching.
    pub focus_tab_id: Option<u32>,
    /// Kitty window id inside the tab (for focus-window).
    pub focus_window_id: Option<u32>,
    /// Detected agent/tool class (FS5).
    pub agent: AgentClass,
    /// Attention state (FS7).
    pub attention: Attention,
}

/// Capabilities a provider may support.
#[derive(Debug, Clone, Copy, Default)]
pub struct Capabilities {
    pub list: bool,
    pub focus: bool,
    pub launch: bool,
}

/// What to run in a newly launched tab (FS13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchKind {
    Shell,
    Claude,
    Grok,
    Kilo,
    Codex,
}

impl LaunchKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Claude => "claude",
            Self::Grok => "grok",
            Self::Kilo => "kilo",
            Self::Codex => "codex",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "shell" | "sh" | "zsh" | "bash" => Some(Self::Shell),
            "claude" => Some(Self::Claude),
            "grok" => Some(Self::Grok),
            "kilo" => Some(Self::Kilo),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }

    /// argv for Kitty `launch` (empty = default shell).
    pub fn command_argv(self) -> Vec<String> {
        match self {
            Self::Shell => Vec::new(),
            Self::Claude => vec!["claude".into()],
            Self::Grok => vec!["grok".into()],
            Self::Kilo => vec!["kilo".into()],
            Self::Codex => vec!["codex".into()],
        }
    }

    pub fn tab_title_hint(self, cwd: Option<&str>) -> String {
        let name = match self {
            Self::Shell => "shell",
            Self::Claude => "Claude",
            Self::Grok => "Grok",
            Self::Kilo => "Kilo",
            Self::Codex => "Codex",
        };
        if let Some(c) = cwd {
            let short = c.rsplit('/').next().unwrap_or(c);
            format!("{name} · {short}")
        } else {
            name.into()
        }
    }
}

/// Request to open a new terminal session (FS13).
#[derive(Debug, Clone)]
pub struct LaunchRequest {
    pub kind: LaunchKind,
    /// Working directory for the new tab.
    pub cwd: Option<String>,
    /// Kitty remote-control endpoint (`unix:…`). If None, provider picks one.
    pub endpoint: Option<String>,
    /// Optional tab title override.
    pub tab_title: Option<String>,
}

/// Result of a successful launch.
#[derive(Debug, Clone)]
pub struct LaunchResult {
    /// Kitty window id (when known).
    pub window_id: Option<u32>,
    /// Endpoint used.
    pub endpoint: String,
    /// Working directory requested.
    pub cwd: Option<String>,
    pub kind: LaunchKind,
}

#[cfg(test)]
mod launch_tests {
    use super::*;

    #[test]
    fn launch_kind_parse() {
        assert_eq!(LaunchKind::parse("claude"), Some(LaunchKind::Claude));
        assert_eq!(LaunchKind::parse("SHELL"), Some(LaunchKind::Shell));
        assert!(LaunchKind::parse("nope").is_none());
        assert!(LaunchKind::Claude.command_argv().contains(&"claude".into()));
        assert!(LaunchKind::Shell.command_argv().is_empty());
    }
}

/// Abstract access to a terminal host.
pub trait TerminalProvider: Send + Sync {
    fn provider_id(&self) -> &str;
    fn capabilities(&self) -> Capabilities;
    fn list_sessions(&self) -> Result<Vec<ProviderSession>>;

    /// Bring the given session to the front (FS4).
    fn focus(&self, session: &ProviderSession) -> Result<()>;

    /// Open a new tab/window (FS13). Default: unsupported.
    fn launch(&self, _req: &LaunchRequest) -> Result<LaunchResult> {
        Err(crate::error::TermorgError::ProviderCommand {
            message: format!(
                "launch not supported by provider `{}`",
                self.provider_id()
            ),
        })
    }
}
