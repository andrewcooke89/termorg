//! Stable machine-readable session list JSON (CLI `list --json`).
//!
//! **Contract (v1)** — each array element is an object with at least:
//! - `provider`, `id`, `title`, `cwd`, `agent`, `attention`, `priority`
//! - `focused` (bool) — also emitted as `is_focused` for older consumers
//! - `group` — manual group title if assigned, else null
//! - `group_id` / `manual_group_id` — same id, dual keys for stability
//! - `section_kind` (`manual`|`auto`), `section_title`
//!
//! Field names must not silently rename; add new keys only.

use serde::{Deserialize, Serialize};

use crate::provider::ProviderSession;
use crate::store::{build_display_sections, DisplaySection, UserState};

/// One session row in `list --json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionJson {
    pub provider: String,
    pub id: String,
    pub title: String,
    pub cwd: Option<String>,
    pub agent: String,
    pub attention: String,
    pub priority: String,
    /// Preferred stable name for focus state.
    pub focused: bool,
    /// Compat alias for `focused`.
    pub is_focused: bool,
    /// Manual group title when assigned, else null.
    pub group: Option<String>,
    /// Manual group id when assigned.
    pub group_id: Option<String>,
    /// Compat alias for `group_id`.
    pub manual_group_id: Option<String>,
    pub section_kind: String,
    pub section_title: String,
}

/// Required top-level keys on every session object (contract tests).
pub const REQUIRED_KEYS: &[&str] = &[
    "provider",
    "id",
    "title",
    "cwd",
    "agent",
    "attention",
    "priority",
    "focused",
    "is_focused",
    "group",
    "group_id",
    "manual_group_id",
    "section_kind",
    "section_title",
];

/// Build stable JSON rows from live sessions + prefs.
pub fn sessions_to_json(sessions: &[ProviderSession], state: &UserState) -> Vec<SessionJson> {
    let sections = build_display_sections(sessions.to_vec(), state);
    let mut out = Vec::new();
    for sec in sections {
        let (kind, section_title, group_id, group_title, members): (
            &str,
            String,
            Option<String>,
            Option<String>,
            Vec<ProviderSession>,
        ) = match sec {
            DisplaySection::Manual { group, sessions } => (
                "manual",
                group.title.clone(),
                Some(group.id.clone()),
                Some(group.title),
                sessions,
            ),
            DisplaySection::Auto {
                title, sessions, ..
            } => ("auto", title, None, None, sessions),
        };
        for s in members {
            let pri = state.priority_for(&s);
            // Prefer live membership over section grouping for group fields.
            let gid = state.manual_group_for(&s).or_else(|| group_id.clone());
            let gtitle = gid.as_ref().and_then(|id| {
                state
                    .manual_groups
                    .iter()
                    .find(|g| &g.id == id)
                    .map(|g| g.title.clone())
                    .or_else(|| group_title.clone())
            });
            out.push(SessionJson {
                provider: s.provider.clone(),
                id: s.id.clone(),
                title: s.title.clone(),
                cwd: s.cwd.clone(),
                agent: s.agent.as_str().to_string(),
                attention: s.attention.as_str().to_string(),
                priority: pri.as_str().to_string(),
                focused: s.is_focused,
                is_focused: s.is_focused,
                group: gtitle,
                group_id: gid.clone(),
                manual_group_id: gid,
                section_kind: kind.to_string(),
                section_title: section_title.clone(),
            });
        }
    }
    out
}

/// Serialize to pretty JSON string (array).
pub fn sessions_to_json_string(sessions: &[ProviderSession], state: &UserState) -> String {
    let rows = sessions_to_json(sessions, state);
    serde_json::to_string_pretty(&rows).unwrap_or_else(|_| "[]".into())
}

/// Validate a JSON value is an array of objects each containing REQUIRED_KEYS.
pub fn validate_json_contract(raw: &str) -> Result<(), String> {
    let v: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("invalid JSON: {e}"))?;
    let arr = v
        .as_array()
        .ok_or_else(|| "root must be a JSON array".to_string())?;
    for (i, item) in arr.iter().enumerate() {
        let obj = item
            .as_object()
            .ok_or_else(|| format!("item {i} is not an object"))?;
        for key in REQUIRED_KEYS {
            if !obj.contains_key(*key) {
                return Err(format!("item {i} missing required key `{key}`"));
            }
        }
        // focused and is_focused must agree when both present
        if let (Some(a), Some(b)) = (obj.get("focused"), obj.get("is_focused")) {
            if a != b {
                return Err(format!("item {i}: focused != is_focused"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentClass;
    use crate::attention::Attention;
    use crate::store::{ManualGroup, Priority, SessionMatch, SessionPref, UserState};

    fn sess(id: &str, provider: &str, title: &str, cwd: &str) -> ProviderSession {
        ProviderSession {
            provider: provider.into(),
            id: id.into(),
            title: title.into(),
            cwd: Some(cwd.into()),
            is_focused: true,
            os_window_id: None,
            focus_endpoint: None,
            focus_tab_id: None,
            focus_window_id: None,
            focus_key: None,
            agent: AgentClass::Claude,
            attention: Attention::NeedsYou,
        }
    }

    #[test]
    fn json_contract_keys_present_and_stable() {
        let mut state = UserState::default();
        state.manual_groups.push(ManualGroup {
            id: "g1".into(),
            title: "Trading".into(),
            sort_index: 0,
        });
        let s = sess("tmux:default:@1", "tmux", "main", "/tmp/proj");
        state.session_prefs.push(SessionPref {
            provider: "tmux".into(),
            match_rule: SessionMatch::ProviderId { id: s.id.clone() },
            manual_group_id: Some("g1".into()),
            priority: Priority::Important,
            cwd: s.cwd.clone(),
            title: Some(s.title.clone()),
            updated_at: 1,
        });
        let raw = sessions_to_json_string(std::slice::from_ref(&s), &state);
        validate_json_contract(&raw).expect(&raw);
        let rows: Vec<SessionJson> = serde_json::from_str(&raw).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].provider, "tmux");
        assert_eq!(rows[0].id, "tmux:default:@1");
        assert_eq!(rows[0].agent, "claude");
        assert_eq!(rows[0].attention, "needs_you");
        assert_eq!(rows[0].priority, "important");
        assert!(rows[0].focused);
        assert_eq!(rows[0].focused, rows[0].is_focused);
        assert_eq!(rows[0].group.as_deref(), Some("Trading"));
        assert_eq!(rows[0].group_id.as_deref(), Some("g1"));
        assert_eq!(rows[0].manual_group_id.as_deref(), Some("g1"));
        assert_eq!(rows[0].cwd.as_deref(), Some("/tmp/proj"));
    }

    #[test]
    fn json_empty_array_valid() {
        let state = UserState::default();
        let raw = sessions_to_json_string(&[], &state);
        assert_eq!(raw.trim(), "[]");
        validate_json_contract(&raw).unwrap();
    }

    #[test]
    fn required_keys_list_is_complete() {
        // Ensure REQUIRED_KEYS matches SessionJson serde names.
        let s = sess("k:1", "kitty", "t", "/x");
        let row = &sessions_to_json(std::slice::from_ref(&s), &UserState::default())[0];
        let v = serde_json::to_value(row).unwrap();
        let obj = v.as_object().unwrap();
        for key in REQUIRED_KEYS {
            assert!(obj.contains_key(*key), "missing {key}");
        }
    }
}
