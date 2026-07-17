//! FS10 — session search / filter.
//!
//! Match a free-text query against session title, cwd, agent, attention,
//! priority, id, and optional group title. Empty query matches everything.

use crate::agent::AgentClass;
use crate::attention::Attention;
use crate::provider::ProviderSession;
use crate::store::{Priority, UserState};

/// True if `query` (case-insensitive, whitespace-split AND tokens) matches
/// any of the session's searchable fields.
pub fn session_matches(
    session: &ProviderSession,
    state: &UserState,
    query: &str,
    group_title: Option<&str>,
) -> bool {
    let q = query.trim();
    if q.is_empty() {
        return true;
    }
    let hay = build_haystack(session, state, group_title);
    // All tokens must appear (order-independent).
    q.split_whitespace()
        .all(|tok| hay.contains(&tok.to_lowercase()))
}

fn build_haystack(
    session: &ProviderSession,
    state: &UserState,
    group_title: Option<&str>,
) -> String {
    let mut parts: Vec<&str> = Vec::with_capacity(12);
    parts.push(session.title.as_str());
    parts.push(session.id.as_str());
    parts.push(session.provider.as_str());
    parts.push(session.agent.as_str());
    parts.push(session.agent.label());
    parts.push(session.attention.as_str());
    parts.push(session.attention.label());
    if let Some(cwd) = session.cwd.as_deref() {
        parts.push(cwd);
    }
    let pri = state.priority_for(session);
    parts.push(pri.as_str());
    match pri {
        Priority::Important => parts.push("star important ★"),
        Priority::Muted => parts.push("mute muted"),
        Priority::Normal => {}
    }
    // Extra agent aliases people type
    match session.agent {
        AgentClass::Claude => parts.push("claude anthropic"),
        AgentClass::Grok => parts.push("grok xai"),
        AgentClass::Kilo => parts.push("kilo"),
        AgentClass::Codex => parts.push("codex openai"),
        AgentClass::Shell => parts.push("shell zsh bash"),
        AgentClass::Other | AgentClass::Unknown => {}
    }
    match session.attention {
        Attention::NeedsYou => parts.push("needs you need"),
        Attention::Working => parts.push("work busy"),
        Attention::Idle => parts.push("idle quiet"),
        Attention::Unknown => {}
    }
    if let Some(g) = group_title {
        parts.push(g);
    }
    if let Some(gid) = state.manual_group_for(session) {
        if let Some(g) = state.manual_groups.iter().find(|g| g.id == gid) {
            parts.push(g.title.as_str());
            parts.push(g.id.as_str());
        }
    }
    parts.join(" ").to_lowercase()
}

/// Filter a session list in place (order preserved).
pub fn filter_sessions(
    sessions: &[ProviderSession],
    state: &UserState,
    query: &str,
) -> Vec<ProviderSession> {
    sessions
        .iter()
        .filter(|s| session_matches(s, state, query, None))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{ManualGroup, SessionMatch, SessionPref, UserState};

    fn sess(id: &str, title: &str, cwd: Option<&str>, agent: AgentClass) -> ProviderSession {
        ProviderSession {
            provider: "kitty".into(),
            id: id.into(),
            title: title.into(),
            cwd: cwd.map(str::to_string),
            is_focused: false,
            os_window_id: None,
            focus_endpoint: None,
            focus_tab_id: None,
            focus_window_id: None,
            focus_key: None,
            agent,
            attention: Attention::Idle,
        }
    }

    #[test]
    fn empty_query_matches_all() {
        let s = sess("a", "foo", Some("/tmp"), AgentClass::Shell);
        let st = UserState::default();
        assert!(session_matches(&s, &st, "", None));
        assert!(session_matches(&s, &st, "   ", None));
    }

    #[test]
    fn matches_title_cwd_agent() {
        let s = sess(
            "1:w1:t2",
            "Claude · Thinking",
            Some("/home/u/trading-brain"),
            AgentClass::Claude,
        );
        let st = UserState::default();
        assert!(session_matches(&s, &st, "thinking", None));
        assert!(session_matches(&s, &st, "trading", None));
        assert!(session_matches(&s, &st, "claude", None));
        assert!(!session_matches(&s, &st, "codex", None));
    }

    #[test]
    fn multi_token_and() {
        let s = sess(
            "x",
            "Sleep then say hi",
            Some("/tmp/proj"),
            AgentClass::Claude,
        );
        let st = UserState::default();
        assert!(session_matches(&s, &st, "sleep claude", None));
        assert!(!session_matches(&s, &st, "sleep codex", None));
    }

    #[test]
    fn matches_group_title() {
        let s = sess("t1", "zsh", Some("/work"), AgentClass::Shell);
        let mut st = UserState::default();
        st.manual_groups.push(ManualGroup {
            id: "g1".into(),
            title: "Trading".into(),
            sort_index: 0,
        });
        st.session_prefs.push(SessionPref {
            provider: "kitty".into(),
            match_rule: SessionMatch::ProviderId { id: "t1".into() },
            manual_group_id: Some("g1".into()),
            priority: Priority::Normal,
            cwd: None,
            title: None,
            updated_at: 0,
        });
        assert!(session_matches(&s, &st, "trading", None));
        assert!(session_matches(&s, &st, "Trading", Some("Trading")));
    }

    #[test]
    fn filter_sessions_preserves_order() {
        let st = UserState::default();
        let all = vec![
            sess("a", "alpha", None, AgentClass::Shell),
            sess("b", "beta claude", None, AgentClass::Claude),
            sess("c", "gamma", None, AgentClass::Grok),
        ];
        let out = filter_sessions(&all, &st, "claude");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "b");
    }
}
