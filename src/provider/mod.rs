//! Terminal provider boundary (D10).
//!
//! Domain/UI code depends on [`TerminalProvider`] — not on Kitty/tmux details.

mod kitty;
mod multi;
mod tmux;

pub use kitty::KittyProvider;
pub use multi::{detect_providers, MultiProvider, ProviderKind};
pub use tmux::{tmux_available, TmuxProvider};

use crate::agent::AgentClass;
use crate::attention::Attention;
use crate::error::Result;

/// Live session discovered from a terminal host (one tab / tmux window).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSession {
    /// Provider id, e.g. `"kitty"` or `"tmux"`.
    pub provider: String,
    /// Stable within the provider while the tab/window lives.
    pub id: String,
    /// Best-effort human title.
    pub title: String,
    /// Optional working directory if the provider reports it.
    pub cwd: Option<String>,
    /// Whether this tab/window is focused in its host.
    pub is_focused: bool,
    /// OS window id if applicable (display only).
    pub os_window_id: Option<u32>,
    /// Provider endpoint for control (Kitty socket, tmux socket name, …).
    pub focus_endpoint: Option<String>,
    /// Tab/window index when numeric.
    pub focus_tab_id: Option<u32>,
    /// Nested window/pane numeric id when applicable.
    pub focus_window_id: Option<u32>,
    /// Opaque focus target for the provider (e.g. tmux `session:@12|%5`).
    pub focus_key: Option<String>,
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
    pub ambient: bool,
}

/// What to run in a newly launched tab/window (FS13).
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

    /// argv for the child process (empty = default shell).
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
    /// Working directory for the new tab/window.
    pub cwd: Option<String>,
    /// Provider-specific launch context (Kitty socket, tmux session name, …).
    pub endpoint: Option<String>,
    /// Optional tab/window title override.
    pub tab_title: Option<String>,
}

/// Result of a successful launch.
#[derive(Debug, Clone)]
pub struct LaunchResult {
    /// Numeric window id when available (Kitty).
    pub window_id: Option<u32>,
    /// Provider context used (socket / session).
    pub endpoint: String,
    /// Working directory requested.
    pub cwd: Option<String>,
    pub kind: LaunchKind,
    /// Opaque native id (e.g. tmux `@12`).
    pub native_id: Option<String>,
}

/// Exact launch rematch: native id **and** endpoint/session identity.
/// Never falls back to cwd alone (would mis-assign sticky path groups).
pub fn session_matches_launch(session: &ProviderSession, result: &LaunchResult) -> bool {
    let id_ok = if let Some(ref nid) = result.native_id {
        session_matches_native_id(session, nid)
    } else if let Some(wid) = result.window_id {
        session.focus_window_id == Some(wid)
    } else {
        return false;
    };
    if !id_ok {
        return false;
    }
    launch_endpoint_matches(session, &result.endpoint)
}

fn launch_endpoint_matches(session: &ProviderSession, endpoint: &str) -> bool {
    if endpoint.is_empty() {
        return true;
    }
    if session.focus_endpoint.as_deref() == Some(endpoint) {
        return true;
    }
    // tmux launch endpoint is session name; focus_endpoint is socket tag.
    if session.provider == "tmux" {
        if let Some(ref key) = session.focus_key {
            if let Some((sess, _, _)) = crate::provider::tmux::TmuxProvider::parse_focus_key(key) {
                return sess == endpoint;
            }
        }
    }
    false
}

/// Exact match of a listed session against a launch `native_id`.
///
/// Must **not** use substring search: tmux `@1` must not match `@10`/`@11`/…
/// Compares the id tail after the last `:`, `focus_window_id`, and the window
/// token in `focus_key` (`session:@N|%pane`).
pub fn session_matches_native_id(session: &ProviderSession, native_id: &str) -> bool {
    let nid = native_id.trim();
    if nid.is_empty() {
        return false;
    }
    if session.id == nid {
        return true;
    }
    // e.g. `tmux:default:@12` → `@12`
    if session.id.rsplit(':').next() == Some(nid) {
        return true;
    }
    if let Some(w) = session.focus_window_id {
        if format!("@{w}") == nid || w.to_string() == nid {
            return true;
        }
    }
    // focus_key window token (tmux `session:@N` or `session:@N|%pane`)
    if let Some(ref key) = session.focus_key {
        let base = key.split('|').next().unwrap_or(key.as_str());
        if let Some(win) = base.rsplit(':').next() {
            if win == nid {
                return true;
            }
        }
    }
    false
}

/// Abstract access to a terminal host.
pub trait TerminalProvider: Send + Sync {
    fn provider_id(&self) -> &str;
    fn capabilities(&self) -> Capabilities;
    fn list_sessions(&self) -> Result<Vec<ProviderSession>>;

    /// Bring the given session to the front.
    fn focus(&self, session: &ProviderSession) -> Result<()>;

    /// Open a new tab/window. Default: unsupported.
    fn launch(&self, _req: &LaunchRequest) -> Result<LaunchResult> {
        Err(crate::error::TermorgError::ProviderCommand {
            message: format!("launch not supported by provider `{}`", self.provider_id()),
        })
    }

    /// Ambient title (optional).
    fn set_tab_title(&self, _session: &ProviderSession, _title: &str) -> Result<()> {
        Err(crate::error::TermorgError::ProviderCommand {
            message: format!(
                "ambient titles not supported by provider `{}`",
                self.provider_id()
            ),
        })
    }

    /// Ambient colors (optional). `color_args` are Kitty-style `active_bg=#rrggbb` tokens.
    fn set_tab_color(&self, _session: &ProviderSession, _color_args: &[String]) -> Result<()> {
        Err(crate::error::TermorgError::ProviderCommand {
            message: format!(
                "ambient colors not supported by provider `{}`",
                self.provider_id()
            ),
        })
    }

    /// Hint for launch placement (socket / session name).
    fn prefer_launch_endpoint(&self, _cwd: Option<&str>) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod launch_tests {
    use super::*;
    use crate::agent::AgentClass;
    use crate::attention::Attention;

    fn sess(id: &str, focus_key: Option<&str>, focus_window_id: Option<u32>) -> ProviderSession {
        ProviderSession {
            provider: "tmux".into(),
            id: id.into(),
            title: "t".into(),
            cwd: None,
            is_focused: false,
            os_window_id: None,
            focus_endpoint: None,
            focus_tab_id: None,
            focus_window_id,
            focus_key: focus_key.map(|s| s.into()),
            agent: AgentClass::Shell,
            attention: Attention::Idle,
        }
    }

    #[test]
    fn launch_kind_parse() {
        assert_eq!(LaunchKind::parse("claude"), Some(LaunchKind::Claude));
        assert_eq!(LaunchKind::parse("SHELL"), Some(LaunchKind::Shell));
        assert!(LaunchKind::parse("nope").is_none());
        assert!(LaunchKind::Claude.command_argv().contains(&"claude".into()));
        assert!(LaunchKind::Shell.command_argv().is_empty());
    }

    #[test]
    fn native_id_at1_does_not_match_at10() {
        let s10 = sess("tmux:default:@10", Some("work:@10|%1"), Some(10));
        let s11 = sess("tmux:default:@11", Some("work:@11|%2"), Some(11));
        let s1 = sess("tmux:default:@1", Some("work:@1|%0"), Some(1));
        assert!(!session_matches_native_id(&s10, "@1"));
        assert!(!session_matches_native_id(&s11, "@1"));
        assert!(session_matches_native_id(&s1, "@1"));
        assert!(session_matches_native_id(&s10, "@10"));
        // @1 absent among @10/@11 only
        let list = [s10, s11];
        assert!(list.iter().all(|s| !session_matches_native_id(s, "@1")));
        assert!(list.iter().any(|s| session_matches_native_id(s, "@10")));
    }
}
