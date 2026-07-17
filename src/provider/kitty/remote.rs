//! Low-level Kitty remote-control invocation and socket discovery.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::error::{Result, TermorgError};

pub(super) fn run_remote(listen: &str, args: &[&str]) -> Result<()> {
    let owned: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    run_remote_capture(listen, &owned).map(|_| ())
}

pub(super) fn run_remote_capture(listen: &str, args: &[String]) -> Result<String> {
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
        message: format!("remote control failed for {listen}: {last_err}"),
    })
}

/// Short stable tag from endpoint for session ids.
pub(super) fn instance_tag(endpoint: &str) -> String {
    let path = endpoint.strip_prefix("unix:").unwrap_or(endpoint);
    let name = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("kitty");
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
    format!("{:x}", simple_hash(path))
}

fn simple_hash(s: &str) -> u32 {
    let mut h: u32 = 5381;
    for b in s.bytes() {
        h = h.wrapping_mul(33).wrapping_add(u32::from(b));
    }
    h
}

pub(super) fn discover_all_control_sockets() -> Vec<PathBuf> {
    let home = match std::env::var_os("HOME") {
        Some(h) => PathBuf::from(h),
        None => return Vec::new(),
    };
    let dir = home.join(".cache/kitty");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| is_control_socket(p))
        .collect();
    out.sort();
    out
}

fn is_control_socket(path: &Path) -> bool {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    (name.starts_with("control") || name.contains("control"))
        && (name.ends_with(".sock") || name.contains(".sock"))
        && path.exists()
}
