//! Agent-driven attention signals (Claude/Codex/… hooks → termorg).
//!
//! CPU sampling lies while agents wait on the network or sleep children.
//! Lifecycle hooks are ground truth:
//!   - Notification (permission / idle) → needs_you
//!   - Stop → needs_you (turn finished, waiting on human)
//!   - UserPromptSubmit / PreToolUse → working
//!
//! Stored under `~/.config/termorg/signals.json`, matched to tabs via
//! KITTY_PID + KITTY_WINDOW_ID (preferred) or cwd (fallback).

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::attention::Attention;
use crate::error::{Result, TermorgError};

const SCHEMA: u32 = 1;
/// Drop signals older than this (stale after long idle / restart).
const DEFAULT_TTL_SECS: u64 = 6 * 3600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalState {
    NeedsYou,
    Working,
    Idle,
}

impl SignalState {
    pub fn as_attention(self) -> Attention {
        match self {
            Self::NeedsYou => Attention::NeedsYou,
            Self::Working => Attention::Working,
            Self::Idle => Attention::Idle,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "needs_you" | "needs-you" | "need" | "wait" => Some(Self::NeedsYou),
            "working" | "work" | "busy" => Some(Self::Working),
            "idle" | "clear" | "done" => Some(Self::Idle),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSignal {
    pub state: SignalState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kitty_pid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kitty_window_id: Option<u32>,
    pub updated_at: u64,
    #[serde(default = "default_ttl")]
    pub ttl_secs: u64,
}

fn default_ttl() -> u64 {
    DEFAULT_TTL_SECS
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SignalFile {
    schema: u32,
    #[serde(default)]
    signals: Vec<AgentSignal>,
}

/// How to match a live tab to a stored signal.
#[derive(Debug, Clone, Copy, Default)]
pub struct MatchHint<'a> {
    pub cwd: Option<&'a str>,
    pub kitty_pid: Option<&'a str>,
    pub kitty_window_id: Option<u32>,
}

pub fn signals_path() -> PathBuf {
    dirs_config().join("signals.json")
}

fn dirs_config() -> PathBuf {
    if let Ok(p) = std::env::var("TERMORG_CONFIG_DIR") {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".config").join("termorg")
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn load_file() -> SignalFile {
    let path = signals_path();
    let Ok(raw) = fs::read_to_string(&path) else {
        return SignalFile {
            schema: SCHEMA,
            signals: Vec::new(),
        };
    };
    serde_json::from_str(&raw).unwrap_or(SignalFile {
        schema: SCHEMA,
        signals: Vec::new(),
    })
}

fn save_file(file: &SignalFile) -> Result<()> {
    let path = signals_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(file).map_err(|e| TermorgError::Parse {
        message: format!("serialize signals: {e}"),
    })?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, raw.as_bytes())?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

fn is_fresh(sig: &AgentSignal, now: u64) -> bool {
    now.saturating_sub(sig.updated_at) <= sig.ttl_secs.max(1)
}

/// Best matching fresh signal for a tab, if any.
pub fn lookup(hint: MatchHint<'_>) -> Option<AgentSignal> {
    let now = now_secs();
    let file = load_file();
    let mut best: Option<(u8, AgentSignal)> = None; // lower rank = better

    for sig in file.signals {
        if !is_fresh(&sig, now) {
            continue;
        }
        // IMPORTANT: do not use `?` here — a non-match on one signal must not
        // abort the whole lookup (that dropped every match after the first miss).
        let Some(rank) = match_rank(&sig, hint) else {
            continue;
        };
        match &best {
            None => best = Some((rank, sig)),
            Some((br, prev)) if rank < *br || (rank == *br && sig.updated_at > prev.updated_at) => {
                best = Some((rank, sig));
            }
            _ => {}
        }
    }
    best.map(|(_, s)| s)
}

fn match_rank(sig: &AgentSignal, hint: MatchHint<'_>) -> Option<u8> {
    // 0 = exact kitty window in same instance
    if let (Some(sp), Some(hp), Some(sw), Some(hw)) = (
        sig.kitty_pid.as_deref(),
        hint.kitty_pid,
        sig.kitty_window_id,
        hint.kitty_window_id,
    ) {
        if sp == hp && sw == hw {
            return Some(0);
        }
    }
    // 1 = same kitty instance + cwd
    if let (Some(sp), Some(hp), Some(sc), Some(hc)) = (
        sig.kitty_pid.as_deref(),
        hint.kitty_pid,
        sig.cwd.as_deref(),
        hint.cwd,
    ) {
        if sp == hp && paths_equal(sc, hc) {
            return Some(1);
        }
    }
    // 2 = cwd only (multiple agents in same dir → latest wins via updated_at)
    if let (Some(sc), Some(hc)) = (sig.cwd.as_deref(), hint.cwd) {
        if paths_equal(sc, hc) {
            return Some(2);
        }
    }
    None
}

fn paths_equal(a: &str, b: &str) -> bool {
    let na = a.trim_end_matches('/');
    let nb = b.trim_end_matches('/');
    na == nb
}

/// Record a signal (upsert by agent_session_id, else by kitty window, else append).
pub fn record(mut sig: AgentSignal) -> Result<()> {
    if sig.updated_at == 0 {
        sig.updated_at = now_secs();
    }
    if sig.ttl_secs == 0 {
        sig.ttl_secs = DEFAULT_TTL_SECS;
    }
    let mut file = load_file();
    file.schema = SCHEMA;
    let now = now_secs();
    file.signals.retain(|s| is_fresh(s, now));

    let idx = file.signals.iter().position(|s| same_slot(s, &sig));
    if let Some(i) = idx {
        file.signals[i] = sig;
    } else {
        file.signals.push(sig);
    }
    // Cap size
    if file.signals.len() > 256 {
        file.signals
            .sort_by_key(|s| std::cmp::Reverse(s.updated_at));
        file.signals.truncate(256);
    }
    save_file(&file)
}

fn same_slot(a: &AgentSignal, b: &AgentSignal) -> bool {
    if let (Some(x), Some(y)) = (&a.agent_session_id, &b.agent_session_id) {
        if !x.is_empty() && x == y {
            return true;
        }
    }
    if let (Some(ap), Some(bp), Some(aw), Some(bw)) = (
        a.kitty_pid.as_deref(),
        b.kitty_pid.as_deref(),
        a.kitty_window_id,
        b.kitty_window_id,
    ) {
        if ap == bp && aw == bw {
            return true;
        }
    }
    false
}

pub fn list_signals() -> Vec<AgentSignal> {
    let now = now_secs();
    let mut file = load_file();
    file.signals.retain(|s| is_fresh(s, now));
    file.signals
        .sort_by_key(|s| std::cmp::Reverse(s.updated_at));
    file.signals
}

/// Ingest agent lifecycle hook JSON (Claude / Grok Build / Codex / compatible).
///
/// Accepts snake_case (`hook_event_name`) and camelCase (`hookEventName`),
/// PascalCase events, and Grok-style `pre_tool_use` names.
///
/// Event → attention:
///   Notification / PermissionRequest → needs_you
///   Stop / StopFailure → needs_you
///   UserPromptSubmit / PreToolUse / PostToolUse → working
///   SessionEnd → idle
/// Returns `Ok(None)` for events that should not change attention (e.g. SessionStart).
pub fn ingest_hook_json(raw: &str) -> Result<Option<AgentSignal>> {
    let v: Value = serde_json::from_str(raw).map_err(|e| TermorgError::Parse {
        message: format!("hook JSON: {e}"),
    })?;

    let event_raw =
        json_str(&v, &["hook_event_name", "hookEventName", "event"]).unwrap_or_default();
    let event = normalize_event_name(&event_raw);

    // SessionStart: no attention change (avoid clobbering on resume).
    if event == "SessionStart" || event == "PreCompact" || event == "PostCompact" {
        return Ok(None);
    }

    let notif_type = json_str(&v, &["notification_type", "notificationType"]).unwrap_or_default();

    let state = match event.as_str() {
        "Notification" | "PermissionRequest" => SignalState::NeedsYou,
        "Stop" | "StopFailure" => SignalState::NeedsYou,
        "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "SubagentStart" => SignalState::Working,
        "SessionEnd" => SignalState::Idle,
        // Unknown: still treat idle/permission-ish messages as needs_you if present
        _ if !notif_type.is_empty()
            || json_str(&v, &["message", "title", "body"]).is_some_and(|m| {
                let m = m.to_lowercase();
                m.contains("permission")
                    || m.contains("waiting")
                    || m.contains("input")
                    || m.contains("needs your")
                    || m.contains("attention")
            }) =>
        {
            SignalState::NeedsYou
        }
        _ => {
            return Err(TermorgError::Parse {
                message: format!("unhandled hook_event_name `{event_raw}` — pass --state instead"),
            });
        }
    };

    let reason = if !notif_type.is_empty() {
        Some(notif_type)
    } else if !event.is_empty() {
        Some(event.clone())
    } else {
        None
    };

    let agent_session_id = json_str(
        &v,
        &["session_id", "sessionId", "thread_id", "threadId", "id"],
    );
    let cwd = json_str(
        &v,
        &["cwd", "workingDirectory", "workspaceRoot", "workspace_root"],
    )
    .or_else(|| std::env::var("PWD").ok());

    let kitty_pid = std::env::var("KITTY_PID").ok().filter(|s| !s.is_empty());
    let kitty_window_id = std::env::var("KITTY_WINDOW_ID")
        .ok()
        .and_then(|s| s.parse().ok());

    let agent_hint = detect_agent_source(&v);
    let source = Some(format!("{agent_hint}:{event}"));

    let sig = AgentSignal {
        state,
        reason,
        source,
        agent_session_id,
        cwd,
        kitty_pid,
        kitty_window_id,
        updated_at: now_secs(),
        ttl_secs: DEFAULT_TTL_SECS,
    };
    record(sig.clone())?;
    Ok(Some(sig))
}

fn json_str(v: &Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(s) = v.get(*k).and_then(|x| x.as_str()) {
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

/// Normalize `pre_tool_use` / `preToolUse` / `PreToolUse` → `PreToolUse`.
fn normalize_event_name(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() {
        return String::new();
    }
    // Already PascalCase event names used by Claude/Codex
    const KNOWN: &[&str] = &[
        "Notification",
        "PermissionRequest",
        "Stop",
        "StopFailure",
        "UserPromptSubmit",
        "PreToolUse",
        "PostToolUse",
        "PostToolUseFailure",
        "SubagentStart",
        "SubagentStop",
        "SessionStart",
        "SessionEnd",
        "PreCompact",
        "PostCompact",
    ];
    for k in KNOWN {
        if s.eq_ignore_ascii_case(k) {
            return (*k).to_string();
        }
    }
    // snake_case / kebab → PascalCase chunks
    let parts: Vec<&str> = s.split(['_', '-', ' ']).filter(|p| !p.is_empty()).collect();
    if parts.len() > 1 {
        return parts
            .iter()
            .map(|p| {
                let mut c = p.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                }
            })
            .collect();
    }
    // camelCase → PascalCase
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

fn detect_agent_source(v: &Value) -> &'static str {
    // Explicit markers
    if let Some(s) = json_str(v, &["source", "agent", "product", "client"]) {
        let l = s.to_lowercase();
        if l.contains("codex") {
            return "codex";
        }
        if l.contains("grok") {
            return "grok";
        }
        if l.contains("kilo") {
            return "kilo";
        }
        if l.contains("claude") {
            return "claude";
        }
    }
    // Heuristics from payload shape / env
    if std::env::var_os("CODEX_THREAD_ID").is_some()
        || std::env::var_os("CODEX_CI").is_some()
        || v.get("turn_id").is_some()
            && v.get("hook_event_name").is_some()
            && v.get("model").is_some()
    {
        return "codex";
    }
    if std::env::var_os("GROK_SESSION_ID").is_some() || v.get("hookEventName").is_some() {
        return "grok";
    }
    if std::env::var_os("KILO_SESSION_ID").is_some() {
        return "kilo";
    }
    "agent"
}

/// Manual record with env-derived location keys.
pub fn record_manual(state: SignalState, reason: &str) -> Result<AgentSignal> {
    let sig = AgentSignal {
        state,
        reason: Some(reason.to_string()),
        source: Some("manual".into()),
        agent_session_id: None,
        cwd: std::env::var("PWD").ok(),
        kitty_pid: std::env::var("KITTY_PID").ok().filter(|s| !s.is_empty()),
        kitty_window_id: std::env::var("KITTY_WINDOW_ID")
            .ok()
            .and_then(|s| s.parse().ok()),
        updated_at: now_secs(),
        ttl_secs: DEFAULT_TTL_SECS,
    };
    record(sig.clone())?;
    Ok(sig)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

    fn with_temp_config<F: FnOnce()>(f: F) {
        let _g = LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("termorg-sig-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        std::env::set_var("TERMORG_CONFIG_DIR", &dir);
        f();
        let _ = fs::remove_dir_all(&dir);
        std::env::remove_var("TERMORG_CONFIG_DIR");
    }

    #[test]
    fn notification_hook_needs_you() {
        with_temp_config(|| {
            std::env::set_var("KITTY_PID", "999");
            std::env::set_var("KITTY_WINDOW_ID", "7");
            let raw = r#"{
              "hook_event_name": "Notification",
              "notification_type": "idle_prompt",
              "session_id": "sess-1",
              "cwd": "/tmp/proj"
            }"#;
            let sig = ingest_hook_json(raw).unwrap().expect("recorded");
            assert_eq!(sig.state, SignalState::NeedsYou);
            let found = lookup(MatchHint {
                cwd: Some("/tmp/proj"),
                kitty_pid: Some("999"),
                kitty_window_id: Some(7),
            });
            assert_eq!(found.unwrap().state, SignalState::NeedsYou);
            std::env::remove_var("KITTY_PID");
            std::env::remove_var("KITTY_WINDOW_ID");
        });
    }

    #[test]
    fn pre_tool_sets_working_and_overrides_slot() {
        with_temp_config(|| {
            std::env::set_var("KITTY_PID", "1");
            std::env::set_var("KITTY_WINDOW_ID", "2");
            ingest_hook_json(r#"{"hook_event_name":"Stop","session_id":"s","cwd":"/x"}"#).unwrap();
            let sig =
                ingest_hook_json(r#"{"hook_event_name":"PreToolUse","session_id":"s","cwd":"/x"}"#)
                    .unwrap()
                    .expect("recorded");
            assert_eq!(sig.state, SignalState::Working);
            let found = lookup(MatchHint {
                cwd: Some("/x"),
                kitty_pid: Some("1"),
                kitty_window_id: Some(2),
            })
            .unwrap();
            assert_eq!(found.state, SignalState::Working);
            std::env::remove_var("KITTY_PID");
            std::env::remove_var("KITTY_WINDOW_ID");
        });
    }

    #[test]
    fn lookup_skips_non_matching_signals() {
        // Regression: early `?` on match_rank aborted after the first miss.
        with_temp_config(|| {
            record(AgentSignal {
                state: SignalState::Working,
                reason: Some("other".into()),
                source: None,
                agent_session_id: Some("a".into()),
                cwd: Some("/other".into()),
                kitty_pid: Some("111".into()),
                kitty_window_id: Some(9),
                updated_at: now_secs(),
                ttl_secs: DEFAULT_TTL_SECS,
            })
            .unwrap();
            record(AgentSignal {
                state: SignalState::NeedsYou,
                reason: Some("me".into()),
                source: None,
                agent_session_id: Some("b".into()),
                cwd: Some("/proj".into()),
                kitty_pid: Some("222".into()),
                kitty_window_id: Some(3),
                updated_at: now_secs(),
                ttl_secs: DEFAULT_TTL_SECS,
            })
            .unwrap();
            let found = lookup(MatchHint {
                cwd: Some("/proj"),
                kitty_pid: Some("222"),
                kitty_window_id: Some(3),
            })
            .expect("second signal must match even when first does not");
            assert_eq!(found.state, SignalState::NeedsYou);
            assert_eq!(found.reason.as_deref(), Some("me"));
        });
    }

    #[test]
    fn grok_camel_case_and_snake_events() {
        with_temp_config(|| {
            std::env::set_var("KITTY_PID", "5");
            std::env::set_var("KITTY_WINDOW_ID", "1");
            let sig = ingest_hook_json(
                r#"{"hookEventName":"stop","sessionId":"g1","cwd":"/g","workspaceRoot":"/g"}"#,
            )
            .unwrap()
            .expect("recorded");
            assert_eq!(sig.state, SignalState::NeedsYou);
            let w =
                ingest_hook_json(r#"{"hookEventName":"pre_tool_use","sessionId":"g1","cwd":"/g"}"#)
                    .unwrap()
                    .expect("recorded");
            assert_eq!(w.state, SignalState::Working);
            assert!(ingest_hook_json(
                r#"{"hookEventName":"session_start","sessionId":"g1","cwd":"/g"}"#
            )
            .unwrap()
            .is_none());
            std::env::remove_var("KITTY_PID");
            std::env::remove_var("KITTY_WINDOW_ID");
        });
    }

    #[test]
    fn codex_permission_request_needs_you() {
        with_temp_config(|| {
            let sig = ingest_hook_json(
                r#"{"hook_event_name":"PermissionRequest","session_id":"c1","cwd":"/c","turn_id":"t","tool_name":"Bash"}"#,
            )
            .unwrap()
            .expect("recorded");
            assert_eq!(sig.state, SignalState::NeedsYou);
        });
    }
}
