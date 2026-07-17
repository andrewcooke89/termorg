//! Tmux provider — one termorg session = one tmux **window** (tab analogue).
//!
//! Uses `tmux list-windows -a`, `select-window`, `new-window` / `new-session`,
//! `rename-window`, and window styles for ambient cues.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{Capabilities, LaunchRequest, LaunchResult, ProviderSession, TerminalProvider};
use crate::agent;
use crate::attention;
use crate::error::{Result, TermorgError};

const PROVIDER_ID: &str = "tmux";

/// Control a tmux server (default socket or named `-L` / `-S`).
#[derive(Debug, Clone)]
pub struct TmuxProvider {
    /// `-L` socket name (default `"default"`). Ignored if `socket_path` is set.
    socket_name: String,
    /// Optional absolute socket path (`-S`).
    socket_path: Option<PathBuf>,
}

impl Default for TmuxProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl TmuxProvider {
    pub fn new() -> Self {
        Self {
            socket_name: std::env::var("TERMORG_TMUX_SOCKET")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "default".into()),
            socket_path: std::env::var_os("TERMORG_TMUX_SOCKET_PATH").map(PathBuf::from),
        }
    }

    pub fn with_socket_name(name: impl Into<String>) -> Self {
        Self {
            socket_name: name.into(),
            socket_path: None,
        }
    }

    pub fn with_socket_path(path: impl Into<PathBuf>) -> Self {
        Self {
            socket_name: "custom".into(),
            socket_path: Some(path.into()),
        }
    }

    fn socket_tag(&self) -> String {
        if let Some(ref p) = self.socket_path {
            p.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("tmux")
                .to_string()
        } else {
            self.socket_name.clone()
        }
    }

    fn base_cmd(&self) -> Command {
        let mut cmd = Command::new("tmux");
        if let Some(ref p) = self.socket_path {
            cmd.arg("-S").arg(p);
        } else {
            cmd.arg("-L").arg(&self.socket_name);
        }
        cmd.stdin(Stdio::null());
        cmd
    }

    fn run(&self, args: &[&str]) -> Result<String> {
        let mut cmd = self.base_cmd();
        for a in args {
            cmd.arg(a);
        }
        let out = cmd.output().map_err(|e| TermorgError::ProviderCommand {
            message: format!("tmux: {e}"),
        })?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let code = out.status.code().unwrap_or(-1);
            return Err(TermorgError::ProviderCommand {
                message: if stderr.is_empty() {
                    format!("tmux exit {code} ({args:?})")
                } else {
                    format!("tmux: {stderr}")
                },
            });
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn server_available(&self) -> bool {
        self.run(&["list-sessions"]).is_ok()
    }

    /// Parse `list-windows -a` formatted lines.
    pub(crate) fn parse_windows_output(raw: &str, socket_tag: &str) -> Vec<ProviderSession> {
        let mut sessions = Vec::new();
        for line in raw.lines() {
            if line.trim().is_empty() {
                continue;
            }
            // session \t window_id \t index \t name \t active \t path \t pid \t cmd \t attached \t pane_id
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 8 {
                continue;
            }
            let session_name = parts[0];
            let window_id = parts[1]; // @N
            let window_index: u32 = parts[2].parse().unwrap_or(0);
            let window_name = parts[3];
            let window_active = parts[4] == "1";
            let cwd = parts[5];
            let pane_pid: i32 = parts[6].parse().unwrap_or(0);
            let pane_cmd = parts[7];
            let session_attached = parts.get(8).map(|s| *s == "1").unwrap_or(false);
            let pane_id = parts.get(9).map(|s| s.to_string());

            let id = format!("tmux:{socket_tag}:{window_id}");
            let focus_key = format!("{session_name}:{window_id}");
            let is_focused = window_active && session_attached;

            let pids = if pane_pid > 0 { vec![pane_pid] } else { vec![] };
            let cmdlines = if pane_cmd.is_empty() {
                vec![]
            } else {
                vec![pane_cmd.to_string()]
            };
            let agent = agent::classify_session(&pids, &cmdlines, window_name);
            // at_prompt: shell-like foreground with idle attention path
            let at_prompt =
                if matches!(agent, agent::AgentClass::Shell | agent::AgentClass::Unknown) {
                    Some(true)
                } else {
                    Some(false)
                };

            let win_num = window_id
                .strip_prefix('@')
                .and_then(|s| s.parse::<u32>().ok());

            // focus_key: "session:@N|%pane" (pane optional, for hook matching)
            let full_key = match pane_id.as_deref().filter(|p| !p.is_empty()) {
                Some(pid) => format!("{focus_key}|{pid}"),
                None => focus_key,
            };
            let pane_for_hint = full_key.split('|').nth(1).map(|s| s.to_string());
            let cwd_owned = if cwd.is_empty() {
                None
            } else {
                Some(cwd.to_string())
            };
            let hint = crate::signals::MatchHint {
                cwd: cwd_owned.as_deref(),
                kitty_pid: None,
                kitty_window_id: None,
                tmux_pane: pane_for_hint.as_deref(),
            };
            let attention = attention::classify(&id, agent, at_prompt, window_name, &pids, hint);

            sessions.push(ProviderSession {
                provider: PROVIDER_ID.into(),
                id,
                title: window_name.into(),
                cwd: cwd_owned,
                is_focused,
                os_window_id: None,
                focus_endpoint: Some(socket_tag.to_string()),
                focus_tab_id: Some(window_index),
                focus_window_id: win_num,
                focus_key: Some(full_key),
                agent,
                attention,
            });
        }
        sessions
    }

    fn list_format() -> &'static str {
        "#{session_name}\t#{window_id}\t#{window_index}\t#{window_name}\t#{window_active}\t#{pane_current_path}\t#{pane_pid}\t#{pane_current_command}\t#{session_attached}\t#{pane_id}"
    }

    /// Parse `focus_key` → `(session_name, window_target, pane_id?)`.
    ///
    /// Formats: `session:@N`, `session:@N|%pane`. Window id is always `@…`.
    /// Session name may contain `:` — we split on the last `:` before `|`.
    pub(crate) fn parse_focus_key(key: &str) -> Option<(&str, &str, Option<&str>)> {
        let key = key.trim();
        if key.is_empty() {
            return None;
        }
        let (base, pane) = match key.split_once('|') {
            Some((b, p)) if !p.is_empty() => (b, Some(p)),
            _ => (key.split('|').next().unwrap_or(key), None),
        };
        let (sess, win) = base.rsplit_once(':')?;
        if sess.is_empty() || !win.starts_with('@') {
            return None;
        }
        Some((sess, win, pane))
    }

    fn target_window(session: &ProviderSession) -> Result<String> {
        // focus_key: "session:@N|%pane" or "session:@N" → tmux target "session:@N"
        if let Some(ref key) = session.focus_key {
            if let Some((sess, win, _)) = Self::parse_focus_key(key) {
                return Ok(format!("{sess}:{win}"));
            }
            // Fallback: strip pane suffix only
            let base = key.split('|').next().unwrap_or(key);
            if !base.is_empty() {
                return Ok(base.to_string());
            }
        }
        Err(TermorgError::ProviderCommand {
            message: format!("tmux session {} missing focus_key", session.id),
        })
    }

    fn session_name_from_key(focus_key: &str) -> Option<&str> {
        Self::parse_focus_key(focus_key).map(|(s, _, _)| s)
    }
}

impl TerminalProvider for TmuxProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            list: true,
            focus: true,
            launch: true,
            ambient: true,
        }
    }

    fn list_sessions(&self) -> Result<Vec<ProviderSession>> {
        // Ensure tmux binary exists
        if Command::new("tmux")
            .arg("-V")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| !s.success())
            .unwrap_or(true)
        {
            return Err(TermorgError::ProviderUnavailable {
                provider: PROVIDER_ID.into(),
                message: "tmux not found on PATH".into(),
            });
        }

        let raw = self
            .run(&["list-windows", "-a", "-F", Self::list_format()])
            .map_err(|e| {
                // No server yet
                TermorgError::ProviderUnavailable {
                    provider: PROVIDER_ID.into(),
                    message: format!(
                        "{e}\n\nStart a tmux session first (`tmux new -s work`) or check TERMORG_TMUX_SOCKET."
                    ),
                }
            })?;

        let mut sessions = Self::parse_windows_output(&raw, &self.socket_tag());
        sessions.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(sessions)
    }

    fn focus(&self, session: &ProviderSession) -> Result<()> {
        let target = Self::target_window(session)?;
        // Select the window; switch client so attached users see it.
        self.run(&["select-window", "-t", &target])?;
        if let Some(sess) = Self::session_name_from_key(session.focus_key.as_deref().unwrap_or(""))
        {
            let _ = self.run(&["switch-client", "-t", sess]);
        }
        Ok(())
    }

    fn launch(&self, req: &LaunchRequest) -> Result<LaunchResult> {
        let cwd = req.cwd.as_deref().filter(|c| !c.is_empty());
        if let Some(c) = cwd {
            if !Path::new(c).is_dir() {
                return Err(TermorgError::ProviderCommand {
                    message: format!("cwd is not a directory: {c}"),
                });
            }
        }

        let title = req
            .tab_title
            .clone()
            .unwrap_or_else(|| req.kind.tab_title_hint(cwd));

        // Pick a session: prefer endpoint (tmux session name), else first existing, else create.
        // Do not special-case the name "default" — that is a valid session name.
        // Kitty unix: endpoints must not be treated as session names.
        let session_name = req
            .endpoint
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .filter(|s| !s.starts_with("unix:") && !s.starts_with("tcp:"))
            .map(|s| s.to_string())
            .or_else(|| {
                self.run(&["display-message", "-p", "#{session_name}"])
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| {
                self.run(&["list-sessions", "-F", "#{session_name}"])
                    .ok()
                    .and_then(|s| s.lines().next().map(|l| l.to_string()))
            });

        let cmd_argv = req.kind.command_argv();
        let shell_cmd = if cmd_argv.is_empty() {
            None
        } else {
            Some(cmd_argv.join(" "))
        };

        let raw = if let Some(ref sess) = session_name {
            let mut args: Vec<String> = vec![
                "new-window".into(),
                "-P".into(),
                "-F".into(),
                "#{window_id}".into(),
                "-t".into(),
                sess.clone(),
                "-n".into(),
                title.clone(),
            ];
            if let Some(c) = cwd {
                args.push("-c".into());
                args.push(c.into());
            }
            if let Some(ref sc) = shell_cmd {
                args.push(sc.clone());
            }
            let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            self.run(&arg_refs)?
        } else {
            // Create detached session with first window
            let name = "termorg";
            let mut args: Vec<String> = vec![
                "new-session".into(),
                "-d".into(),
                "-P".into(),
                "-F".into(),
                "#{window_id}".into(),
                "-s".into(),
                name.into(),
                "-n".into(),
                title.clone(),
            ];
            if let Some(c) = cwd {
                args.push("-c".into());
                args.push(c.into());
            }
            if let Some(ref sc) = shell_cmd {
                args.push(sc.clone());
            }
            let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            self.run(&arg_refs)?
        };

        let window_id = raw.trim().to_string();
        let win_num = window_id
            .strip_prefix('@')
            .and_then(|s| s.parse::<u32>().ok());

        Ok(LaunchResult {
            window_id: win_num,
            endpoint: session_name.unwrap_or_else(|| "termorg".into()),
            cwd: cwd.map(|s| s.to_string()),
            kind: req.kind,
            native_id: Some(window_id),
        })
    }

    fn set_tab_title(&self, session: &ProviderSession, title: &str) -> Result<()> {
        let target = Self::target_window(session)?;
        self.run(&["rename-window", "-t", &target, title])?;
        Ok(())
    }

    fn set_tab_color(&self, session: &ProviderSession, color_args: &[String]) -> Result<()> {
        let target = Self::target_window(session)?;
        // Map Kitty-style active_bg=#rrggbb into tmux window-style.
        let mut bg = None;
        let mut fg = None;
        for a in color_args {
            if let Some(v) = a.strip_prefix("active_bg=") {
                if v != "NONE" {
                    bg = Some(v.to_string());
                }
            }
            if let Some(v) = a.strip_prefix("active_fg=") {
                if v != "NONE" {
                    fg = Some(v.to_string());
                }
            }
        }
        if bg.is_none() && fg.is_none() {
            // Reset styles
            let _ = self.run(&["set-option", "-w", "-u", "-t", &target, "window-style"]);
            let _ = self.run(&[
                "set-option",
                "-w",
                "-u",
                "-t",
                &target,
                "window-active-style",
            ]);
            return Ok(());
        }
        let mut style = String::new();
        if let Some(bg) = bg {
            style.push_str("bg=");
            style.push_str(&bg);
        }
        if let Some(fg) = fg {
            if !style.is_empty() {
                style.push(',');
            }
            style.push_str("fg=");
            style.push_str(&fg);
        }
        self.run(&["set-option", "-w", "-t", &target, "window-style", &style])?;
        self.run(&[
            "set-option",
            "-w",
            "-t",
            &target,
            "window-active-style",
            &style,
        ])?;
        Ok(())
    }

    fn prefer_launch_endpoint(&self, cwd: Option<&str>) -> Option<String> {
        let sessions = self.list_sessions().ok()?;
        let session_name = |s: &ProviderSession| -> Option<String> {
            s.focus_key
                .as_ref()
                .and_then(|k| Self::parse_focus_key(k).map(|(sess, _, _)| sess.to_string()))
        };
        if let Some(c) = cwd {
            if let Some(s) = sessions.iter().find(|s| s.cwd.as_deref() == Some(c)) {
                if let Some(name) = session_name(s) {
                    return Some(name);
                }
            }
        }
        sessions.into_iter().find_map(|s| session_name(&s))
    }
}

/// True if a default tmux server appears reachable.
pub fn tmux_available() -> bool {
    TmuxProvider::new().server_available()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{LaunchKind, LaunchRequest, TerminalProvider};

    #[test]
    fn parse_sample_windows() {
        let raw = "\
work\t@12\t1\tclaude\t1\t/home/u/proj\t1234\tclaude\t1\t%5
work\t@13\t2\tzsh\t0\t/home/u/proj\t1235\tzsh\t1\t%6
other\t@20\t1\tbash\t1\t/tmp\t99\tbash\t0\t%1
";
        let sessions = TmuxProvider::parse_windows_output(raw, "default");
        assert_eq!(sessions.len(), 3);
        assert_eq!(sessions[0].id, "tmux:default:@12");
        assert_eq!(sessions[0].provider, "tmux");
        assert!(sessions[0].is_focused); // active + attached
        assert!(!sessions[1].is_focused); // inactive window
        assert!(!sessions[2].is_focused); // session not attached
        assert_eq!(sessions[0].cwd.as_deref(), Some("/home/u/proj"));
        assert_eq!(sessions[0].focus_key.as_deref(), Some("work:@12|%5"));
        assert_eq!(sessions[0].focus_window_id, Some(12));
        assert_eq!(sessions[0].focus_tab_id, Some(1));
        assert_eq!(sessions[0].title, "claude");
    }

    #[test]
    fn parse_empty_and_malformed_do_not_invent_sessions() {
        assert!(TmuxProvider::parse_windows_output("", "default").is_empty());
        assert!(TmuxProvider::parse_windows_output("\n\n  \n", "s").is_empty());
        // too few columns
        let bad = "only\tfew\tcols\n";
        assert!(TmuxProvider::parse_windows_output(bad, "s").is_empty());
        // 8 fields minimum (no attached/pane) still accepted
        let eight = "work\t@1\t0\tzsh\t1\t/tmp\t1\tzsh\n";
        let s = TmuxProvider::parse_windows_output(eight, "sock");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].id, "tmux:sock:@1");
        assert_eq!(s[0].focus_key.as_deref(), Some("work:@1")); // no pane
        assert!(!s[0].is_focused); // attached defaults false when field missing
        assert_eq!(s[0].cwd.as_deref(), Some("/tmp"));
    }

    #[test]
    fn parse_empty_cwd_is_none() {
        let raw = "s\t@2\t0\tn\t0\t\t0\t\t1\t%9\n";
        let s = TmuxProvider::parse_windows_output(raw, "d");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].cwd, None);
        assert_eq!(s[0].focus_key.as_deref(), Some("s:@2|%9"));
        assert!(!s[0].is_focused); // inactive window even if attached
    }

    #[test]
    fn parse_focus_key_handles_colon_in_session_name() {
        let (sess, win, pane) = TmuxProvider::parse_focus_key("proj:main:@7|%3").expect("parse");
        assert_eq!(sess, "proj:main");
        assert_eq!(win, "@7");
        assert_eq!(pane, Some("%3"));
        let (sess2, win2, pane2) = TmuxProvider::parse_focus_key("work:@1").expect("parse");
        assert_eq!(sess2, "work");
        assert_eq!(win2, "@1");
        assert_eq!(pane2, None);
        assert!(TmuxProvider::parse_focus_key("").is_none());
        assert!(TmuxProvider::parse_focus_key("nocolon").is_none());
    }

    #[test]
    fn prefer_endpoint_uses_parse_focus_key_for_colon_session() {
        // Synthetic list line with a session name containing ':'.
        // prefer_launch_endpoint must return the full name, not truncated at first ':'.
        let sessions = TmuxProvider::parse_windows_output(
            "proj:main\t@7\t0\tsh\t0\t/tmp/colon-proj\t1\tzsh\t0\t%3\n",
            "sock",
        );
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].focus_key.as_deref(), Some("proj:main:@7|%3"));
        let name = sessions[0]
            .focus_key
            .as_ref()
            .and_then(|k| TmuxProvider::parse_focus_key(k).map(|(s, _, _)| s.to_string()));
        assert_eq!(name.as_deref(), Some("proj:main"));
        // Old bug: split(':').next() → "proj"
        let truncated = sessions[0]
            .focus_key
            .as_ref()
            .and_then(|k| k.split('|').next()?.split(':').next())
            .map(|s| s.to_string());
        assert_eq!(truncated.as_deref(), Some("proj"));
        assert_ne!(name, truncated);
    }

    #[test]
    fn target_window_from_session_focus_key() {
        let mut s = TmuxProvider::parse_windows_output(
            "work\t@12\t1\tx\t1\t/tmp\t1\tzsh\t1\t%5\n",
            "default",
        );
        let sess = s.remove(0);
        assert_eq!(TmuxProvider::target_window(&sess).unwrap(), "work:@12");
    }

    /// Live control plane on an isolated tmux socket (does not touch user server).
    #[test]
    fn live_list_focus_launch_ambient_isolated_socket() {
        if Command::new("tmux")
            .arg("-V")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| !s.success())
            .unwrap_or(true)
        {
            eprintln!("tmux binary missing — skip live control-plane test");
            return;
        }

        let sock = format!("termorg-ut-{}", std::process::id());
        let _ = Command::new("tmux")
            .args(["-L", &sock, "kill-server"])
            .output();
        let ok = Command::new("tmux")
            .args([
                "-L",
                &sock,
                "new-session",
                "-d",
                "-s",
                "smoke",
                "-n",
                "main",
                "-c",
                "/tmp",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            eprintln!("could not start isolated tmux — skip live test");
            return;
        }

        let provider = TmuxProvider::with_socket_name(&sock);
        let listed = provider.list_sessions().expect("list");
        assert_eq!(listed.len(), 1, "expected one window: {listed:?}");
        assert_eq!(listed[0].provider, "tmux");
        assert!(listed[0].id.contains("@"), "id={}", listed[0].id);
        assert!(
            listed[0]
                .focus_key
                .as_deref()
                .unwrap_or("")
                .contains("smoke:"),
            "focus_key={:?}",
            listed[0].focus_key
        );
        assert_eq!(listed[0].cwd.as_deref(), Some("/tmp"));

        provider.focus(&listed[0]).expect("focus");

        provider
            .set_tab_title(&listed[0], "ambient-title")
            .expect("title");
        let after_title = provider.list_sessions().expect("list2");
        assert_eq!(after_title[0].title, "ambient-title");
        // focus targeting must still work after rename
        provider.focus(&after_title[0]).expect("focus after rename");

        provider
            .set_tab_color(
                &after_title[0],
                &["active_bg=#ff0000".into(), "active_fg=#ffffff".into()],
            )
            .expect("color");
        // reset colors
        provider
            .set_tab_color(
                &after_title[0],
                &["active_bg=NONE".into(), "active_fg=NONE".into()],
            )
            .expect("color reset");

        let launched = provider
            .launch(&LaunchRequest {
                kind: LaunchKind::Shell,
                cwd: Some("/tmp".into()),
                endpoint: Some("smoke".into()),
                tab_title: Some("launched".into()),
            })
            .expect("launch shell");
        assert!(
            launched.native_id.as_deref().unwrap_or("").starts_with('@'),
            "native_id={:?}",
            launched.native_id
        );
        assert_eq!(launched.endpoint, "smoke");

        let after = provider.list_sessions().expect("list after launch");
        assert!(after.len() >= 2, "expected ≥2 windows, got {}", after.len());
        let nid = launched.native_id.as_deref().unwrap();
        let found = after
            .iter()
            .find(|s| crate::provider::session_matches_native_id(s, nid))
            .unwrap_or_else(|| panic!("launched {nid} not found exactly in {after:?}"));
        assert_eq!(found.id.rsplit(':').next(), Some(nid));
        assert_eq!(
            found.focus_window_id, launched.window_id,
            "focus_window_id must equal launch window_id"
        );
        // Ensure @1-style prefix confusion cannot pass: if nid is @N, no other id tail may match.
        for s in &after {
            if crate::provider::session_matches_native_id(s, nid) {
                assert_eq!(s.id.rsplit(':').next(), Some(nid));
            }
        }

        // agent-kind launch path (command string); may exit quickly if binary missing —
        // only assert the window appears when launch returns Ok.
        if let Ok(l2) = provider.launch(&LaunchRequest {
            kind: LaunchKind::Claude,
            cwd: Some("/tmp".into()),
            endpoint: Some("smoke".into()),
            tab_title: Some("claude-win".into()),
        }) {
            let again = provider.list_sessions().expect("list claude");
            let nid2 = l2.native_id.as_deref().unwrap_or("");
            assert!(
                !nid2.is_empty()
                    && again
                        .iter()
                        .any(|s| crate::provider::session_matches_native_id(s, nid2)),
                "claude window {nid2} missing exactly in {again:?}"
            );
        }

        let _ = Command::new("tmux")
            .args(["-L", &sock, "kill-server"])
            .output();
    }
}
