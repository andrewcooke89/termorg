//! Kitty default provider — talks to Kitty via remote control (`kitten @ ls`).
//!
//! **Multi-instance:** each Kitty OS window is a separate process. A single
//! fixed `listen_on` path only covers one of them. We discover *all* control
//! sockets under `~/.cache/kitty/` and merge their tabs.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

use super::{
    Capabilities, LaunchRequest, LaunchResult, ProviderSession, TerminalProvider,
};
use crate::agent::{self, AgentClass};
use crate::attention::{self, Attention};
use crate::error::{Result, TermorgError};

const PROVIDER_ID: &str = "kitty";

pub struct KittyProvider {
    /// If set, only this endpoint is queried (testing / override).
    listen_on: Option<String>,
}

impl KittyProvider {
    pub fn new() -> Self {
        Self { listen_on: None }
    }

    pub fn with_listen_on(listen_on: impl Into<String>) -> Self {
        Self {
            listen_on: Some(listen_on.into()),
        }
    }

    /// All remote-control endpoints to query.
    fn endpoints(&self) -> Vec<String> {
        if let Some(ref explicit) = self.listen_on {
            return vec![explicit.clone()];
        }
        let mut out = Vec::new();
        for key in ["KITTY_LISTEN_ON", "TERMORG_KITTY_LISTEN_ON"] {
            if let Ok(env) = std::env::var(key) {
                if !env.is_empty() && !out.contains(&env) {
                    out.push(env);
                }
            }
        }
        for path in discover_all_control_sockets() {
            let ep = format!("unix:{}", path.display());
            if !out.contains(&ep) {
                out.push(ep);
            }
        }
        out
    }

    fn run_ls_on(&self, listen: &str) -> std::result::Result<String, String> {
        let mut last_err = String::new();
        for bin in ["kitten", "kitty"] {
            // `timeout` keeps the UI/refresh thread from hanging on a bad socket.
            let mut cmd = Command::new("timeout");
            cmd.arg("2")
                .arg(bin)
                .arg("@")
                .arg("--to")
                .arg(listen)
                .arg("ls");
            cmd.stdin(Stdio::null());

            match cmd.output() {
                Ok(out) if out.status.success() => {
                    return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
                }
                Ok(out) => {
                    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                    let code = out.status.code().unwrap_or(-1);
                    last_err = if code == 124 {
                        format!("{bin}: timed out")
                    } else if !stderr.is_empty() {
                        format!("{bin}: {stderr}")
                    } else {
                        format!("{bin}: exit {code}")
                    };
                }
                Err(e) => last_err = format!("{bin}: {e}"),
            }
        }
        Err(last_err)
    }
}

impl Default for KittyProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalProvider for KittyProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            list: true,
            focus: true,
            launch: true,
        }
    }

    fn list_sessions(&self) -> Result<Vec<ProviderSession>> {
        let endpoints = self.endpoints();
        if endpoints.is_empty() {
            return Err(TermorgError::ProviderUnavailable {
                provider: PROVIDER_ID.into(),
                message: format!(
                    "No Kitty remote-control sockets found.\n\n\
                     Each Kitty OS window needs its own listen socket. In kitty.conf:\n\
                       allow_remote_control socket-only\n\
                       listen_on unix:${{HOME}}/.cache/kitty/control-{{kitty_pid}}.sock\n\
                     Then **restart every Kitty window** (new config does not attach to old processes).\n\
                     Sockets should appear as ~/.cache/kitty/control-*.sock"
                ),
            });
        }

        let mut sessions = Vec::new();
        let mut errors = Vec::new();
        let mut ok_endpoints = 0usize;

        for ep in &endpoints {
            match self.run_ls_on(ep) {
                Ok(raw) => {
                    let instance = instance_tag(ep);
                    match parse_kitty_ls(&raw, &instance, ep) {
                        Ok(mut batch) => {
                            ok_endpoints += 1;
                            sessions.append(&mut batch);
                        }
                        Err(e) => errors.push(format!("{ep}: {e}")),
                    }
                }
                Err(e) => errors.push(format!("{ep}: {e}")),
            }
        }

        if sessions.is_empty() && ok_endpoints == 0 {
            return Err(TermorgError::ProviderUnavailable {
                provider: PROVIDER_ID.into(),
                message: format!(
                    "Kitty remote control did not respond on any socket.\n\
                     Tried {} endpoint(s):\n  - {}\n\n\
                     Restart Kitty windows after enabling listen_on with {{kitty_pid}}.",
                    endpoints.len(),
                    errors.join("\n  - ")
                ),
            });
        }

        // Stable order: by id.
        sessions.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(sessions)
    }

    fn focus(&self, session: &ProviderSession) -> Result<()> {
        let endpoint = session.focus_endpoint.as_deref().ok_or_else(|| {
            TermorgError::ProviderCommand {
                message: format!(
                    "session {} has no focus endpoint (stale list? refresh and try again)",
                    session.id
                ),
            }
        })?;
        let tab_id = session.focus_tab_id.ok_or_else(|| TermorgError::ProviderCommand {
            message: format!("session {} has no tab id for focus", session.id),
        })?;

        // 1) Select the tab inside this Kitty instance.
        //    Never use `action focus_os_window` — remote control rejects it and
        //    Kitty paints "Failed to parse action" into the target tab.
        let tab_match = format!("id:{tab_id}");
        self.run_remote(endpoint, &["focus-tab", "--match", &tab_match])?;

        // 2) Focus a window in that tab (keyboard target).
        if let Some(wid) = session.focus_window_id {
            let win_match = format!("id:{wid}");
            let _ = self.run_remote(endpoint, &["focus-window", "--match", &win_match]);
        }

        // 3) Within one Kitty process that has multiple OS windows (e.g.
        //    Ctrl+Shift+N), switch the active OS window. Safe no-op when only one.
        //    Uses nth_os_window (works); focus_os_window does NOT over RC.
        if let Some(os_id) = session.os_window_id {
            let _ = self.run_remote(
                endpoint,
                &["action", "nth_os_window", &os_id.to_string()],
            );
        }

        // 4) Un-minimize if needed.
        if let Some(wid) = session.focus_window_id {
            let win_match = format!("id:{wid}");
            let _ = self.run_remote(
                endpoint,
                &["resize-os-window", "--action=show", "--match", &win_match],
            );
        }

        // 5) Best-effort compositor raise for *separate* Kitty processes
        //    (GNOME Wayland usually blocks this; X11/wmctrl may work).
        let _ = raise_os_window_best_effort(session, endpoint);

        Ok(())
    }

    fn launch(&self, req: &LaunchRequest) -> Result<LaunchResult> {
        let endpoint = req
            .endpoint
            .clone()
            .or_else(|| self.endpoints().into_iter().next())
            .ok_or_else(|| TermorgError::ProviderUnavailable {
                provider: PROVIDER_ID.into(),
                message: "no Kitty remote-control socket for launch".into(),
            })?;

        let cwd = req.cwd.as_deref().filter(|c| !c.is_empty());
        if let Some(c) = cwd {
            if !std::path::Path::new(c).is_dir() {
                return Err(TermorgError::ProviderCommand {
                    message: format!("cwd is not a directory: {c}"),
                });
            }
        }

        let tab_title = req
            .tab_title
            .clone()
            .unwrap_or_else(|| req.kind.tab_title_hint(cwd));

        // kitten @ launch --type=tab --cwd=… --tab-title=… [-- cmd…]
        let mut args: Vec<String> = vec![
            "launch".into(),
            "--type=tab".into(),
            format!("--tab-title={tab_title}"),
        ];
        if let Some(c) = cwd {
            args.push(format!("--cwd={c}"));
        }
        let cmd = req.kind.command_argv();
        if !cmd.is_empty() {
            args.push("--".into());
            args.extend(cmd);
        }

        let raw = self.run_remote_capture(&endpoint, &args)?;
        let window_id = raw
            .trim()
            .lines()
            .rev()
            .find_map(|l| l.trim().parse::<u32>().ok());

        Ok(LaunchResult {
            window_id,
            endpoint,
            cwd: cwd.map(|s| s.to_string()),
            kind: req.kind,
        })
    }
}

impl KittyProvider {
    fn run_remote(&self, listen: &str, args: &[&str]) -> Result<()> {
        let mut last_err = String::new();
        for bin in ["kitten", "kitty"] {
            let mut cmd = Command::new("timeout");
            cmd.arg("2").arg(bin).arg("@").arg("--to").arg(listen);
            for a in args {
                cmd.arg(a);
            }
            cmd.stdin(Stdio::null());
            match cmd.output() {
                Ok(out) if out.status.success() => return Ok(()),
                Ok(out) => {
                    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                    last_err = if !stderr.is_empty() {
                        format!("{bin}: {stderr}")
                    } else {
                        format!("{bin}: exit {}", out.status.code().unwrap_or(-1))
                    };
                }
                Err(e) => last_err = format!("{bin}: {e}"),
            }
        }
        Err(TermorgError::ProviderCommand {
            message: format!(
                "remote control failed for {listen} ({args:?}): {last_err}",
                args = args
            ),
        })
    }

    /// FS12: set tab bar colors (`kitten @ set-tab-color`).
    pub fn set_tab_color(&self, session: &ProviderSession, color_args: &[String]) -> Result<()> {
        let endpoint = session.focus_endpoint.as_deref().ok_or_else(|| {
            TermorgError::ProviderCommand {
                message: "no focus endpoint for set-tab-color".into(),
            }
        })?;
        let tab_id = session.focus_tab_id.ok_or_else(|| TermorgError::ProviderCommand {
            message: "no tab id for set-tab-color".into(),
        })?;
        let tab_match = format!("id:{tab_id}");
        let mut args: Vec<&str> = vec!["set-tab-color", "--match", &tab_match];
        let owned: Vec<&str> = color_args.iter().map(|s| s.as_str()).collect();
        args.extend(owned);
        self.run_remote(endpoint, &args)
    }

    /// FS12: set tab title (`kitten @ set-tab-title`).
    pub fn set_tab_title(&self, session: &ProviderSession, title: &str) -> Result<()> {
        let endpoint = session.focus_endpoint.as_deref().ok_or_else(|| {
            TermorgError::ProviderCommand {
                message: "no focus endpoint for set-tab-title".into(),
            }
        })?;
        let tab_id = session.focus_tab_id.ok_or_else(|| TermorgError::ProviderCommand {
            message: "no tab id for set-tab-title".into(),
        })?;
        let tab_match = format!("id:{tab_id}");
        self.run_remote(
            endpoint,
            &["set-tab-title", "--match", &tab_match, title],
        )
    }

    /// Like `run_remote` but returns stdout (for `launch` window id).
    fn run_remote_capture(&self, listen: &str, args: &[String]) -> Result<String> {
        let mut last_err = String::new();
        for bin in ["kitten", "kitty"] {
            let mut cmd = Command::new("timeout");
            cmd.arg("5").arg(bin).arg("@").arg("--to").arg(listen);
            for a in args {
                cmd.arg(a);
            }
            cmd.stdin(Stdio::null());
            match cmd.output() {
                Ok(out) if out.status.success() => {
                    return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
                }
                Ok(out) => {
                    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                    last_err = if !stderr.is_empty() {
                        format!("{bin}: {stderr}")
                    } else {
                        format!("{bin}: exit {}", out.status.code().unwrap_or(-1))
                    };
                }
                Err(e) => last_err = format!("{bin}: {e}"),
            }
        }
        Err(TermorgError::ProviderCommand {
            message: format!("launch failed for {listen}: {last_err}"),
        })
    }

    /// Prefer an endpoint that already has a session in `cwd` (same project OS window).
    pub fn prefer_endpoint_for_cwd(&self, cwd: Option<&str>) -> Option<String> {
        let sessions = self.list_sessions().ok()?;
        if let Some(c) = cwd {
            if let Some(s) = sessions.iter().find(|s| s.cwd.as_deref() == Some(c)) {
                return s.focus_endpoint.clone();
            }
            // prefix match (subdir)
            if let Some(s) = sessions.iter().find(|s| {
                s.cwd
                    .as_deref()
                    .is_some_and(|sc| sc.starts_with(c) || c.starts_with(sc))
            }) {
                return s.focus_endpoint.clone();
            }
        }
        sessions
            .into_iter()
            .find_map(|s| s.focus_endpoint)
            .or_else(|| self.endpoints().into_iter().next())
    }
}

/// Try to bring the Kitty OS window to the foreground via the compositor.
/// Kitty remote control cannot reliably steal focus on GNOME/Wayland.
fn raise_os_window_best_effort(session: &ProviderSession, endpoint: &str) -> Result<()> {
    let pid = kitty_pid_from_endpoint(endpoint);
    let title_hint = session.title.replace('\n', " ");

    // wmctrl works when Kitty is on X11/XWayland and listed.
    if raise_with_wmctrl(pid, &title_hint) {
        return Ok(());
    }

    // xdotool (optional)
    if let Some(pid) = pid {
        if raise_with_xdotool(pid) {
            return Ok(());
        }
    }

    Ok(())
}

fn kitty_pid_from_endpoint(endpoint: &str) -> Option<i32> {
    let path = endpoint.strip_prefix("unix:").unwrap_or(endpoint);
    let name = Path::new(path).file_name()?.to_str()?;
    // control-12345.sock or control.sock-12345
    if let Some(rest) = name.strip_prefix("control-") {
        let num = rest.trim_end_matches(".sock");
        if let Ok(pid) = num.parse::<i32>() {
            return Some(pid);
        }
    }
    if let Some(idx) = name.rfind('-') {
        let maybe = name[idx + 1..].trim_end_matches(".sock");
        if let Ok(pid) = maybe.parse::<i32>() {
            return Some(pid);
        }
    }
    None
}

fn raise_with_wmctrl(pid: Option<i32>, title_hint: &str) -> bool {
    // wmctrl -lp → window_id desktop pid host title
    let Ok(output) = Command::new("wmctrl").arg("-lp").output() else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let title_lc = title_hint.to_lowercase();
    let mut candidate: Option<String> = None;

    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let win_id = parts.next().unwrap_or("");
        let _desk = parts.next();
        let line_pid = parts.next().and_then(|p| p.parse::<i32>().ok());
        // remainder is host + title; skip host
        let _host = parts.next();
        let title: String = parts.collect::<Vec<_>>().join(" ");

        let pid_match = match (pid, line_pid) {
            (Some(want), Some(have)) => want == have,
            _ => false,
        };
        let title_match = !title_lc.is_empty()
            && title.to_lowercase().contains(&title_lc)
            && title_lc.len() >= 3;

        // Prefer PID match (stable); title is fallback for XWayland classed windows.
        if pid_match || title_match {
            candidate = Some(win_id.to_string());
            if pid_match {
                break;
            }
        }
    }

    let Some(win_id) = candidate else {
        return false;
    };

    Command::new("wmctrl")
        .args(["-ia", &win_id])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn raise_with_xdotool(pid: i32) -> bool {
    let search = Command::new("xdotool")
        .args(["search", "--pid", &pid.to_string()])
        .output();
    let Ok(out) = search else {
        return false;
    };
    if !out.status.success() {
        return false;
    }
    let id = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if id.is_empty() {
        return false;
    }
    Command::new("xdotool")
        .args(["windowactivate", "--sync", &id])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Short stable tag from endpoint for session ids (avoids w1:t1 collisions across instances).
fn instance_tag(endpoint: &str) -> String {
    let path = endpoint.strip_prefix("unix:").unwrap_or(endpoint);
    let name = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("kitty");
    // control-12345.sock → 12345 ; control.sock-12345 → 12345
    if let Some(rest) = name.strip_prefix("control-") {
        let num = rest.trim_end_matches(".sock");
        if !num.is_empty() {
            return num.to_string();
        }
    }
    if let Some(idx) = name.rfind('-') {
        let maybe = &name[idx + 1..];
        let maybe = maybe.trim_end_matches(".sock");
        if maybe.chars().all(|c| c.is_ascii_digit()) {
            return maybe.to_string();
        }
    }
    // fallback: hash-ish from full path length + bytes
    format!("{:x}", simple_hash(path))
}

fn simple_hash(s: &str) -> u32 {
    let mut h: u32 = 5381;
    for b in s.bytes() {
        h = h.wrapping_mul(33).wrapping_add(u32::from(b));
    }
    h
}

fn discover_all_control_sockets() -> Vec<PathBuf> {
    let home = match std::env::var_os("HOME") {
        Some(h) => PathBuf::from(h),
        None => return Vec::new(),
    };
    let dir = home.join(".cache/kitty");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut socks: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| is_control_socket(p))
        .collect();
    socks.sort();
    socks.dedup();
    socks
}

fn is_control_socket(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    // control.sock, control.sock-PID, control-PID.sock, anything with control + sock
    let looks = name.contains("control")
        && (name.ends_with(".sock") || name.contains(".sock") || path.exists());
    if !looks {
        return false;
    }
    // Must be a live socket file.
    path.exists()
}

// --- Kitty `ls` JSON (subset) ------------------------------------------------

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

fn parse_kitty_ls(raw: &str, instance: &str, endpoint: &str) -> Result<Vec<ProviderSession>> {
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
            let hint = crate::signals::MatchHint {
                cwd: cwd.as_deref(),
                kitty_pid: Some(instance),
                kitty_window_id: focus_window_id,
            };
            let attention = attention::classify(&id, agent, at_prompt, &title, &pids, hint);
            sessions.push(ProviderSession {
                provider: PROVIDER_ID.into(),
                // Instance tag prevents id collisions across OS windows / processes.
                id,
                title,
                cwd,
                os_window_id: Some(osw.id),
                is_focused: tab.is_focused,
                focus_endpoint: Some(endpoint.to_string()),
                focus_tab_id: Some(tab.id),
                focus_window_id,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tabs_with_instance_prefix() {
        let sample = r#"[
          {
            "id": 1,
            "tabs": [
              {
                "id": 10,
                "title": "vim",
                "is_focused": true,
                "windows": [{
                  "id": 100, "title": "vim", "cwd": "/home/a/proj",
                  "foreground_processes": [{"pid": 1, "cmdline": ["claude", "--dangerously-skip-permissions"]}]
                }]
              },
              {
                "id": 11,
                "title": "shell",
                "is_focused": false,
                "windows": [{ "id": 101, "title": "zsh", "cwd": "/tmp", "pid": 2 }]
              }
            ]
          }
        ]"#;
        let sessions = parse_kitty_ls(sample, "42", "unix:/tmp/x.sock").unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].id, "42:w1:t10");
        assert_eq!(sessions[0].title, "vim");
        assert_eq!(sessions[0].cwd.as_deref(), Some("/home/a/proj"));
        assert_eq!(sessions[0].focus_tab_id, Some(10));
        assert_eq!(sessions[0].focus_window_id, Some(100));
        assert_eq!(sessions[0].focus_endpoint.as_deref(), Some("unix:/tmp/x.sock"));
        assert_eq!(sessions[0].agent, AgentClass::Claude);
        assert_eq!(sessions[1].id, "42:w1:t11");
        // at_prompt absent → attention may be unknown/working depending on agent
        let _ = Attention::Unknown;
    }

    #[test]
    fn instance_tag_from_pid_socket() {
        assert_eq!(
            instance_tag("unix:/home/u/.cache/kitty/control-12345.sock"),
            "12345"
        );
        assert_eq!(
            instance_tag("unix:/home/u/.cache/kitty/control.sock-99"),
            "99"
        );
    }
}
