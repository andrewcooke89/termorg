//! FS5 — classify agent/tool running in a session (process tree + title).

use std::fs;
use std::path::PathBuf;

/// Stable agent classes (product palette).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentClass {
    Claude,
    Grok,
    Kilo,
    Codex,
    Shell,
    Other,
    Unknown,
}

impl AgentClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Grok => "grok",
            Self::Kilo => "kilo",
            Self::Codex => "codex",
            Self::Shell => "shell",
            Self::Other => "other",
            Self::Unknown => "unknown",
        }
    }

    /// Short UI label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Grok => "Grok",
            Self::Kilo => "Kilo",
            Self::Codex => "Codex",
            Self::Shell => "shell",
            Self::Other => "other",
            Self::Unknown => "?",
        }
    }

    /// Distinct RGB for panel chips (Tokyo Night-adjacent).
    pub fn rgb(self) -> (u8, u8, u8) {
        match self {
            Self::Claude => (247, 118, 142), // red/pink
            Self::Grok => (122, 162, 247),   // blue
            Self::Kilo => (187, 154, 247),   // purple
            Self::Codex => (158, 206, 106),  // green
            Self::Shell => (86, 95, 137),    // muted
            Self::Other => (224, 175, 104),  // amber
            Self::Unknown => (65, 72, 104),  // dim
        }
    }
}

impl std::fmt::Display for AgentClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Classify from free-form text (cmdline, comm, title).
pub fn classify_text(text: &str) -> Option<AgentClass> {
    let t = text.to_lowercase();
    // Order matters: more specific first.
    if t.contains("claude-desktop") {
        return Some(AgentClass::Other); // GUI app, not terminal agent
    }
    if t.contains("claude") {
        return Some(AgentClass::Claude);
    }
    if t.contains("codex") {
        return Some(AgentClass::Codex);
    }
    // kilo binary / kilo code — avoid matching random "kilobyte" etc. via path segments
    if t.contains("kilo-code")
        || t.contains("/kilo ")
        || t.contains("/kilo\t")
        || t.ends_with("/kilo")
        || t.contains(" kilo ")
        || t.starts_with("kilo ")
        || t == "kilo"
        || t.contains("kilo_code")
        || t.contains(".kilo")
    {
        return Some(AgentClass::Kilo);
    }
    if t.contains("grok") {
        return Some(AgentClass::Grok);
    }
    None
}

/// Shell-like process names (when only these, class = shell).
fn is_shell_name(name: &str) -> bool {
    matches!(
        name,
        "bash"
            | "zsh"
            | "sh"
            | "fish"
            | "dash"
            | "nu"
            | "pwsh"
            | "tmux"
            | "screen"
            | "login"
            | "sudo"
    )
}

/// Classify a session from pids/cmdlines and optional title.
pub fn classify_session(pid_hints: &[i32], cmdlines: &[String], title: &str) -> AgentClass {
    // 1) Direct cmdline strings from provider.
    for c in cmdlines {
        if let Some(a) = classify_text(c) {
            return a;
        }
    }

    // 2) Walk process trees from window / foreground pids.
    let mut saw_shell = false;
    let mut saw_other = false;
    for &pid in pid_hints {
        if pid <= 0 {
            continue;
        }
        match classify_tree(pid, 0) {
            AgentClass::Claude => return AgentClass::Claude,
            AgentClass::Grok => return AgentClass::Grok,
            AgentClass::Kilo => return AgentClass::Kilo,
            AgentClass::Codex => return AgentClass::Codex,
            AgentClass::Other => saw_other = true,
            AgentClass::Shell => saw_shell = true,
            AgentClass::Unknown => {}
        }
    }

    // 3) Title heuristics (often last command / agent name).
    if let Some(a) = classify_text(title) {
        return a;
    }

    if saw_other {
        return AgentClass::Other;
    }
    if saw_shell || pid_hints.iter().any(|&p| p > 0 && tree_is_shell_only(p, 0)) {
        return AgentClass::Shell;
    }

    AgentClass::Unknown
}

fn classify_tree(pid: i32, depth: u8) -> AgentClass {
    if depth > 12 || pid <= 0 {
        return AgentClass::Unknown;
    }
    let comm = read_comm(pid).unwrap_or_default();
    let cmdline = read_cmdline(pid).unwrap_or_default();
    let blob = format!("{comm} {cmdline}");
    if let Some(a) = classify_text(&blob) {
        return a;
    }
    // children
    for child in children_of(pid) {
        let c = classify_tree(child, depth + 1);
        if !matches!(c, AgentClass::Shell | AgentClass::Unknown) {
            return c;
        }
    }
    if is_shell_name(comm.trim()) {
        return AgentClass::Shell;
    }
    AgentClass::Unknown
}

fn tree_is_shell_only(pid: i32, depth: u8) -> bool {
    if depth > 8 || pid <= 0 {
        return true;
    }
    let comm = read_comm(pid).unwrap_or_default();
    let cmdline = read_cmdline(pid).unwrap_or_default();
    if classify_text(&format!("{comm} {cmdline}")).is_some() {
        return false;
    }
    for child in children_of(pid) {
        if !tree_is_shell_only(child, depth + 1) {
            return false;
        }
    }
    is_shell_name(comm.trim()) || children_of(pid).is_empty()
}

fn read_comm(pid: i32) -> Option<String> {
    let s = fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    Some(s.trim().to_string())
}

fn read_cmdline(pid: i32) -> Option<String> {
    let raw = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    if raw.is_empty() {
        return None;
    }
    let s = raw
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    Some(s)
}

fn children_of(pid: i32) -> Vec<i32> {
    // /proc/pid/task/*/children only — never scan all of /proc (too slow for refresh).
    let mut kids = Vec::new();
    let task_dir = PathBuf::from(format!("/proc/{pid}/task"));
    let Ok(entries) = fs::read_dir(task_dir) else {
        return kids;
    };
    for e in entries.flatten() {
        let children_path = e.path().join("children");
        if let Ok(s) = fs::read_to_string(children_path) {
            for p in s.split_whitespace() {
                if let Ok(c) = p.parse::<i32>() {
                    kids.push(c);
                }
            }
        }
    }
    kids.sort_unstable();
    kids.dedup();
    kids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_agent_names() {
        assert_eq!(
            classify_text("claude --dangerously-skip-permissions"),
            Some(AgentClass::Claude)
        );
        assert_eq!(classify_text("/bin/codex"), Some(AgentClass::Codex));
        assert_eq!(
            classify_text("codex-code-mode-host"),
            Some(AgentClass::Codex)
        );
        assert_eq!(classify_text("grok"), Some(AgentClass::Grok));
        assert_eq!(
            classify_text("/home/u/.local/bin/kilo"),
            Some(AgentClass::Kilo)
        );
        assert_eq!(
            classify_text("claude-desktop --type=gpu"),
            Some(AgentClass::Other)
        );
    }

    #[test]
    fn title_fallback() {
        let a = classify_session(&[], &[], "claude doing stuff");
        assert_eq!(a, AgentClass::Claude);
    }

    #[test]
    fn empty_is_unknown() {
        assert_eq!(classify_session(&[], &[], ""), AgentClass::Unknown);
    }
}
