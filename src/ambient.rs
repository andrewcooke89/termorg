//! FS12 — ambient Kitty cues (tab bar color + optional title prefix).
//!
//! Glance at the tab strip and get the same story as the panel:
//!   - **Color** → attention (needs_you / working) or agent when idle
//!   - **Title prefix** → `!` needs you · `…` working (optional)
//!
//! Disable: `TERMORG_AMBIENT=0` or `~/.config/termorg/ambient.json`.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::agent::AgentClass;
use crate::attention::Attention;
use crate::provider::{KittyProvider, ProviderSession};

const PREFIX_NEED: &str = "! ";
const PREFIX_WORK: &str = "… ";
const PREFIX_IDLE_AGENT: &str = "· ";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AmbientConfig {
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default = "default_true")]
    set_colors: bool,
    /// Rewrite tab titles with a short attention prefix.
    #[serde(default = "default_true")]
    set_titles: bool,
}

fn default_true() -> bool {
    true
}

impl Default for AmbientConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            set_colors: true,
            set_titles: true,
        }
    }
}

fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("TERMORG_CONFIG_DIR") {
        return PathBuf::from(p).join("ambient.json");
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".config").join("termorg").join("ambient.json")
}

fn load_config() -> AmbientConfig {
    if let Ok(v) = std::env::var("TERMORG_AMBIENT") {
        let off = matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "no"
        );
        if off {
            return AmbientConfig {
                enabled: false,
                ..AmbientConfig::default()
            };
        }
    }
    let path = config_path();
    let Ok(raw) = fs::read_to_string(&path) else {
        return AmbientConfig::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn ensure_default_config() {
    let path = config_path();
    if path.exists() {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(raw) = serde_json::to_string_pretty(&AmbientConfig::default()) {
        let _ = fs::write(path, raw + "\n");
    }
}

/// Desired tab bar colors (Kitty `set-tab-color` kwargs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabColorSpec {
    /// Serialized as `active_bg=#rrggbb` etc. Empty → reset all to NONE.
    pub args: Vec<String>,
    /// Stable signature for change detection.
    pub sig: String,
}

/// Compute colors for a session.
pub fn tab_colors(session: &ProviderSession) -> TabColorSpec {
    let (bg, fg) = match session.attention {
        Attention::NeedsYou => {
            let (r, g, b) = Attention::NeedsYou.rgb();
            (Some((r, g, b)), Some((26_u8, 27, 38)))
        }
        Attention::Working => {
            let (r, g, b) = Attention::Working.rgb();
            (Some((r, g, b)), Some((26, 27, 38)))
        }
        Attention::Idle | Attention::Unknown => {
            if Attention::is_agent(session.agent) {
                let (r, g, b) = session.agent.rgb();
                // Dim agent color for idle agent tabs.
                let bg = (
                    (r as u16 * 45 / 100) as u8,
                    (g as u16 * 45 / 100) as u8,
                    (b as u16 * 45 / 100) as u8,
                );
                (Some(bg), Some((192_u8, 202, 245)))
            } else {
                // Quiet shell → default Kitty tab colors.
                (None, None)
            }
        }
    };

    match bg {
        None => TabColorSpec {
            args: vec![
                "active_fg=NONE".into(),
                "active_bg=NONE".into(),
                "inactive_fg=NONE".into(),
                "inactive_bg=NONE".into(),
            ],
            sig: "none".into(),
        },
        Some((br, bg_, bb)) => {
            let (fr, fg_, fb) = fg.unwrap_or((192, 202, 245));
            // Inactive slightly darker.
            let ibr = (br as u16 * 70 / 100) as u8;
            let ibg = (bg_ as u16 * 70 / 100) as u8;
            let ibb = (bb as u16 * 70 / 100) as u8;
            let abg = format!("#{br:02x}{bg_:02x}{bb:02x}");
            let afg = format!("#{fr:02x}{fg_:02x}{fb:02x}");
            let ibg_s = format!("#{ibr:02x}{ibg:02x}{ibb:02x}");
            let sig = format!("{abg}/{afg}");
            TabColorSpec {
                args: vec![
                    format!("active_bg={abg}"),
                    format!("active_fg={afg}"),
                    format!("inactive_bg={ibg_s}"),
                    format!("inactive_fg={afg}"),
                ],
                sig,
            }
        }
    }
}

/// Strip our ambient prefixes so re-application doesn't stack.
/// Only exact prefixes we add — do **not** strip bare `…` (Kitty uses that in path titles).
pub fn strip_ambient_prefix(title: &str) -> String {
    let mut t = title.trim_start();
    loop {
        let next = t
            .strip_prefix(PREFIX_NEED) // "! "
            .or_else(|| t.strip_prefix(PREFIX_WORK)) // "… "
            .or_else(|| t.strip_prefix(PREFIX_IDLE_AGENT)); // "· "
        match next {
            Some(rest) => t = rest.trim_start(),
            None => break,
        }
    }
    t.to_string()
}

/// Desired tab title (optional prefix + original content).
pub fn desired_title(session: &ProviderSession) -> String {
    let base = strip_ambient_prefix(&session.title);
    let base = if base.is_empty() {
        session
            .cwd
            .as_deref()
            .map(|c| {
                let home = std::env::var("HOME").unwrap_or_default();
                if !home.is_empty() && c.starts_with(&home) {
                    format!("~{}", &c[home.len()..])
                } else {
                    c.to_string()
                }
            })
            .unwrap_or_else(|| session.agent.label().to_string())
    } else {
        base
    };
    // Keep titles short for the tab bar.
    let base = if base.chars().count() > 48 {
        let mut s: String = base.chars().take(46).collect();
        s.push('…');
        s
    } else {
        base
    };

    match session.attention {
        Attention::NeedsYou => format!("{PREFIX_NEED}{base}"),
        Attention::Working => format!("{PREFIX_WORK}{base}"),
        Attention::Idle | Attention::Unknown => {
            if Attention::is_agent(session.agent) {
                // Light agent mark when idle (color carries most of the signal).
                let base = strip_ambient_prefix(&session.title);
                if base.is_empty() {
                    format!("{PREFIX_IDLE_AGENT}{}", session.agent.label())
                } else {
                    format!("{PREFIX_IDLE_AGENT}{base}")
                }
            } else {
                // Shell: restore base without our prefixes; don't invent a new title.
                strip_ambient_prefix(&session.title)
            }
        }
    }
}

/// Applies cues only when they change (avoids RC spam).
pub struct AmbientApplier {
    last: HashMap<String, (String, String)>, // id -> (color_sig, title)
    cfg: AmbientConfig,
}

impl Default for AmbientApplier {
    fn default() -> Self {
        Self::new()
    }
}

impl AmbientApplier {
    pub fn new() -> Self {
        Self {
            last: HashMap::new(),
            cfg: load_config(),
        }
    }

    pub fn reload_config(&mut self) {
        self.cfg = load_config();
    }

    pub fn apply_all(&mut self, provider: &KittyProvider, sessions: &[ProviderSession]) {
        if !self.cfg.enabled {
            return;
        }
        let mut seen = std::collections::HashSet::new();
        for s in sessions {
            seen.insert(s.id.clone());
            self.apply_one(provider, s);
        }
        self.last.retain(|id, _| seen.contains(id));
    }

    fn apply_one(&mut self, provider: &KittyProvider, session: &ProviderSession) {
        let colors = tab_colors(session);
        let title = if self.cfg.set_titles {
            desired_title(session)
        } else {
            String::new()
        };
        let color_sig = if self.cfg.set_colors {
            colors.sig.clone()
        } else {
            String::new()
        };
        let prev = self.last.get(&session.id);
        let same = prev.is_some_and(|(c, t)| c == &color_sig && t == &title);
        if same {
            return;
        }

        if self.cfg.set_colors {
            let _ = provider.set_tab_color(session, &colors.args);
        }
        if self.cfg.set_titles && !title.is_empty() && session.title != title {
            let _ = provider.set_tab_title(session, &title);
        }

        self.last
            .insert(session.id.clone(), (color_sig, title));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentClass;
    use crate::attention::Attention;

    fn sess(agent: AgentClass, attention: Attention, title: &str) -> ProviderSession {
        ProviderSession {
            provider: "kitty".into(),
            id: "t".into(),
            title: title.into(),
            cwd: Some("/tmp/proj".into()),
            is_focused: false,
            os_window_id: None,
            focus_endpoint: None,
            focus_tab_id: Some(1),
            focus_window_id: None,
            agent,
            attention,
        }
    }

    #[test]
    fn needs_you_color_not_none() {
        let c = tab_colors(&sess(
            AgentClass::Claude,
            Attention::NeedsYou,
            "hi",
        ));
        assert_ne!(c.sig, "none");
        assert!(c.args.iter().any(|a| a.starts_with("active_bg=#")));
    }

    #[test]
    fn shell_idle_resets_color() {
        let c = tab_colors(&sess(AgentClass::Shell, Attention::Idle, "zsh"));
        assert_eq!(c.sig, "none");
    }

    #[test]
    fn strip_prefixes_idempotent() {
        assert_eq!(strip_ambient_prefix("! hello"), "hello");
        assert_eq!(strip_ambient_prefix("… ! hello"), "hello");
        assert_eq!(
            strip_ambient_prefix(&desired_title(&sess(
                AgentClass::Claude,
                Attention::NeedsYou,
                "! old"
            ))),
            "old"
        );
    }

    #[test]
    fn title_prefixes() {
        let n = desired_title(&sess(
            AgentClass::Claude,
            Attention::NeedsYou,
            "Sleep then say hi",
        ));
        assert!(n.starts_with(PREFIX_NEED), "{n}");
        let w = desired_title(&sess(AgentClass::Grok, Attention::Working, "thinking"));
        assert!(w.starts_with(PREFIX_WORK), "{w}");
    }
}
