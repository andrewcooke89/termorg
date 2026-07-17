//! Kitty terminal provider.

mod ls;
mod raise;
mod remote;

use super::{Capabilities, LaunchRequest, LaunchResult, ProviderSession, TerminalProvider};
use crate::error::{Result, TermorgError};
use ls::parse_kitty_ls;
use remote::{discover_all_control_sockets, instance_tag, run_remote, run_remote_capture};

const PROVIDER_ID: &str = "kitty";

/// Default [`TerminalProvider`] using Kitty remote control.
pub struct KittyProvider {
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

    /// Prefer an endpoint that already has a session in `cwd`.
    pub fn prefer_endpoint_for_cwd(&self, cwd: Option<&str>) -> Option<String> {
        let sessions = self.list_sessions().ok()?;
        if let Some(c) = cwd {
            if let Some(s) = sessions.iter().find(|s| s.cwd.as_deref() == Some(c)) {
                return s.focus_endpoint.clone();
            }
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

    /// Set tab bar colors (`kitten @ set-tab-color`).
    pub fn set_tab_color(&self, session: &ProviderSession, color_args: &[String]) -> Result<()> {
        let endpoint =
            session
                .focus_endpoint
                .as_deref()
                .ok_or_else(|| TermorgError::ProviderCommand {
                    message: "no focus endpoint for set-tab-color".into(),
                })?;
        let tab_id = session
            .focus_tab_id
            .ok_or_else(|| TermorgError::ProviderCommand {
                message: "no tab id for set-tab-color".into(),
            })?;
        let tab_match = format!("id:{tab_id}");
        let mut args: Vec<&str> = vec!["set-tab-color", "--match", &tab_match];
        let owned: Vec<&str> = color_args.iter().map(|s| s.as_str()).collect();
        args.extend(owned);
        run_remote(endpoint, &args)
    }

    /// Set tab title (`kitten @ set-tab-title`).
    pub fn set_tab_title(&self, session: &ProviderSession, title: &str) -> Result<()> {
        let endpoint =
            session
                .focus_endpoint
                .as_deref()
                .ok_or_else(|| TermorgError::ProviderCommand {
                    message: "no focus endpoint for set-tab-title".into(),
                })?;
        let tab_id = session
            .focus_tab_id
            .ok_or_else(|| TermorgError::ProviderCommand {
                message: "no tab id for set-tab-title".into(),
            })?;
        let tab_match = format!("id:{tab_id}");
        run_remote(endpoint, &["set-tab-title", "--match", &tab_match, title])
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
            ambient: true,
        }
    }

    fn list_sessions(&self) -> Result<Vec<ProviderSession>> {
        let endpoints = self.endpoints();
        if endpoints.is_empty() {
            return Err(TermorgError::ProviderUnavailable {
                provider: PROVIDER_ID.into(),
                message: "No Kitty remote-control sockets found.

                     Each Kitty OS window needs its own listen socket. In kitty.conf:
                       allow_remote_control socket-only
                       listen_on unix:${HOME}/.cache/kitty/control-{kitty_pid}.sock

                     Restart Kitty after changing listen_on."
                    .to_string(),
            });
        }

        let mut sessions = Vec::new();
        let mut errors = Vec::new();
        for ep in &endpoints {
            match run_remote_capture(ep, &["ls".into()]) {
                Ok(raw) => {
                    let instance = instance_tag(ep);
                    match parse_kitty_ls(&raw, &instance, ep) {
                        Ok(mut list) => sessions.append(&mut list),
                        Err(e) => errors.push(format!("{ep}: {e}")),
                    }
                }
                Err(e) => errors.push(format!("{ep}: {e}")),
            }
        }

        if sessions.is_empty() && !errors.is_empty() {
            return Err(TermorgError::ProviderUnavailable {
                provider: PROVIDER_ID.into(),
                message: "No Kitty remote-control sockets found.\n\nEach Kitty OS window needs its own listen socket. In kitty.conf:\n  allow_remote_control socket-only\n  listen_on unix:${HOME}/.cache/kitty/control-{kitty_pid}.sock\n\nRestart Kitty after changing listen_on."
                .to_string(),
            });
        }

        sessions.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(sessions)
    }

    fn focus(&self, session: &ProviderSession) -> Result<()> {
        let endpoint =
            session
                .focus_endpoint
                .as_deref()
                .ok_or_else(|| TermorgError::ProviderCommand {
                    message: format!(
                        "session {} has no focus endpoint (stale list? refresh and try again)",
                        session.id
                    ),
                })?;
        let tab_id = session
            .focus_tab_id
            .ok_or_else(|| TermorgError::ProviderCommand {
                message: format!("session {} has no tab id for focus", session.id),
            })?;

        let tab_match = format!("id:{tab_id}");
        run_remote(endpoint, &["focus-tab", "--match", &tab_match])?;

        if let Some(wid) = session.focus_window_id {
            let win_match = format!("id:{wid}");
            let _ = run_remote(endpoint, &["focus-window", "--match", &win_match]);
        }

        if let Some(os_id) = session.os_window_id {
            let _ = run_remote(endpoint, &["action", "nth_os_window", &os_id.to_string()]);
        }

        if let Some(wid) = session.focus_window_id {
            let win_match = format!("id:{wid}");
            let _ = run_remote(
                endpoint,
                &["resize-os-window", "--action=show", "--match", &win_match],
            );
        }

        let _ = raise::raise_os_window_best_effort(session, endpoint);
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

        let raw = run_remote_capture(&endpoint, &args)?;
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
            native_id: window_id.map(|w| w.to_string()),
        })
    }

    fn set_tab_title(&self, session: &ProviderSession, title: &str) -> Result<()> {
        KittyProvider::set_tab_title(self, session, title)
    }

    fn set_tab_color(&self, session: &ProviderSession, color_args: &[String]) -> Result<()> {
        KittyProvider::set_tab_color(self, session, color_args)
    }

    fn prefer_launch_endpoint(&self, cwd: Option<&str>) -> Option<String> {
        self.prefer_endpoint_for_cwd(cwd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use remote::instance_tag;

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

    #[test]
    fn parses_tabs_with_instance_prefix() {
        let raw = r#"[{
            "id": 1,
            "tabs": [{
                "id": 2,
                "title": "t",
                "is_focused": true,
                "windows": [{
                    "id": 3,
                    "title": "w",
                    "cwd": "/tmp",
                    "at_prompt": true,
                    "foreground_processes": [{"pid": 1, "cmdline": ["zsh"]}]
                }]
            }]
        }]"#;
        let sessions = parse_kitty_ls(raw, "99", "unix:/tmp/control-99.sock").unwrap();
        assert_eq!(sessions.len(), 1);
        assert!(sessions[0].id.starts_with("99:"));
        assert_eq!(sessions[0].focus_tab_id, Some(2));
    }
}
