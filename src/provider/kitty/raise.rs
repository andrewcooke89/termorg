//! Best-effort OS-window raise (often blocked on Wayland).

use std::path::Path;
use std::process::Command;

use crate::error::Result;
use crate::provider::ProviderSession;

pub(super) fn raise_os_window_best_effort(session: &ProviderSession, endpoint: &str) -> Result<()> {
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
        let title_match =
            !title_lc.is_empty() && title.to_lowercase().contains(&title_lc) && title_lc.len() >= 3;

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
