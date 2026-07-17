//! Multi-backend provider: Kitty, tmux, or both merged.

use super::kitty::KittyProvider;
use super::tmux::TmuxProvider;
use super::{
    Capabilities, LaunchRequest, LaunchResult, ProviderSession, TerminalProvider,
};
use crate::error::{Result, TermorgError};

/// Which backend(s) to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Kitty,
    Tmux,
    /// Merge sessions from every available backend.
    All,
}

impl ProviderKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "kitty" => Some(Self::Kitty),
            "tmux" => Some(Self::Tmux),
            "all" | "auto" | "both" => Some(Self::All),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Kitty => "kitty",
            Self::Tmux => "tmux",
            Self::All => "all",
        }
    }
}

/// One or more backends behind a single [`TerminalProvider`].
pub enum MultiProvider {
    Kitty(KittyProvider),
    Tmux(TmuxProvider),
    Both {
        kitty: KittyProvider,
        tmux: TmuxProvider,
    },
}

impl MultiProvider {
    pub fn from_kind(kind: ProviderKind, kitty_to: Option<&str>) -> Result<Self> {
        let kitty = || {
            if let Some(to) = kitty_to {
                KittyProvider::with_listen_on(to)
            } else {
                KittyProvider::new()
            }
        };
        match kind {
            ProviderKind::Kitty => Ok(Self::Kitty(kitty())),
            ProviderKind::Tmux => Ok(Self::Tmux(TmuxProvider::new())),
            ProviderKind::All => {
                let k = kitty();
                let t = TmuxProvider::new();
                let k_ok = k.list_sessions().is_ok();
                let t_ok = t.list_sessions().is_ok();
                match (k_ok, t_ok) {
                    (true, true) => Ok(Self::Both { kitty: k, tmux: t }),
                    (true, false) => Ok(Self::Kitty(k)),
                    (false, true) => Ok(Self::Tmux(t)),
                    (false, false) => Err(TermorgError::ProviderUnavailable {
                        provider: "all".into(),
                        message: "neither Kitty nor tmux is available \
                             (enable Kitty remote control and/or start a tmux session)"
                            .into(),
                    }),
                }
            }
        }
    }
}

/// Detect a sensible default: both if possible, else whichever works, else Kitty.
pub fn detect_providers(kitty_to: Option<&str>) -> MultiProvider {
    MultiProvider::from_kind(ProviderKind::All, kitty_to).unwrap_or_else(|_| {
        if let Some(to) = kitty_to {
            MultiProvider::Kitty(KittyProvider::with_listen_on(to))
        } else {
            MultiProvider::Kitty(KittyProvider::new())
        }
    })
}

impl TerminalProvider for MultiProvider {
    fn provider_id(&self) -> &str {
        match self {
            Self::Kitty(_) => "kitty",
            Self::Tmux(_) => "tmux",
            Self::Both { .. } => "all",
        }
    }

    fn capabilities(&self) -> Capabilities {
        match self {
            Self::Kitty(k) => k.capabilities(),
            Self::Tmux(t) => t.capabilities(),
            Self::Both { kitty, tmux } => {
                let a = kitty.capabilities();
                let b = tmux.capabilities();
                Capabilities {
                    list: a.list || b.list,
                    focus: a.focus || b.focus,
                    launch: a.launch || b.launch,
                    ambient: a.ambient || b.ambient,
                }
            }
        }
    }

    fn list_sessions(&self) -> Result<Vec<ProviderSession>> {
        match self {
            Self::Kitty(k) => k.list_sessions(),
            Self::Tmux(t) => t.list_sessions(),
            Self::Both { kitty, tmux } => {
                let mut out = Vec::new();
                let mut errors = Vec::new();
                match kitty.list_sessions() {
                    Ok(mut s) => out.append(&mut s),
                    Err(e) => errors.push(format!("kitty: {e}")),
                }
                match tmux.list_sessions() {
                    Ok(mut s) => out.append(&mut s),
                    Err(e) => errors.push(format!("tmux: {e}")),
                }
                if out.is_empty() {
                    return Err(TermorgError::ProviderUnavailable {
                        provider: "all".into(),
                        message: errors.join("; "),
                    });
                }
                out.sort_by(|a, b| a.id.cmp(&b.id));
                Ok(out)
            }
        }
    }

    fn focus(&self, session: &ProviderSession) -> Result<()> {
        match self {
            Self::Kitty(k) => k.focus(session),
            Self::Tmux(t) => t.focus(session),
            Self::Both { kitty, tmux } => {
                if session.provider == "tmux" {
                    tmux.focus(session)
                } else {
                    kitty.focus(session)
                }
            }
        }
    }

    fn launch(&self, req: &LaunchRequest) -> Result<LaunchResult> {
        match self {
            Self::Kitty(k) => k.launch(req),
            Self::Tmux(t) => t.launch(req),
            Self::Both { kitty, tmux } => {
                // Prefer backend implied by endpoint, else try kitty then tmux.
                if let Some(ref ep) = req.endpoint {
                    if ep.starts_with("unix:") {
                        return kitty.launch(req);
                    }
                    // bare session name → tmux
                    if !ep.contains('/') && !ep.contains(':') {
                        return tmux.launch(req);
                    }
                }
                match kitty.launch(req) {
                    Ok(r) => Ok(r),
                    Err(_) => tmux.launch(req),
                }
            }
        }
    }

    fn set_tab_title(&self, session: &ProviderSession, title: &str) -> Result<()> {
        match self {
            Self::Kitty(k) => k.set_tab_title(session, title),
            Self::Tmux(t) => t.set_tab_title(session, title),
            Self::Both { kitty, tmux } => {
                if session.provider == "tmux" {
                    tmux.set_tab_title(session, title)
                } else {
                    kitty.set_tab_title(session, title)
                }
            }
        }
    }

    fn set_tab_color(&self, session: &ProviderSession, color_args: &[String]) -> Result<()> {
        match self {
            Self::Kitty(k) => k.set_tab_color(session, color_args),
            Self::Tmux(t) => t.set_tab_color(session, color_args),
            Self::Both { kitty, tmux } => {
                if session.provider == "tmux" {
                    tmux.set_tab_color(session, color_args)
                } else {
                    kitty.set_tab_color(session, color_args)
                }
            }
        }
    }

    fn prefer_launch_endpoint(&self, cwd: Option<&str>) -> Option<String> {
        match self {
            Self::Kitty(k) => k.prefer_launch_endpoint(cwd),
            Self::Tmux(t) => t.prefer_launch_endpoint(cwd),
            Self::Both { kitty, tmux } => kitty
                .prefer_launch_endpoint(cwd)
                .or_else(|| tmux.prefer_launch_endpoint(cwd)),
        }
    }
}
