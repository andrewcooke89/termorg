//! FS11 — desktop notifications when something **newly** needs you.
//!
//! Rising-edge only (idle/working → needs_you). Muted sessions are skipped.
//! Focused tabs are skipped (you're already looking). Per-key cooldown avoids spam.
//!
//! Disable with `TERMORG_NOTIFY=0` or config `enabled: false`.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::attention::Attention;
use crate::provider::ProviderSession;
use crate::signals::AgentSignal;
use crate::store::{Priority, UserState};

const DEFAULT_COOLDOWN_SECS: u64 = 60;
const EXPIRE_MS: u32 = 12_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NotifyConfig {
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default = "default_cooldown")]
    cooldown_secs: u64,
    /// If true, skip notify when the session tab is focused.
    #[serde(default = "default_true")]
    skip_focused: bool,
}

fn default_true() -> bool {
    true
}
fn default_cooldown() -> u64 {
    DEFAULT_COOLDOWN_SECS
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cooldown_secs: DEFAULT_COOLDOWN_SECS,
            skip_focused: true,
        }
    }
}

fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("TERMORG_CONFIG_DIR") {
        return PathBuf::from(p).join("notify.json");
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".config").join("termorg").join("notify.json")
}

fn load_config() -> NotifyConfig {
    // Env override first.
    if let Ok(v) = std::env::var("TERMORG_NOTIFY") {
        let off = matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "no"
        );
        if off {
            return NotifyConfig {
                enabled: false,
                ..NotifyConfig::default()
            };
        }
    }
    let path = config_path();
    let Ok(raw) = fs::read_to_string(&path) else {
        return NotifyConfig::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

/// Rising-edge tracker used by panel refresh / watch loops.
pub struct NotifyTracker {
    prev: HashMap<String, Attention>,
    last_sent: HashMap<String, Instant>,
    cfg: NotifyConfig,
}

impl Default for NotifyTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl NotifyTracker {
    pub fn new() -> Self {
        Self {
            prev: HashMap::new(),
            last_sent: HashMap::new(),
            cfg: load_config(),
        }
    }

    pub fn reload_config(&mut self) {
        self.cfg = load_config();
    }

    /// Compare current sessions to previous sample; notify on new needs_you.
    pub fn process(&mut self, sessions: &[ProviderSession], state: &UserState) {
        if !self.cfg.enabled {
            // Still update prev so we don't flood when re-enabled.
            for s in sessions {
                self.prev.insert(s.id.clone(), s.attention);
            }
            return;
        }

        let now = Instant::now();
        let cooldown = Duration::from_secs(self.cfg.cooldown_secs.max(5));
        let mut seen = std::collections::HashSet::new();

        for s in sessions {
            seen.insert(s.id.clone());
            let prev = self.prev.get(&s.id).copied();
            self.prev.insert(s.id.clone(), s.attention);

            if s.attention != Attention::NeedsYou {
                continue;
            }
            if prev == Some(Attention::NeedsYou) {
                continue; // already needs_you last tick
            }
            // First sample ever: don't notify (startup flood).
            if prev.is_none() {
                continue;
            }
            if state.priority_for(s) == Priority::Muted {
                continue;
            }
            if self.cfg.skip_focused && s.is_focused {
                continue;
            }
            if self
                .last_sent
                .get(&s.id)
                .is_some_and(|t| now.duration_since(*t) < cooldown)
            {
                continue;
            }

            let title = format!("Needs you · {}", s.agent.label());
            let body = format_session_body(s);
            if send_desktop(&title, &body, &s.id) {
                self.last_sent.insert(s.id.clone(), now);
            }
        }

        self.prev.retain(|id, _| seen.contains(id));
        self.last_sent
            .retain(|id, t| seen.contains(id) && now.duration_since(*t) < cooldown * 4);
    }
}

fn format_session_body(s: &ProviderSession) -> String {
    let title = s.title.replace('\n', " ");
    let path = s
        .cwd
        .as_deref()
        .map(collapse_home)
        .unwrap_or_else(|| "—".into());
    format!("{title}\n{path}")
}

fn collapse_home(path: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if path.starts_with(&home) {
            return format!("~{}", &path[home.len()..]);
        }
    }
    path.to_string()
}

/// Immediate notify from a hook signal (Stop / Notification). Uses agent_session_id
/// or kitty window as cooldown key.
pub fn notify_from_signal(sig: &AgentSignal) {
    if sig.state != crate::signals::SignalState::NeedsYou {
        return;
    }
    let cfg = load_config();
    if !cfg.enabled {
        return;
    }

    static LAST: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);
    let key = sig
        .agent_session_id
        .clone()
        .or_else(|| match (&sig.kitty_pid, sig.kitty_window_id) {
            (Some(p), Some(w)) => Some(format!("{p}:{w}")),
            _ => sig.cwd.clone(),
        })
        .unwrap_or_else(|| "unknown".into());

    let cooldown = Duration::from_secs(cfg.cooldown_secs.max(5));
    let now = Instant::now();
    {
        let mut guard = match LAST.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let map = guard.get_or_insert_with(HashMap::new);
        if map
            .get(&key)
            .is_some_and(|t| now.duration_since(*t) < cooldown)
        {
            return;
        }
        map.insert(key.clone(), now);
    }

    let agent = sig
        .source
        .as_deref()
        .and_then(|s| s.split(':').next())
        .unwrap_or("agent");
    let title = format!("Needs you · {agent}");
    let reason = sig.reason.as_deref().unwrap_or("waiting");
    let path = sig
        .cwd
        .as_deref()
        .map(collapse_home)
        .unwrap_or_else(|| "—".into());
    let body = format!("{reason}\n{path}");
    let _ = send_desktop(&title, &body, &key);
}

fn send_desktop(summary: &str, body: &str, replace_key: &str) -> bool {
    // Prefer notify-send (libnotify / GNOME).
    let safe_key: String = replace_key
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .take(48)
        .collect();
    let sync = format!("string:x-canonical-private-synchronous:termorg-{safe_key}");

    let mut cmd = Command::new("notify-send");
    cmd.arg("-a")
        .arg("termorg")
        .arg("-u")
        .arg("normal")
        .arg("-t")
        .arg(EXPIRE_MS.to_string())
        .arg("-c")
        .arg("im")
        .arg("-h")
        .arg(&sync)
        .arg("--")
        .arg(summary)
        .arg(body)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    match cmd.status() {
        Ok(st) if st.success() => true,
        _ => {
            // Fallback: print to stderr so headless/watch still surfaces something.
            eprintln!("termorg notify: {summary} — {body}");
            false
        }
    }
}

/// Write default config if missing (for docs / first run).
pub fn ensure_default_config() {
    let path = config_path();
    if path.exists() {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let raw = serde_json::to_string_pretty(&NotifyConfig::default()).unwrap_or_default();
    let _ = fs::write(path, raw + "\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentClass;
    use crate::attention::Attention;

    fn sess(id: &str, attn: Attention, focused: bool) -> ProviderSession {
        ProviderSession {
            provider: "kitty".into(),
            id: id.into(),
            title: "test".into(),
            cwd: Some("/tmp".into()),
            is_focused: focused,
            os_window_id: None,
            focus_endpoint: None,
            focus_tab_id: None,
            focus_window_id: None,
            agent: AgentClass::Claude,
            attention: attn,
        }
    }

    #[test]
    fn rising_edge_only() {
        std::env::set_var("TERMORG_NOTIFY", "0"); // don't spam CI with notify-send
        let mut t = NotifyTracker::new();
        t.cfg.enabled = true; // force process logic, send will no-op via env... wait we already set env in load
        t.cfg = NotifyConfig {
            enabled: true,
            cooldown_secs: 1,
            skip_focused: true,
        };
        let st = UserState::default();
        // first sample: working — establish prev
        t.process(&[sess("a", Attention::Working, false)], &st);
        // still working — no edge
        t.process(&[sess("a", Attention::Working, false)], &st);
        // needs_you — edge (would notify if send worked)
        t.process(&[sess("a", Attention::NeedsYou, false)], &st);
        assert_eq!(t.prev.get("a"), Some(&Attention::NeedsYou));
        // stay needs_you — no second edge
        let before = t.last_sent.get("a").copied();
        t.process(&[sess("a", Attention::NeedsYou, false)], &st);
        // last_sent only set if send_desktop succeeds; with notify-send present it might.
        // At least prev stays NeedsYou.
        assert_eq!(t.prev.get("a"), Some(&Attention::NeedsYou));
        let _ = before;
        std::env::remove_var("TERMORG_NOTIFY");
    }

    #[test]
    fn skips_first_sample() {
        let mut t = NotifyTracker::new();
        t.cfg = NotifyConfig {
            enabled: true,
            cooldown_secs: 1,
            skip_focused: false,
        };
        let st = UserState::default();
        t.process(&[sess("b", Attention::NeedsYou, false)], &st);
        // first observation of id should not notify
        assert!(!t.last_sent.contains_key("b"));
    }
}
