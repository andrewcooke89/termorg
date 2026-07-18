//! FS15 — path hints for manual groups.
//!
//! When you assign a tab in a repo/path to a manual group, we learn
//! `path_key → group`. Unassigned tabs under that path get a suggestion:
//! "this path often belongs in Trading" → Accept / Dismiss.

use crate::path_group::path_group_for_cwd;
use crate::provider::ProviderSession;
use crate::store::UserState;

/// One actionable suggestion for a live session.
#[derive(Debug, Clone)]
pub struct PathHintSuggestion {
    pub session_id: String,
    pub session_title: String,
    pub cwd: String,
    pub path_key: String,
    pub path_title: String,
    pub group_id: String,
    pub group_title: String,
    pub hits: u32,
}

/// Stable path key for learning (git root abs path, or `path:…`).
pub fn path_key_for_session(session: &ProviderSession) -> Option<(String, String)> {
    let cwd = session.cwd.as_deref()?;
    let pg = path_group_for_cwd(Some(cwd));
    if pg.unknown {
        return None;
    }
    Some((pg.id, pg.title))
}

/// Build suggestions for unassigned sessions from learned path→group map.
pub fn suggestions(sessions: &[ProviderSession], state: &UserState) -> Vec<PathHintSuggestion> {
    let mut out = Vec::new();
    for s in sessions {
        if state.manual_group_for(s).is_some() {
            continue;
        }
        let Some((path_key, path_title)) = path_key_for_session(s) else {
            continue;
        };
        if state.is_hint_dismissed(&path_key) {
            continue;
        }
        let Some(hint) = state.best_hint_for_path(&path_key) else {
            continue;
        };
        let Some(group) = state.manual_groups.iter().find(|g| g.id == hint.group_id) else {
            continue;
        };
        out.push(PathHintSuggestion {
            session_id: s.id.clone(),
            session_title: s.title.replace('\n', " "),
            cwd: s.cwd.clone().unwrap_or_default(),
            path_key,
            path_title,
            group_id: group.id.clone(),
            group_title: group.title.clone(),
            hits: hint.hits,
        });
    }
    // Stable order: more evidence first, then path title.
    out.sort_by(|a, b| {
        b.hits
            .cmp(&a.hits)
            .then_with(|| a.path_title.cmp(&b.path_title))
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    out
}

/// Human one-liner for CLI/UI.
pub fn suggestion_label(s: &PathHintSuggestion) -> String {
    format!(
        "“{}” often belongs in ◆ {}  ({}× · {})",
        s.path_title, s.group_title, s.hits, s.session_id
    )
}

/// Rebuild path hints from existing assigned prefs (migration / repair).
pub fn rebuild_hints_from_prefs(state: &mut UserState, live: &[ProviderSession]) {
    for pref in &state.session_prefs.clone() {
        let Some(gid) = pref.manual_group_id.as_ref() else {
            continue;
        };
        if !state.manual_groups.iter().any(|g| &g.id == gid) {
            continue;
        }
        // Prefer live session cwd by id, else stored cwd.
        let cwd = live
            .iter()
            .find(|s| {
                s.provider == pref.provider
                    && matches!(&pref.match_rule, crate::store::SessionMatch::ProviderId { id } if id == &s.id)
            })
            .and_then(|s| s.cwd.clone())
            .or_else(|| pref.cwd.clone());
        let Some(cwd) = cwd else {
            continue;
        };
        let pg = path_group_for_cwd(Some(&cwd));
        if pg.unknown {
            continue;
        }
        state.record_path_hint(&pg.id, gid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentClass;
    use crate::attention::Attention;
    use crate::store::{ManualGroup, SessionMatch, SessionPref, UserState};

    fn sess(id: &str, cwd: &str) -> ProviderSession {
        ProviderSession {
            provider: "kitty".into(),
            id: id.into(),
            title: "t".into(),
            cwd: Some(cwd.into()),
            is_focused: false,
            os_window_id: None,
            focus_endpoint: None,
            focus_tab_id: None,
            focus_window_id: None,
            focus_key: None,
            agent: AgentClass::Shell,
            attention: Attention::Idle,
        }
    }

    #[test]
    fn learns_and_suggests() {
        let mut st = UserState::default();
        st.manual_groups.push(ManualGroup {
            id: "g1".into(),
            title: "Trading".into(),
            sort_index: 0,
        });
        // Fake path key directly
        st.record_path_hint("/home/u/trading-brain", "g1");
        st.record_path_hint("/home/u/trading-brain", "g1");

        let unassigned = sess("new", "/home/u/trading-brain/src");
        // path_group_for_cwd may use git root — for non-git path: key is path:abs
        // Force by using exact path key from path_group
        let (key, _) = path_key_for_session(&unassigned).unwrap_or_else(|| {
            (
                format!("path:{}", unassigned.cwd.as_ref().unwrap()),
                "x".into(),
            )
        });
        // If key differs from what we recorded, re-record with real key
        st.path_hints.clear();
        st.record_path_hint(&key, "g1");

        let sug = suggestions(&[unassigned], &st);
        assert_eq!(sug.len(), 1);
        assert_eq!(sug[0].group_title, "Trading");
    }

    #[test]
    fn dismissed_suppressed() {
        let mut st = UserState::default();
        st.manual_groups.push(ManualGroup {
            id: "g1".into(),
            title: "Trading".into(),
            sort_index: 0,
        });
        let s = sess("x", "/tmp/proj-a");
        let (key, _) = path_key_for_session(&s).expect("key");
        st.record_path_hint(&key, "g1");
        st.dismiss_path_hint(&key);
        assert!(suggestions(&[s], &st).is_empty());
    }

    #[test]
    fn already_assigned_no_suggest() {
        let mut st = UserState::default();
        st.manual_groups.push(ManualGroup {
            id: "g1".into(),
            title: "Trading".into(),
            sort_index: 0,
        });
        let s = sess("x", "/tmp/proj-b");
        let (key, _) = path_key_for_session(&s).expect("key");
        st.record_path_hint(&key, "g1");
        st.session_prefs.push(SessionPref {
            provider: "kitty".into(),
            match_rule: SessionMatch::ProviderId { id: "x".into() },
            manual_group_id: Some("g1".into()),
            priority: crate::store::Priority::Normal,
            explicit_priority: false,
            suppress_sticky_group: false,
            cwd: Some("/tmp/proj-b".into()),
            title: None,
            updated_at: 0,
        });
        assert!(suggestions(&[s], &st).is_empty());
    }
}
