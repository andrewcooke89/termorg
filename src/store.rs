//! Persisted user state: manual groups + session prefs (`~/.config/termorg/`).
//!
//! **Assignment is per tab (provider session id).** Fingerprint (cwd+title) is only
//! used to rebind a *stale* id after restart — never to fan out one assignment to
//! every tab sharing a cwd (important when work is tab-centric on Wayland).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{Result, TermorgError};
use crate::provider::ProviderSession;

const SCHEMA: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualGroup {
    pub id: String,
    pub title: String,
    pub sort_index: i32,
}

/// User priority (FS8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Important,
    #[default]
    Normal,
    Muted,
}

impl Priority {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Important => "important",
            Self::Normal => "normal",
            Self::Muted => "muted",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Important => "★",
            Self::Normal => "",
            Self::Muted => "muted",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "important" | "star" | "high" | "*" | "★" => Some(Self::Important),
            "normal" | "default" | "clear" => Some(Self::Normal),
            "muted" | "mute" | "low" => Some(Self::Muted),
            _ => None,
        }
    }

    pub fn rank(self) -> u8 {
        match self {
            Self::Important => 0,
            Self::Normal => 1,
            Self::Muted => 2,
        }
    }
}

/// How we remember a session. Live membership always uses ProviderId.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionMatch {
    ProviderId {
        id: String,
    },
    /// Legacy: old builds stored standalone fingerprints that applied to every
    /// matching tab. Kept for deserialize; ignored for live membership.
    #[serde(other)]
    LegacyOther,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPref {
    pub provider: String,
    #[serde(rename = "match")]
    pub match_rule: SessionMatch,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual_group_id: Option<String>,
    /// User priority (FS8). Default normal.
    #[serde(default)]
    pub priority: Priority,
    /// True when the user explicitly set priority (including Normal).
    /// Keeps a Normal-only pref so it can shadow sticky path mute across reloads.
    #[serde(default)]
    pub explicit_priority: bool,
    /// For rebind after restart when the tab id changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub updated_at: u64,
}

impl SessionPref {
    /// Pref rows worth keeping on disk / after rebind.
    pub(crate) fn is_meaningful(&self) -> bool {
        self.manual_group_id.is_some()
            || self.priority != Priority::Normal
            || self.explicit_priority
    }
}

/// Learned path → manual group (FS15).
///
/// When `sticky` is true, the group applies automatically to unassigned
/// sessions under that path (no Accept click required). Assigning a tab
/// always marks the path sticky.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathHint {
    /// Git root abs path or `path:…` key from path_group.
    pub path_key: String,
    pub group_id: String,
    #[serde(default)]
    pub hits: u32,
    pub updated_at: u64,
    /// Auto-apply group to matching paths (sticky rule).
    #[serde(default)]
    pub sticky: bool,
    /// Auto-mute sessions under this path when no per-tab priority is set.
    #[serde(default)]
    pub mute: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserState {
    pub schema_version: u32,
    #[serde(default)]
    pub manual_groups: Vec<ManualGroup>,
    #[serde(default)]
    pub session_prefs: Vec<SessionPref>,
    /// FS15: path_key → preferred manual group (from past assigns).
    #[serde(default)]
    pub path_hints: Vec<PathHint>,
    /// FS15: path keys the user dismissed (no more suggestions until re-learned).
    #[serde(default)]
    pub dismissed_path_hints: Vec<String>,
}

impl Default for UserState {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA,
            manual_groups: Vec::new(),
            session_prefs: Vec::new(),
            path_hints: Vec::new(),
            dismissed_path_hints: Vec::new(),
        }
    }
}

impl UserState {
    pub fn load() -> Result<Self> {
        let path = state_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(&path).map_err(TermorgError::Io)?;
        let mut state: UserState = serde_json::from_str(&raw).map_err(|e| TermorgError::Parse {
            message: format!("state file {}: {e}", path.display()),
        })?;
        if state.schema_version == 0 {
            state.schema_version = SCHEMA;
        }
        state.retain_persisted_prefs();
        Ok(state)
    }

    /// Drop prefs that should not survive disk reload / rebind cleanup.
    pub(crate) fn retain_persisted_prefs(&mut self) {
        self.session_prefs.retain(|p| {
            matches!(p.match_rule, SessionMatch::ProviderId { .. }) && p.is_meaningful()
        });
    }

    pub fn save(&self) -> Result<()> {
        let path = state_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(TermorgError::Io)?;
        }
        let raw = serde_json::to_string_pretty(self).map_err(|e| TermorgError::Parse {
            message: format!("serialize state: {e}"),
        })?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, raw).map_err(TermorgError::Io)?;
        fs::rename(&tmp, &path).map_err(TermorgError::Io)?;
        Ok(())
    }

    pub fn create_group(&mut self, title: &str) -> ManualGroup {
        let title = title.trim();
        let id = new_id("g");
        let sort_index = self
            .manual_groups
            .iter()
            .map(|g| g.sort_index)
            .max()
            .unwrap_or(-1)
            + 1;
        let g = ManualGroup {
            id: id.clone(),
            title: if title.is_empty() {
                "Group".into()
            } else {
                title.into()
            },
            sort_index,
        };
        self.manual_groups.push(g.clone());
        g
    }

    pub fn rename_group(&mut self, id_or_title: &str, new_title: &str) -> Result<()> {
        let g = self
            .find_group_mut(id_or_title)
            .ok_or_else(|| TermorgError::ProviderCommand {
                message: format!("no manual group matching `{id_or_title}`"),
            })?;
        let t = new_title.trim();
        if !t.is_empty() {
            g.title = t.into();
        }
        Ok(())
    }

    pub fn delete_group(&mut self, id_or_title: &str) -> Result<()> {
        let id = self
            .find_group(id_or_title)
            .map(|g| g.id.clone())
            .ok_or_else(|| TermorgError::ProviderCommand {
                message: format!("no manual group matching `{id_or_title}`"),
            })?;
        self.manual_groups.retain(|g| g.id != id);
        self.session_prefs
            .retain(|p| p.manual_group_id.as_deref() != Some(id.as_str()));
        self.path_hints.retain(|h| h.group_id != id);
        Ok(())
    }

    pub fn find_group(&self, id_or_title: &str) -> Option<&ManualGroup> {
        self.manual_groups
            .iter()
            .find(|g| g.id == id_or_title || g.title.eq_ignore_ascii_case(id_or_title))
    }

    pub fn find_group_mut(&mut self, id_or_title: &str) -> Option<&mut ManualGroup> {
        self.manual_groups
            .iter_mut()
            .find(|g| g.id == id_or_title || g.title.eq_ignore_ascii_case(id_or_title))
    }

    pub fn ordered_groups(&self) -> Vec<&ManualGroup> {
        let mut v: Vec<_> = self.manual_groups.iter().collect();
        v.sort_by_key(|g| (g.sort_index, g.title.as_str()));
        v
    }

    /// Live membership: per-tab pref first, else sticky path→group rule.
    pub fn manual_group_for(&self, session: &ProviderSession) -> Option<String> {
        if let Some(gid) = self
            .pref_for(session)
            .and_then(|p| p.manual_group_id.clone())
        {
            return Some(gid);
        }
        self.sticky_group_for_session(session)
    }

    /// User priority for a live session (default normal).
    /// Per-tab pref wins; else sticky path mute.
    pub fn priority_for(&self, session: &ProviderSession) -> Priority {
        if let Some(p) = self.pref_for(session) {
            return p.priority;
        }
        if self.path_sticky_mute(session) {
            return Priority::Muted;
        }
        Priority::Normal
    }

    /// Sticky path→group for a session cwd (if any).
    pub fn sticky_group_for_session(&self, session: &ProviderSession) -> Option<String> {
        let (path_key, _) = crate::hints::path_key_for_session(session)?;
        if self.is_hint_dismissed(&path_key) {
            return None;
        }
        let hint = self.best_sticky_hint_for_path(&path_key)?;
        // Group must still exist.
        if self.manual_groups.iter().any(|g| g.id == hint.group_id) {
            Some(hint.group_id.clone())
        } else {
            None
        }
    }

    pub fn sticky_mute_for_session(&self, session: &ProviderSession) -> bool {
        // Only when no per-tab pref shadows it.
        if self.pref_for(session).is_some() {
            return false;
        }
        self.path_sticky_mute(session)
    }

    fn best_sticky_hint_for_path(&self, path_key: &str) -> Option<&PathHint> {
        self.path_hints
            .iter()
            .filter(|h| h.path_key == path_key && h.sticky && !h.group_id.is_empty())
            .max_by_key(|h| (h.hits, h.updated_at))
    }

    fn pref_for(&self, session: &ProviderSession) -> Option<&SessionPref> {
        self.session_prefs.iter().find(|p| {
            p.provider == session.provider
                && matches!(&p.match_rule, SessionMatch::ProviderId { id } if id == &session.id)
        })
    }

    pub fn assign(&mut self, session: &ProviderSession, group_id_or_title: &str) -> Result<()> {
        let gid = self
            .find_group(group_id_or_title)
            .map(|g| g.id.clone())
            .ok_or_else(|| TermorgError::ProviderCommand {
                message: format!("no manual group matching `{group_id_or_title}`"),
            })?;
        self.touch_pref(session, |p| {
            p.manual_group_id = Some(gid.clone());
        });
        // FS15 + sticky: learn path → group and auto-apply for future tabs.
        if let Some((path_key, _)) = crate::hints::path_key_for_session(session) {
            self.record_path_hint(&path_key, &gid);
            self.set_path_sticky(&path_key, true);
            self.dismissed_path_hints.retain(|k| k != &path_key);
        }
        Ok(())
    }

    /// Record or strengthen a path→group association.
    pub fn record_path_hint(&mut self, path_key: &str, group_id: &str) {
        let now = now_secs();
        if let Some(h) = self
            .path_hints
            .iter_mut()
            .find(|h| h.path_key == path_key && h.group_id == group_id)
        {
            h.hits = h.hits.saturating_add(1);
            h.updated_at = now;
            return;
        }
        // Same path, different group → switch association (user re-assigned).
        if let Some(h) = self.path_hints.iter_mut().find(|h| h.path_key == path_key) {
            h.group_id = group_id.into();
            h.hits = h.hits.saturating_add(1).max(1);
            h.updated_at = now;
            return;
        }
        self.path_hints.push(PathHint {
            path_key: path_key.into(),
            group_id: group_id.into(),
            hits: 1,
            updated_at: now,
            sticky: false,
            mute: false,
        });
    }

    /// Mark path rule sticky (auto-assign group on matching sessions).
    pub fn set_path_sticky(&mut self, path_key: &str, sticky: bool) {
        let mut found = false;
        for h in &mut self.path_hints {
            if h.path_key == path_key {
                h.sticky = sticky;
                h.updated_at = now_secs();
                found = true;
            }
        }
        if !found && sticky {
            // Sticky without group is useless; callers should record_path_hint first.
        }
        let _ = found;
    }

    /// Set or clear mute for a sticky path rule (creates hint row if needed with empty group).
    pub fn set_path_mute(&mut self, path_key: &str, mute: bool) -> Result<()> {
        if let Some(h) = self.path_hints.iter_mut().find(|h| h.path_key == path_key) {
            h.mute = mute;
            h.sticky = true;
            h.updated_at = now_secs();
            return Ok(());
        }
        if !mute {
            return Ok(());
        }
        // Mute-only sticky rule (no group).
        self.path_hints.push(PathHint {
            path_key: path_key.into(),
            group_id: String::new(),
            hits: 1,
            updated_at: now_secs(),
            sticky: true,
            mute: true,
        });
        Ok(())
    }

    pub fn best_hint_for_path(&self, path_key: &str) -> Option<&PathHint> {
        self.path_hints
            .iter()
            .filter(|h| h.path_key == path_key)
            .max_by_key(|h| (h.hits, h.updated_at))
    }

    pub fn is_hint_dismissed(&self, path_key: &str) -> bool {
        self.dismissed_path_hints.iter().any(|k| k == path_key)
    }

    pub fn dismiss_path_hint(&mut self, path_key: &str) {
        if !self.is_hint_dismissed(path_key) {
            self.dismissed_path_hints.push(path_key.into());
        }
    }

    pub fn clear_dismissed_hint(&mut self, path_key: &str) {
        self.dismissed_path_hints.retain(|k| k != path_key);
    }

    /// Drop hints pointing at deleted groups (keep mute-only empty group_id).
    pub fn prune_stale_hints(&mut self) {
        let gids: HashSet<&str> = self.manual_groups.iter().map(|g| g.id.as_str()).collect();
        self.path_hints
            .retain(|h| h.group_id.is_empty() || gids.contains(h.group_id.as_str()));
    }

    pub fn unassign(&mut self, session: &ProviderSession) {
        // Clear group only; keep priority if non-normal.
        let mut drop = false;
        if let Some(p) = self.pref_for_mut(session) {
            p.manual_group_id = None;
            p.updated_at = now_secs();
            if !p.is_meaningful() {
                drop = true;
            }
        }
        if drop {
            self.remove_pref(session);
        }
    }

    pub fn set_priority(&mut self, session: &ProviderSession, priority: Priority) {
        // Always write a per-tab pref so Normal can override sticky path mute.
        // `explicit_priority` keeps Normal-only rows through load/rebind.
        self.touch_pref(session, |p| {
            p.priority = priority;
            p.explicit_priority = true;
        });
    }

    fn path_sticky_mute(&self, session: &ProviderSession) -> bool {
        let Some((path_key, _)) = crate::hints::path_key_for_session(session) else {
            return false;
        };
        if self.is_hint_dismissed(&path_key) {
            return false;
        }
        self.path_hints
            .iter()
            .any(|h| h.path_key == path_key && h.sticky && h.mute)
    }

    fn pref_for_mut(&mut self, session: &ProviderSession) -> Option<&mut SessionPref> {
        let provider = session.provider.clone();
        let sid = session.id.clone();
        self.session_prefs.iter_mut().find(|p| {
            p.provider == provider
                && matches!(&p.match_rule, SessionMatch::ProviderId { id } if id == &sid)
        })
    }

    fn remove_pref(&mut self, session: &ProviderSession) {
        self.session_prefs.retain(|p| {
            !(p.provider == session.provider
                && matches!(&p.match_rule, SessionMatch::ProviderId { id } if id == &session.id))
        });
    }

    /// Ensure a provider-id pref exists, then mutate it.
    fn touch_pref<F>(&mut self, session: &ProviderSession, f: F)
    where
        F: FnOnce(&mut SessionPref),
    {
        let now = now_secs();
        if self.pref_for(session).is_none() {
            self.session_prefs.push(SessionPref {
                provider: session.provider.clone(),
                match_rule: SessionMatch::ProviderId {
                    id: session.id.clone(),
                },
                manual_group_id: None,
                priority: Priority::Normal,
                explicit_priority: false,
                cwd: session.cwd.clone(),
                title: Some(session.title.clone()),
                updated_at: now,
            });
        }
        if let Some(p) = self.pref_for_mut(session) {
            p.cwd = session.cwd.clone();
            p.title = Some(session.title.clone());
            p.updated_at = now;
            f(p);
        }
        // Drop empty prefs (Normal without group and without explicit_priority).
        if let Some(p) = self.pref_for(session) {
            if !p.is_meaningful() {
                self.remove_pref(session);
            }
        }
    }

    /// If a stored session id is gone, rebind **at most one** live tab using
    /// cwd+title from that pref. Never applies one pref to every tab with that cwd.
    /// Returns true if state changed (caller should save).
    pub fn rebind_stale_session_ids(&mut self, live: &[ProviderSession]) -> bool {
        let live_ids: HashSet<(&str, &str)> = live
            .iter()
            .map(|s| (s.provider.as_str(), s.id.as_str()))
            .collect();

        // Sessions already claimed by a live provider_id pref (group or priority).
        let mut claimed: HashSet<String> = HashSet::new();
        for s in live {
            if self.pref_for(s).is_some() {
                claimed.insert(s.id.clone());
            }
        }

        let mut changed = false;
        let now = now_secs();

        for pref in &mut self.session_prefs {
            let SessionMatch::ProviderId { id } = &pref.match_rule else {
                continue;
            };
            if live_ids.contains(&(pref.provider.as_str(), id.as_str())) {
                continue; // still live
            }
            if !pref.is_meaningful() {
                continue;
            }
            // Group must still exist if set.
            if let Some(gid) = &pref.manual_group_id {
                if !self.manual_groups.iter().any(|g| &g.id == gid) {
                    pref.manual_group_id = None;
                    changed = true;
                    if !pref.is_meaningful() {
                        continue;
                    }
                }
            }

            let cwd = pref.cwd.as_deref().unwrap_or("");
            let title_n = pref
                .title
                .as_deref()
                .map(normalize_title)
                .unwrap_or_default();
            if cwd.is_empty() {
                continue;
            }

            let candidates: Vec<&ProviderSession> = live
                .iter()
                .filter(|s| {
                    s.provider == pref.provider
                        && !claimed.contains(&s.id)
                        && s.cwd.as_deref() == Some(cwd)
                })
                .collect();

            if candidates.is_empty() {
                continue;
            }

            let exact: Vec<_> = candidates
                .iter()
                .copied()
                .filter(|s| normalize_title(&s.title) == title_n)
                .collect();
            let chosen = if exact.len() == 1 {
                Some(exact[0])
            } else if exact.is_empty() && candidates.len() == 1 {
                Some(candidates[0])
            } else {
                None
            };

            if let Some(s) = chosen {
                pref.match_rule = SessionMatch::ProviderId { id: s.id.clone() };
                pref.title = Some(s.title.clone());
                pref.cwd = s.cwd.clone();
                pref.updated_at = now;
                claimed.insert(s.id.clone());
                changed = true;
            }
        }

        // Drop empty prefs (no group, normal priority).
        let before = self.session_prefs.len();
        self.session_prefs.retain(|p| p.is_meaningful());
        // Clear orphaned group ids.
        for p in &mut self.session_prefs {
            if let Some(gid) = &p.manual_group_id {
                if !self.manual_groups.iter().any(|g| &g.id == gid) {
                    p.manual_group_id = None;
                    changed = true;
                }
            }
        }
        self.session_prefs.retain(|p| p.is_meaningful());
        if self.session_prefs.len() != before {
            changed = true;
        }

        changed
    }
}

pub fn state_path() -> PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("termorg").join("state.json");
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".config/termorg/state.json")
}

fn new_id(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{prefix}-{t:x}-{n:x}")
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn normalize_title(t: &str) -> String {
    t.replace('\n', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Display sections: manual groups first, then auto path groups for unassigned.
#[derive(Debug, Clone)]
pub enum DisplaySection {
    Manual {
        group: ManualGroup,
        sessions: Vec<ProviderSession>,
    },
    Auto {
        title: String,
        path_hint: String,
        sessions: Vec<ProviderSession>,
    },
}

pub fn build_display_sections(
    sessions: Vec<ProviderSession>,
    state: &UserState,
) -> Vec<DisplaySection> {
    use crate::path_group::sessions_by_group;

    let mut assigned: HashMap<String, Vec<ProviderSession>> = HashMap::new();
    let mut unassigned: Vec<ProviderSession> = Vec::new();

    for s in sessions {
        if let Some(gid) = state.manual_group_for(&s) {
            if state.manual_groups.iter().any(|g| g.id == gid) {
                assigned.entry(gid).or_default().push(s);
                continue;
            }
        }
        unassigned.push(s);
    }

    let mut out = Vec::new();

    for g in state.ordered_groups() {
        let mut members = assigned.remove(&g.id).unwrap_or_default();
        sort_sessions_for_display(&mut members, state);
        out.push(DisplaySection::Manual {
            group: g.clone(),
            sessions: members,
        });
    }

    for (pg, mut members) in sessions_by_group(unassigned) {
        sort_sessions_for_display(&mut members, state);
        let path_hint = if pg.id.starts_with("path:") {
            pg.id.strip_prefix("path:").unwrap_or(&pg.id).to_string()
        } else {
            pg.id.clone()
        };
        out.push(DisplaySection::Auto {
            title: pg.title,
            path_hint,
            sessions: members,
        });
    }

    out
}

fn sort_sessions_for_display(sessions: &mut [ProviderSession], state: &UserState) {
    sessions.sort_by(|a, b| {
        let pa = state.priority_for(a);
        let pb = state.priority_for(b);
        pa.rank()
            .cmp(&pb.rank())
            .then_with(|| attention_rank(a.attention).cmp(&attention_rank(b.attention)))
            .then_with(|| a.id.cmp(&b.id))
    });
}

fn attention_rank(a: crate::attention::Attention) -> u8 {
    use crate::attention::Attention;
    match a {
        Attention::NeedsYou => 0,
        Attention::Working => 1,
        Attention::Unknown => 2,
        Attention::Idle => 3,
    }
}

/// Load state, rebind stale ids, save if needed.
pub fn load_and_rebind(live: &[ProviderSession]) -> Result<UserState> {
    let mut state = UserState::load()?;
    let mut dirty = state.rebind_stale_session_ids(live);
    state.prune_stale_hints();
    // Soft-learn from existing assigns if map empty (first run after upgrade).
    if state.path_hints.is_empty()
        && state
            .session_prefs
            .iter()
            .any(|p| p.manual_group_id.is_some())
    {
        crate::hints::rebuild_hints_from_prefs(&mut state, live);
        dirty = true;
    }
    if dirty {
        let _ = state.save();
    }
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentClass;
    use crate::attention::Attention;

    fn sess(id: &str, cwd: &str, title: &str) -> ProviderSession {
        ProviderSession {
            provider: "kitty".into(),
            id: id.into(),
            title: title.into(),
            cwd: Some(cwd.into()),
            is_focused: false,
            os_window_id: Some(1),
            focus_endpoint: None,
            focus_tab_id: None,
            focus_window_id: None,
            focus_key: None,
            agent: AgentClass::Shell,
            attention: crate::attention::Attention::Idle,
        }
    }

    #[test]
    fn assign_sets_sticky_path_so_siblings_inherit_group() {
        let mut state = UserState::default();
        state.create_group("Trading");
        let a = sess("w1:t1", "/tmp/repo-sticky-a", "bash");
        let b = sess("w1:t2", "/tmp/repo-sticky-a", "bash"); // new tab, same cwd
        state.assign(&a, "Trading").unwrap();

        let gid = state.manual_groups[0].id.clone();
        assert_eq!(state.manual_group_for(&a).as_deref(), Some(gid.as_str()));
        // Sticky path rule auto-applies to sibling under same path.
        assert_eq!(state.manual_group_for(&b).as_deref(), Some(gid.as_str()));

        // Unassign clears only per-tab pref; sticky still covers a and b.
        state.unassign(&a);
        assert_eq!(state.manual_group_for(&a).as_deref(), Some(gid.as_str()));
        assert_eq!(state.manual_group_for(&b).as_deref(), Some(gid.as_str()));
    }

    #[test]
    fn sticky_mute_applies_without_per_tab_pref() {
        let mut state = UserState::default();
        let s = sess("w9:t1", "/tmp/mute-path-xyz", "zsh");
        let (key, _) = crate::hints::path_key_for_session(&s).expect("path key");
        state.set_path_mute(&key, true).unwrap();
        assert_eq!(state.priority_for(&s), Priority::Muted);
        // Explicit normal pref wins over sticky mute.
        state.set_priority(&s, Priority::Normal);
        assert_eq!(state.priority_for(&s), Priority::Normal);
    }

    /// Skeptic: Normal override of sticky mute must survive save → load → rebind.
    #[test]
    fn sticky_mute_normal_override_survives_save_load_rebind() {
        use std::sync::Mutex;
        static LOCK: Mutex<()> = Mutex::new(());
        let _g = LOCK.lock().unwrap();

        let dir = std::env::temp_dir().join(format!("termorg-sticky-mute-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        std::env::set_var("TERMORG_CONFIG_DIR", &dir);

        let mut state = UserState::default();
        let s = sess("kitty:w1:t1", "/tmp/sticky-mute-roundtrip", "zsh");
        let (key, _) = crate::hints::path_key_for_session(&s).expect("path key");
        state.set_path_mute(&key, true).unwrap();
        assert_eq!(state.priority_for(&s), Priority::Muted);
        state.set_priority(&s, Priority::Normal);
        assert_eq!(state.priority_for(&s), Priority::Normal);
        // Pref must be marked meaningful via explicit_priority.
        assert!(
            state
                .session_prefs
                .iter()
                .any(|p| p.explicit_priority && p.priority == Priority::Normal),
            "expected explicit Normal pref: {:?}",
            state.session_prefs
        );
        state.save().unwrap();

        // Simulate new process: load drops non-meaningful prefs.
        let loaded = UserState::load().unwrap();
        assert_eq!(
            loaded.priority_for(&s),
            Priority::Normal,
            "after load, sticky mute must stay shadowed"
        );

        // rebind with same live id must not drop the override.
        let mut loaded2 = loaded;
        let _ = loaded2.rebind_stale_session_ids(std::slice::from_ref(&s));
        assert_eq!(
            loaded2.priority_for(&s),
            Priority::Normal,
            "after rebind, sticky mute must stay shadowed"
        );

        // load_and_rebind public path
        let via = load_and_rebind(std::slice::from_ref(&s)).unwrap();
        assert_eq!(via.priority_for(&s), Priority::Normal);

        let _ = fs::remove_dir_all(&dir);
        std::env::remove_var("TERMORG_CONFIG_DIR");
    }

    #[test]
    fn per_tab_pref_outranks_sticky_group() {
        let mut state = UserState::default();
        let g1 = state.create_group("One");
        let g2 = state.create_group("Two");
        let a = sess("w1:t1", "/tmp/pref-outrank", "bash");
        let b = sess("w1:t2", "/tmp/pref-outrank", "bash");
        state.assign(&a, &g1.id).unwrap(); // sticky → One
        state.assign(&b, &g2.id).unwrap(); // b per-tab Two
        assert_eq!(state.manual_group_for(&b).as_deref(), Some(g2.id.as_str()));
        // a still One (pref or sticky)
        assert_eq!(state.manual_group_for(&a).as_deref(), Some(g1.id.as_str()));
        let _ = Attention::Idle;
    }

    #[test]
    fn unassign_one_tab_leaves_other_pref_sticky_covers_both() {
        let mut state = UserState::default();
        state.create_group("Trading");
        let a = sess("w1:t1", "/tmp/repo-unassign", "bash");
        let b = sess("w1:t2", "/tmp/repo-unassign", "bash");
        state.assign(&a, "Trading").unwrap();
        state.assign(&b, "Trading").unwrap();
        state.unassign(&a);
        // a loses per-tab pref but sticky path still assigns.
        assert!(state.manual_group_for(&a).is_some());
        assert!(state.manual_group_for(&b).is_some());
    }

    #[test]
    fn rebind_only_when_unique() {
        let mut state = UserState::default();
        let g = state.create_group("Trading");
        let old = sess("old:w1:t1", "/tmp/only-rebind", "shell-a");
        state.assign(&old, "Trading").unwrap();

        // Old id gone; one live tab at that cwd → rebind and/or sticky.
        let neu = sess("new:w1:t9", "/tmp/only-rebind", "shell-a");
        let _ = state.rebind_stale_session_ids(std::slice::from_ref(&neu));
        assert_eq!(state.manual_group_for(&neu).as_deref(), Some(g.id.as_str()));

        // Two live tabs at same cwd → sticky still groups both; rebind of stale id is ambiguous.
        let mut state2 = UserState::default();
        let g2 = state2.create_group("Trading");
        let old2 = sess("gone:w1:t1", "/tmp/multi-rebind", "bash");
        state2.assign(&old2, "Trading").unwrap();
        let t1 = sess("a:w1:t1", "/tmp/multi-rebind", "bash");
        let t2 = sess("a:w1:t2", "/tmp/multi-rebind", "bash");
        let _changed = state2.rebind_stale_session_ids(&[t1.clone(), t2.clone()]);
        // Sticky path rule applies to both tabs under the path.
        assert_eq!(
            state2.manual_group_for(&t1).as_deref(),
            Some(g2.id.as_str())
        );
        assert_eq!(
            state2.manual_group_for(&t2).as_deref(),
            Some(g2.id.as_str())
        );
    }

    #[test]
    fn display_manual_first() {
        let mut state = UserState::default();
        state.create_group("Trading");
        let a = sess("a:w1:t1", "/tmp/alpha-proj", "x");
        let b = sess("b:w1:t1", "/tmp/beta-proj", "y");
        state.assign(&a, "Trading").unwrap();
        let sections = build_display_sections(vec![a, b], &state);
        assert!(matches!(sections[0], DisplaySection::Manual { .. }));
        let DisplaySection::Manual { sessions, .. } = &sections[0] else {
            panic!();
        };
        assert_eq!(sessions.len(), 1);
    }

    #[test]
    fn priority_persists_and_sorts() {
        let mut state = UserState::default();
        let a = sess("a:w1:t1", "/tmp/a", "a");
        let b = sess("b:w1:t1", "/tmp/b", "b");
        let c = sess("c:w1:t1", "/tmp/c", "c");
        state.set_priority(&a, Priority::Muted);
        state.set_priority(&c, Priority::Important);
        assert_eq!(state.priority_for(&a), Priority::Muted);
        assert_eq!(state.priority_for(&b), Priority::Normal);
        assert_eq!(state.priority_for(&c), Priority::Important);

        let sections = build_display_sections(vec![a.clone(), b.clone(), c.clone()], &state);
        // All auto path groups; within each group only one session — check global order via sort helper
        let mut all = vec![a, b, c];
        sort_sessions_for_display(&mut all, &state);
        assert_eq!(all[0].id, "c:w1:t1"); // important first
        assert_eq!(all[2].id, "a:w1:t1"); // muted last
        let _ = sections;
    }
}
