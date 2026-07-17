//! Parse `kitten @ ls` JSON into provider sessions.

use std::path::PathBuf;

use serde::Deserialize;

use crate::agent;
use crate::attention;
use crate::error::{Result, TermorgError};
use crate::provider::ProviderSession;

#[derive(Debug, Deserialize)]
struct KittyOsWindow {
    id: u32,
    #[serde(default)]
    tabs: Vec<KittyTab>,
}

#[derive(Debug, Deserialize)]
struct KittyTab {
    id: u32,
    #[serde(default)]
    title: String,
    #[serde(default)]
    is_focused: bool,
    #[serde(default)]
    windows: Vec<KittyWindow>,
}

#[derive(Debug, Deserialize)]
struct KittyWindow {
    #[serde(default)]
    id: u32,
    #[serde(default)]
    title: String,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    pid: Option<i32>,
    /// Kitty shell integration: true when shell is sitting at a prompt.
    #[serde(default)]
    at_prompt: Option<bool>,
    #[serde(default)]
    foreground_processes: Vec<KittyForeground>,
}

#[derive(Debug, Deserialize)]
struct KittyForeground {
    #[serde(default)]
    pid: Option<i32>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    cmdline: Vec<String>,
}

pub(super) fn parse_kitty_ls(
    raw: &str,
    instance: &str,
    endpoint: &str,
) -> Result<Vec<ProviderSession>> {
    let windows: Vec<KittyOsWindow> =
        serde_json::from_str(raw).map_err(|e| TermorgError::Parse {
            message: format!("kitty ls JSON: {e}"),
        })?;

    let mut sessions = Vec::new();
    for osw in windows {
        for tab in osw.tabs {
            let title = tab_title(&tab);
            let cwd = resolve_tab_cwd(&tab);
            let focus_window_id = tab.windows.first().map(|w| w.id);
            let (pids, cmdlines) = collect_proc_hints(&tab);
            let agent = agent::classify_session(&pids, &cmdlines, &title);
            let at_prompt = tab_at_prompt(&tab);
            let id = format!("{instance}:w{}:t{}", osw.id, tab.id);
            let focus_key = Some(format!("{instance}:t{}:w{}", tab.id, focus_window_id.unwrap_or(0)));
            let hint = crate::signals::MatchHint {
                cwd: cwd.as_deref(),
                kitty_pid: Some(instance),
                kitty_window_id: focus_window_id,
                tmux_pane: None,
            };
            let attention = attention::classify(&id, agent, at_prompt, &title, &pids, hint);
            sessions.push(ProviderSession {
                provider: "kitty".into(),
                // Instance tag prevents id collisions across OS windows / processes.
                id,
                title,
                cwd,
                os_window_id: Some(osw.id),
                is_focused: tab.is_focused,
                focus_endpoint: Some(endpoint.to_string()),
                focus_tab_id: Some(tab.id),
                focus_window_id,
                focus_key,
                agent,
                attention,
            });
        }
    }
    Ok(sessions)
}

fn tab_at_prompt(tab: &KittyTab) -> Option<bool> {
    // Prefer any window that reports true (multi-pane: if one is at prompt).
    let mut saw_false = false;
    let mut saw_any = false;
    for w in &tab.windows {
        if let Some(v) = w.at_prompt {
            saw_any = true;
            if v {
                return Some(true);
            }
            saw_false = true;
        }
    }
    if saw_false {
        return Some(false);
    }
    if saw_any {
        return Some(false);
    }
    None
}

fn collect_proc_hints(tab: &KittyTab) -> (Vec<i32>, Vec<String>) {
    let mut pids = Vec::new();
    let mut cmdlines = Vec::new();
    for w in &tab.windows {
        if let Some(pid) = w.pid {
            pids.push(pid);
        }
        for fg in &w.foreground_processes {
            if let Some(pid) = fg.pid {
                pids.push(pid);
            }
            if !fg.cmdline.is_empty() {
                cmdlines.push(fg.cmdline.join(" "));
            }
        }
    }
    pids.sort_unstable();
    pids.dedup();
    (pids, cmdlines)
}

fn resolve_tab_cwd(tab: &KittyTab) -> Option<String> {
    for w in &tab.windows {
        if let Some(cwd) = w.cwd.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
            return Some(cwd.to_string());
        }
        for fg in &w.foreground_processes {
            if let Some(cwd) = fg.cwd.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
                return Some(cwd.to_string());
            }
            if let Some(pid) = fg.pid {
                if let Some(cwd) = cwd_from_proc(pid) {
                    return Some(cwd);
                }
            }
        }
        if let Some(pid) = w.pid {
            if let Some(cwd) = cwd_from_proc(pid) {
                return Some(cwd);
            }
        }
    }
    None
}

fn cwd_from_proc(pid: i32) -> Option<String> {
    if pid <= 0 {
        return None;
    }
    let link = PathBuf::from(format!("/proc/{pid}/cwd"));
    let path = std::fs::read_link(link).ok()?;
    let s = path.to_string_lossy().into_owned();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn tab_title(tab: &KittyTab) -> String {
    if !tab.title.trim().is_empty() {
        return tab.title.trim().to_string();
    }
    tab.windows
        .iter()
        .map(|w| w.title.trim())
        .find(|t| !t.is_empty())
        .unwrap_or("(untitled)")
        .to_string()
}
