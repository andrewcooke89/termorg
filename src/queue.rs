//! FS9 — action queue (computed, not stored).
//!
//! Inclusion (D22):
//!   (priority != muted && attention == needs_you)
//!   || (priority == important && attention in {needs_you, working, unknown})
//!
//! Order (D23): priority rank, attention rank, name.

use crate::attention::Attention;
use crate::provider::ProviderSession;
use crate::store::{Priority, UserState};

/// Build the ordered action queue for live sessions.
pub fn build_action_queue(sessions: &[ProviderSession], state: &UserState) -> Vec<ProviderSession> {
    let mut q: Vec<ProviderSession> = sessions
        .iter()
        .filter(|s| include_in_queue(s, state))
        .cloned()
        .collect();

    q.sort_by(|a, b| {
        let pa = state.priority_for(a);
        let pb = state.priority_for(b);
        pa.rank()
            .cmp(&pb.rank())
            .then_with(|| attention_rank(a.attention).cmp(&attention_rank(b.attention)))
            .then_with(|| a.id.cmp(&b.id))
    });
    q
}

fn include_in_queue(s: &ProviderSession, state: &UserState) -> bool {
    let p = state.priority_for(s);
    // Mute never enters the queue.
    if p == Priority::Muted {
        return false;
    }
    // Primary: needs human attention.
    if s.attention == Attention::NeedsYou {
        return true;
    }
    // Important + actively not-idle stays visible in the queue (D22).
    // Normal + working does NOT stay in the queue.
    if p == Priority::Important {
        return matches!(s.attention, Attention::Working | Attention::Unknown);
    }
    false
}

fn attention_rank(a: Attention) -> u8 {
    match a {
        Attention::NeedsYou => 0,
        Attention::Working => 1,
        Attention::Unknown => 2,
        Attention::Idle => 3,
    }
}

/// Index of `id` in queue, or None.
pub fn queue_index(queue: &[ProviderSession], id: &str) -> Option<usize> {
    queue.iter().position(|s| s.id == id)
}

/// Next session after `current_id` (wraps). If current not in queue, returns first.
pub fn queue_next<'a>(
    queue: &'a [ProviderSession],
    current_id: Option<&str>,
) -> Option<&'a ProviderSession> {
    if queue.is_empty() {
        return None;
    }
    match current_id.and_then(|id| queue_index(queue, id)) {
        Some(i) => Some(&queue[(i + 1) % queue.len()]),
        None => Some(&queue[0]),
    }
}

/// Previous session before `current_id` (wraps).
pub fn queue_prev<'a>(
    queue: &'a [ProviderSession],
    current_id: Option<&str>,
) -> Option<&'a ProviderSession> {
    if queue.is_empty() {
        return None;
    }
    match current_id.and_then(|id| queue_index(queue, id)) {
        Some(i) => {
            let j = if i == 0 { queue.len() - 1 } else { i - 1 };
            Some(&queue[j])
        }
        None => Some(&queue[0]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentClass;
    use crate::attention::Attention;

    fn sess(id: &str, attention: Attention) -> ProviderSession {
        ProviderSession {
            provider: "kitty".into(),
            id: id.into(),
            title: id.into(),
            cwd: Some("/tmp".into()),
            is_focused: false,
            os_window_id: Some(1),
            focus_endpoint: None,
            focus_tab_id: None,
            focus_window_id: None,
            focus_key: None,
            agent: AgentClass::Shell,
            attention,
        }
    }

    #[test]
    fn queue_rules() {
        let mut state = UserState::default();
        let needs = sess("n", Attention::NeedsYou);
        let idle_imp = sess("i", Attention::Idle);
        let work_imp = sess("w", Attention::Working);
        let muted_needs = sess("m", Attention::NeedsYou);
        let normal_work = sess("x", Attention::Working);

        state.set_priority(&idle_imp, Priority::Important);
        state.set_priority(&work_imp, Priority::Important);
        state.set_priority(&muted_needs, Priority::Muted);

        let all = vec![
            needs.clone(),
            idle_imp.clone(),
            work_imp.clone(),
            muted_needs.clone(),
            normal_work.clone(),
        ];
        let q = build_action_queue(&all, &state);
        let ids: Vec<_> = q.iter().map(|s| s.id.as_str()).collect();
        // important working first (priority), then needs_you
        assert!(ids.contains(&"n"));
        assert!(ids.contains(&"w"));
        assert!(!ids.contains(&"i")); // important idle out
        assert!(!ids.contains(&"m")); // muted out
        assert!(!ids.contains(&"x")); // normal working out
                                      // order: important (w) before normal (n)
        assert!(ids.iter().position(|x| *x == "w") < ids.iter().position(|x| *x == "n"));
    }

    #[test]
    fn next_prev_wrap() {
        let state = UserState::default();
        let a = sess("a", Attention::NeedsYou);
        let b = sess("b", Attention::NeedsYou);
        let q = build_action_queue(&[a, b], &state);
        assert_eq!(queue_next(&q, Some("a")).unwrap().id, "b");
        assert_eq!(queue_next(&q, Some("b")).unwrap().id, "a");
        assert_eq!(queue_prev(&q, Some("a")).unwrap().id, "b");
    }
}
