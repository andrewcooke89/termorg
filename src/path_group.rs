//! Auto path grouping (FS2): git root preferred, else collapsed path.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use crate::provider::ProviderSession;

/// Derived auto-group for a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathGroup {
    /// Stable key for grouping (absolute git root or path key).
    pub id: String,
    /// Short human title (repo name or collapsed path).
    pub title: String,
    /// True when cwd could not be resolved.
    pub unknown: bool,
}

/// Session plus its auto path group.
#[derive(Debug, Clone)]
pub struct GroupedSession {
    pub session: ProviderSession,
    pub group: PathGroup,
}

/// Resolve auto path group from an absolute-ish cwd.
pub fn path_group_for_cwd(cwd: Option<&str>) -> PathGroup {
    let Some(cwd) = cwd.map(str::trim).filter(|s| !s.is_empty()) else {
        return unknown_group();
    };

    let path = PathBuf::from(cwd);
    if let Some(root) = find_git_root(&path) {
        let title = root
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("repo")
            .to_string();
        return PathGroup {
            id: root.to_string_lossy().into_owned(),
            title,
            unknown: false,
        };
    }

    let collapsed = collapse_path(&path);
    PathGroup {
        id: format!("path:{}", path.to_string_lossy()),
        title: collapsed,
        unknown: false,
    }
}

pub fn group_sessions(sessions: Vec<ProviderSession>) -> Vec<GroupedSession> {
    sessions
        .into_iter()
        .map(|session| {
            let group = path_group_for_cwd(session.cwd.as_deref());
            GroupedSession { session, group }
        })
        .collect()
}

/// Sessions ordered by group title, then session id. Returns (group, members).
pub fn sessions_by_group(sessions: Vec<ProviderSession>) -> Vec<(PathGroup, Vec<ProviderSession>)> {
    let grouped = group_sessions(sessions);
    let mut map: BTreeMap<String, (PathGroup, Vec<ProviderSession>)> = BTreeMap::new();

    for g in grouped {
        let key = g.group.id.clone();
        map.entry(key)
            .and_modify(|(_pg, list)| list.push(g.session.clone()))
            .or_insert_with(|| (g.group.clone(), vec![g.session]));
    }

    let mut out: Vec<(PathGroup, Vec<ProviderSession>)> = map.into_values().collect();
    // Sort groups: Unknown last, else by title then id.
    out.sort_by(|a, b| match (a.0.unknown, b.0.unknown) {
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => a.0.title.cmp(&b.0.title).then_with(|| a.0.id.cmp(&b.0.id)),
    });
    for (_g, sessions) in &mut out {
        sessions.sort_by(|a, b| a.id.cmp(&b.id));
    }
    out
}

fn unknown_group() -> PathGroup {
    PathGroup {
        id: "unknown".into(),
        title: "Unknown".into(),
        unknown: true,
    }
}

/// Walk parents for a `.git` file or directory.
fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut cur = if start.is_absolute() {
        start.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(start)
    };

    // If path is a file, start from parent (unlikely for cwd).
    if cur.is_file() {
        cur = cur.parent()?.to_path_buf();
    }

    loop {
        let git = cur.join(".git");
        if git.exists() {
            return Some(cur);
        }
        if !cur.pop() {
            break;
        }
    }
    None
}

/// Collapse path for display: `~/…` and keep last two meaningful segments when deep.
fn collapse_path(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let display = if let Some(ref home) = home {
        if let Ok(stripped) = path.strip_prefix(home) {
            if stripped.as_os_str().is_empty() {
                return "~".into();
            }
            PathBuf::from("~").join(stripped)
        } else {
            path.to_path_buf()
        }
    } else {
        path.to_path_buf()
    };

    let components: Vec<&str> = display
        .components()
        .filter_map(|c| match c {
            Component::RootDir => Some("/"),
            Component::Normal(s) => s.to_str(),
            Component::Prefix(p) => p.as_os_str().to_str(),
            Component::CurDir | Component::ParentDir => None,
        })
        .collect();

    if components.is_empty() {
        return display.to_string_lossy().into_owned();
    }

    // Keep full path if short; else ~/…/parent/leaf or /…/parent/leaf
    if components.len() <= 3 {
        return join_display(&components);
    }

    let leaf = components[components.len() - 1];
    let parent = components[components.len() - 2];
    if components[0] == "~" {
        format!("~/…/{parent}/{leaf}")
    } else if components[0] == "/" {
        format!("/…/{parent}/{leaf}")
    } else {
        format!("…/{parent}/{leaf}")
    }
}

fn join_display(parts: &[&str]) -> String {
    if parts.is_empty() {
        return String::new();
    }
    if parts[0] == "/" {
        format!("/{}", parts[1..].join("/"))
    } else {
        parts.join("/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn unknown_when_no_cwd() {
        let g = path_group_for_cwd(None);
        assert!(g.unknown);
        assert_eq!(g.title, "Unknown");
    }

    #[test]
    fn git_root_groups_by_repo_name() {
        let dir = tempfile_git_repo("alpha-proj");
        let nested = dir.join("src");
        fs::create_dir_all(&nested).unwrap();
        let g = path_group_for_cwd(Some(nested.to_str().unwrap()));
        assert!(!g.unknown);
        let expected_title = dir.file_name().unwrap().to_str().unwrap();
        assert_eq!(g.title, expected_title);
        assert_eq!(g.id, dir.to_string_lossy());
    }

    #[test]
    fn same_repo_same_group_id() {
        let dir = tempfile_git_repo("shared");
        let a = dir.join("a");
        let b = dir.join("b");
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        let ga = path_group_for_cwd(Some(a.to_str().unwrap()));
        let gb = path_group_for_cwd(Some(b.to_str().unwrap()));
        assert_eq!(ga.id, gb.id);
        assert_eq!(ga.title, gb.title);
    }

    #[test]
    fn non_git_uses_collapsed_path() {
        let dir = std::env::temp_dir().join(format!("termorg-nongit-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let g = path_group_for_cwd(Some(dir.to_str().unwrap()));
        assert!(!g.unknown);
        assert!(!g.id.starts_with("unknown"));
        // title is some path-like string
        assert!(!g.title.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_repos_two_groups() {
        let a = tempfile_git_repo("repo-a");
        let b = tempfile_git_repo("repo-b");
        let sessions = vec![
            sess("1", a.to_str().unwrap()),
            sess("2", b.to_str().unwrap()),
            sess("3", a.join("sub").to_str().unwrap()),
        ];
        // ensure sub exists
        fs::create_dir_all(a.join("sub")).unwrap();
        let groups = sessions_by_group(sessions);
        assert_eq!(groups.len(), 2);
        let title_a = a.file_name().unwrap().to_string_lossy().into_owned();
        let title_b = b.file_name().unwrap().to_string_lossy().into_owned();
        let titles: Vec<_> = groups.iter().map(|(g, _)| g.title.clone()).collect();
        assert!(titles.contains(&title_a));
        assert!(titles.contains(&title_b));
        let a_count = groups
            .iter()
            .find(|(g, _)| g.title == title_a)
            .map(|(_, s)| s.len())
            .unwrap();
        assert_eq!(a_count, 2);
    }

    fn sess(id: &str, cwd: &str) -> ProviderSession {
        ProviderSession {
            provider: "kitty".into(),
            id: id.into(),
            title: id.into(),
            cwd: Some(cwd.into()),
            os_window_id: Some(1),
            is_focused: false,
            focus_endpoint: None,
            focus_tab_id: None,
            focus_window_id: None,
            agent: crate::agent::AgentClass::Shell,
            attention: crate::attention::Attention::Idle,
        }
    }

    fn tempfile_git_repo(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("termorg-git-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir(dir.join(".git")).unwrap();
        dir
    }
}
