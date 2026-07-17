//! FS7 — attention state: needs_you / working / idle / unknown.
//!
//! **Primary (agents):** lifecycle hooks write signals (`termorg hook`) when the
//! agent needs input (Notification/Stop) or is busy (PreToolUse/…). CPU is a
//! poor proxy for “waiting on network”.
//!
//! **Secondary:** title phrases, live tool children (`sleep`, `cargo`, …), and
//! short CPU/IO hot-hold — used when hooks are missing or tools are in flight.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use crate::agent::AgentClass;
use crate::signals::{self, MatchHint, SignalState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Attention {
    NeedsYou,
    Working,
    Idle,
    Unknown,
}

impl Attention {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NeedsYou => "needs_you",
            Self::Working => "working",
            Self::Idle => "idle",
            Self::Unknown => "unknown",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NeedsYou => "needs you",
            Self::Working => "working",
            Self::Idle => "idle",
            Self::Unknown => "?",
        }
    }

    pub fn rgb(self) -> (u8, u8, u8) {
        match self {
            Self::NeedsYou => (247, 118, 142),
            Self::Working => (224, 175, 104),
            Self::Idle => (86, 95, 137),
            Self::Unknown => (65, 72, 104),
        }
    }

    pub fn is_agent(agent: AgentClass) -> bool {
        matches!(
            agent,
            AgentClass::Claude | AgentClass::Grok | AgentClass::Kilo | AgentClass::Codex
        )
    }
}

impl std::fmt::Display for Attention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

struct ActSample {
    /// Sum of utime+stime (jiffies) over the process tree.
    jiffies: u64,
    /// Sum of read_bytes+write_bytes from /proc/pid/io (tree).
    io_bytes: u64,
    /// Wall time of sample.
    at: Instant,
    /// Sticky "recently active" until this instant.
    hot_until: Instant,
}

static ACT_SAMPLES: Mutex<Option<HashMap<String, ActSample>>> = Mutex::new(None);

/// Minimum jiffy increase to count as CPU activity (across ~1s refresh).
const MIN_JIFFY_DELTA: u64 = 3;
/// Minimum IO byte increase — high enough to ignore keepalive noise.
const MIN_IO_DELTA: u64 = 64 * 1024;
/// How long to keep showing "working" after last observed activity bump.
/// Kept short so quiet agents flip to needs_you quickly; tool children cover
/// long sleeps that burn no CPU.
const HOT_HOLD: std::time::Duration = std::time::Duration::from_secs(2);

/// Classify attention.
///
/// `activity_key` should be stable per session (e.g. session id) for CPU deltas.
/// `hint` matches agent hook signals (Kitty window / cwd).
pub fn classify(
    activity_key: &str,
    agent: AgentClass,
    at_prompt: Option<bool>,
    title: &str,
    pids: &[i32],
    hint: MatchHint<'_>,
) -> Attention {
    let title_l = title.to_lowercase();
    let act_hot = update_activity_hot(activity_key, pids);
    let run_state_hot = pids_in_run_state(pids);
    let tool_work = tree_has_tool_work(pids);
    let live_work = act_hot || run_state_hot || tool_work; // shells only
    let hook = signals::lookup(hint);

    // 1) Title phrases (tight).
    if title_suggests_needs_you(&title_l) {
        return Attention::NeedsYou;
    }
    if title_suggests_working(&title_l) {
        return Attention::Working;
    }

    // 2) Agents — hook signals are primary (Notification/Stop → needs_you;
    //    PreToolUse/UserPromptSubmit → working).
    //
    //    Stop/Notification always win: Claude keeps long-lived MCP children
    //    (node/python servers) that look like "tools" but are not user work.
    //    Only shell-spawned tools (bash → sleep/cargo/…) count as live_work.
    if Attention::is_agent(agent) {
        if let Some(sig) = &hook {
            match sig.state {
                SignalState::NeedsYou => return Attention::NeedsYou,
                SignalState::Working => {
                    // Still working if a real shell tool is in flight; else trust
                    // the sticky Working signal until Stop/Notification.
                    return Attention::Working;
                }
                SignalState::Idle if tool_work => return Attention::Working,
                SignalState::Idle => return Attention::Idle,
            }
        }
        if tool_work || run_state_hot || act_hot {
            return Attention::Working;
        }
        // No hook signal: quiet agent → needs you (legacy / hooks not installed).
        return Attention::NeedsYou;
    }

    // 3) Shells / other — ignore agent hooks (cwd collisions); process + prompt.
    match at_prompt {
        Some(true) => Attention::Idle,
        Some(false) | None => {
            if live_work {
                Attention::Working
            } else {
                Attention::Idle
            }
        }
    }
}

fn title_suggests_needs_you(title_l: &str) -> bool {
    const PHRASES: &[&str] = &[
        "permission",
        "waiting for your",
        "waiting for input",
        "awaiting",
        "approve",
        "allow this",
        "do you want",
        "y/n",
        "[y/n]",
        "(y/n)",
        "press enter",
        "needs your",
        "human:",
        "your turn",
        "confirm",
        "authorization",
    ];
    PHRASES.iter().any(|p| title_l.contains(p))
}

fn title_suggests_working(title_l: &str) -> bool {
    const PHRASES: &[&str] = &[
        "thinking",
        "generating",
        "running tool",
        "running…",
        "running...",
        "compiling",
        "streaming",
        "processing",
        "working",
        "⠋",
        "⠙",
        "⠹",
        "⠸",
        "⠼",
        "⠴",
        "⠦",
        "⠧",
        "⠇",
        "⠏",
    ];
    PHRASES.iter().any(|p| title_l.contains(p))
}

/// Update activity samples for this session key; return true if recently active.
fn update_activity_hot(key: &str, pids: &[i32]) -> bool {
    let now = Instant::now();
    let jiffies = sum_tree_jiffies(pids);
    let io_bytes = sum_tree_io(pids);

    let mut guard = match ACT_SAMPLES.lock() {
        Ok(g) => g,
        Err(_) => return false,
    };
    let map = guard.get_or_insert_with(HashMap::new);

    let prev = map.get(key);
    let grew_cpu = prev
        .map(|p| jiffies.saturating_sub(p.jiffies) >= MIN_JIFFY_DELTA)
        .unwrap_or(false);
    let grew_io = prev
        .map(|p| io_bytes.saturating_sub(p.io_bytes) >= MIN_IO_DELTA)
        .unwrap_or(false);
    let grew = grew_cpu || grew_io;
    let still_held = prev.map(|p| now < p.hot_until).unwrap_or(false);
    let is_hot = grew || still_held;
    let new_hot_until = if grew {
        now + HOT_HOLD
    } else if still_held {
        prev.map(|p| p.hot_until).unwrap_or(now)
    } else {
        now
    };

    map.insert(
        key.to_string(),
        ActSample {
            jiffies,
            io_bytes,
            at: now,
            hot_until: new_hot_until,
        },
    );

    if map.len() > 512 {
        map.retain(|_, s| now.duration_since(s.at) < std::time::Duration::from_secs(600));
    }

    is_hot
}

fn sum_tree_jiffies(pids: &[i32]) -> u64 {
    let mut total = 0u64;
    let mut seen = std::collections::HashSet::new();
    for &pid in pids {
        total = total.saturating_add(tree_jiffies(pid, 0, &mut seen));
    }
    total
}

fn sum_tree_io(pids: &[i32]) -> u64 {
    let mut total = 0u64;
    let mut seen = std::collections::HashSet::new();
    for &pid in pids {
        total = total.saturating_add(tree_io(pid, 0, &mut seen));
    }
    total
}

fn tree_jiffies(pid: i32, depth: u8, seen: &mut std::collections::HashSet<i32>) -> u64 {
    if depth > 12 || pid <= 0 || !seen.insert(pid) {
        return 0;
    }
    let mut t = cpu_jiffies(pid).unwrap_or(0);
    if let Ok(s) = std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children")) {
        for c in s.split_whitespace() {
            if let Ok(cp) = c.parse::<i32>() {
                t = t.saturating_add(tree_jiffies(cp, depth + 1, seen));
            }
        }
    }
    t
}

fn tree_io(pid: i32, depth: u8, seen: &mut std::collections::HashSet<i32>) -> u64 {
    if depth > 12 || pid <= 0 || !seen.insert(pid) {
        return 0;
    }
    let mut t = io_bytes(pid).unwrap_or(0);
    if let Ok(s) = std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children")) {
        for c in s.split_whitespace() {
            if let Ok(cp) = c.parse::<i32>() {
                t = t.saturating_add(tree_io(cp, depth + 1, seen));
            }
        }
    }
    t
}

fn io_bytes(pid: i32) -> Option<u64> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/io")).ok()?;
    let mut read_b = 0u64;
    let mut write_b = 0u64;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("read_bytes:") {
            read_b = v.trim().parse().unwrap_or(0);
        }
        if let Some(v) = line.strip_prefix("write_bytes:") {
            write_b = v.trim().parse().unwrap_or(0);
        }
    }
    Some(read_b.saturating_add(write_b))
}

/// utime + stime from /proc/pid/stat (fields 14 and 15 after comm).
fn cpu_jiffies(pid: i32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rparen = stat.rfind(')')?;
    let rest = stat.get(rparen + 2..)?; // skip ") "
    let mut fields = rest.split_whitespace();
    let _state = fields.next()?;
    // fields after state: ppid(2) ... utime is 14th overall after pid/comm,
    // which is index 11 in the post-state split (state=1 → utime = field 12 of rest? )
    // Linux stat: after ") " : state ppid pgrp session tty_nr tpgid flags minflt cminflt
    // majflt cmajflt utime stime
    // indices 0=state ... 11=utime 12=stime
    let mut utime = None;
    let mut stime = None;
    for (i, f) in fields.enumerate() {
        if i == 11 {
            utime = f.parse::<u64>().ok();
        }
        if i == 12 {
            stime = f.parse::<u64>().ok();
            break;
        }
    }
    Some(utime.unwrap_or(0).saturating_add(stime.unwrap_or(0)))
}

fn pids_in_run_state(pids: &[i32]) -> bool {
    for &pid in pids {
        if pid <= 0 {
            continue;
        }
        if tree_has_run_state(pid, 0) {
            return true;
        }
    }
    false
}

fn tree_has_run_state(pid: i32, depth: u8) -> bool {
    if depth > 10 || pid <= 0 {
        return false;
    }
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    // Skip shells, agent runtimes, and MCP/helpers — they sit in R often.
    // Only R/D under a real shell tool path matters (handled via tool_work);
    // here we only flag busy non-infra leaves (rare).
    if !is_shellish(&comm) && !is_agent_infra(pid, &comm) {
        if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            if let Some(state) = parse_stat_state(&stat) {
                if matches!(state, 'R' | 'D') {
                    // Only count if this process is under a shell (actual tool).
                    if process_has_shell_ancestor(pid) {
                        return true;
                    }
                }
            }
        }
    }
    if let Ok(s) = std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children")) {
        for c in s.split_whitespace() {
            if let Ok(cp) = c.parse::<i32>() {
                if tree_has_run_state(cp, depth + 1) {
                    return true;
                }
            }
        }
    }
    false
}

/// True if a real tool process is still alive (`sleep`, `cargo`, `git`, …).
///
/// Long-lived MCP / agent helpers are filtered via [`is_agent_infra`]. Bash may
/// `exec` simple commands (`bash -c 'sleep 3'` → process becomes `sleep`), so we
/// must detect the tool itself, not only shell→child relationships.
fn tree_has_tool_work(pids: &[i32]) -> bool {
    let mut seen = std::collections::HashSet::new();
    for &pid in pids {
        if pid <= 0 {
            continue;
        }
        if tree_has_tool_work_at(pid, 0, &mut seen) {
            return true;
        }
    }
    false
}

fn tree_has_tool_work_at(
    pid: i32,
    depth: u8,
    seen: &mut std::collections::HashSet<i32>,
) -> bool {
    if depth > 12 || pid <= 0 || !seen.insert(pid) {
        return false;
    }
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    // Shells + agent + MCP scaffolding are not tools.
    if !is_shellish(&comm) && !is_agent_infra(pid, &comm) {
        if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            if let Some(state) = parse_stat_state(&stat) {
                if state != 'Z' {
                    return true;
                }
            }
        }
    }

    if let Ok(s) = std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children")) {
        for c in s.split_whitespace() {
            if let Ok(cp) = c.parse::<i32>() {
                if tree_has_tool_work_at(cp, depth + 1, seen) {
                    return true;
                }
            }
        }
    }
    false
}

fn process_has_shell_ancestor(pid: i32) -> bool {
    let mut cur = pid;
    for _ in 0..16 {
        let ppid = parent_pid(cur).unwrap_or(0);
        if ppid <= 1 {
            return false;
        }
        let comm = std::fs::read_to_string(format!("/proc/{ppid}/comm"))
            .ok()
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if is_shellish(&comm) {
            return true;
        }
        if is_agent_runtime(&comm) {
            return false;
        }
        cur = ppid;
    }
    false
}

fn parent_pid(pid: i32) -> Option<i32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rparen = stat.rfind(')')?;
    let rest = stat.get(rparen + 2..)?;
    let mut fields = rest.split_whitespace();
    let _state = fields.next()?;
    fields.next()?.parse().ok()
}

fn is_shellish(comm: &str) -> bool {
    matches!(
        comm.trim(),
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
            | "kitty"
            | "kitten"
    )
}

/// Agent CLI / runtime process names — not "tool work" by themselves.
/// Idle Claude is often a single `node`/`claude` in S; those must not stick
/// working forever. Real tools (`sleep`, `cargo`, `rg`, …) are not listed here.
fn is_agent_runtime(comm: &str) -> bool {
    matches!(
        comm.trim(),
        "claude"
            | "node"
            | "nodejs"
            | "codex"
            | "kilo"
            | "grok"
            | "npm"
            | "npx"
            | "bun"
            | "deno"
    )
}

/// Long-lived agent helpers (MCP servers, dispatchers) that must not count as
/// shell tools. `/proc/pid/comm` is truncated to 15 chars.
fn is_agent_infra(pid: i32, comm: &str) -> bool {
    if is_agent_runtime(comm) {
        return true;
    }
    let c = comm.trim();
    // comm is max 15 chars: "harness-dispat", "brave-search-m", …
    if c.starts_with("harness")
        || c.contains("mcp")
        || c == "uv"
        || c == "uvx"
        || c.starts_with("brave-search")
        || c.starts_with("google-drive")
    {
        return true;
    }
    // python/python3 MCP servers — check cmdline
    if matches!(c, "python" | "python3" | "MainThread") {
        if let Some(cmd) = cmdline_of(pid) {
            let l = cmd.to_lowercase();
            if l.contains("mcp")
                || l.contains("agent_co")
                || l.contains("agent-comm")
                || l.contains("fast-file")
                || l.contains("gemini")
                || l.contains("ticket")
                || l.contains("harness")
            {
                return true;
            }
        }
    }
    false
}

fn cmdline_of(pid: i32) -> Option<String> {
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    if raw.is_empty() {
        return None;
    }
    Some(
        raw.split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn parse_stat_state(stat: &str) -> Option<char> {
    let rparen = stat.rfind(')')?;
    let rest = stat.get(rparen + 1..)?.trim_start();
    rest.chars().next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::MatchHint;

    fn c(
        key: &str,
        agent: AgentClass,
        at_prompt: Option<bool>,
        title: &str,
        pids: &[i32],
    ) -> Attention {
        classify(key, agent, at_prompt, title, pids, MatchHint::default())
    }

    #[test]
    fn agent_at_prompt_needs_you() {
        assert_eq!(
            c("t1", AgentClass::Claude, Some(true), "claude", &[]),
            Attention::NeedsYou
        );
    }

    #[test]
    fn shell_at_prompt_idle() {
        assert_eq!(
            c("t2", AgentClass::Shell, Some(true), "zsh", &[]),
            Attention::Idle
        );
    }

    #[test]
    fn shell_quiet_idle() {
        assert_eq!(
            c("t3", AgentClass::Shell, Some(false), "zsh", &[]),
            Attention::Idle
        );
    }

    #[test]
    fn quiet_agent_needs_you_even_if_not_at_prompt() {
        // Idle Claude often has at_prompt=false; still needs you, not working.
        assert_eq!(
            c("t4", AgentClass::Claude, Some(false), "claude", &[]),
            Attention::NeedsYou
        );
        assert_eq!(
            c("t4b", AgentClass::Claude, None, "claude", &[]),
            Attention::NeedsYou
        );
    }

    #[test]
    fn title_permission_needs_you() {
        assert_eq!(
            c(
                "t5",
                AgentClass::Claude,
                None,
                "Claude: permission required",
                &[]
            ),
            Attention::NeedsYou
        );
    }

    #[test]
    fn title_thinking_working() {
        assert_eq!(
            c("t6", AgentClass::Claude, None, "Claude · Thinking…", &[]),
            Attention::Working
        );
    }

    #[test]
    fn sleeping_tool_child_keeps_agent_working() {
        // bash -c 'sleep N' often execs sleep (no shell left). Still a real tool.
        let mut child = std::process::Command::new("bash")
            .args(["-c", "sleep 3"])
            .spawn()
            .expect("spawn sleep");
        let pid = child.id() as i32;
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(
            tree_has_tool_work(&[pid]),
            "sleep must count as tool work (even if bash exec'd it)"
        );
        assert_eq!(
            c(
                "t-sleep-tool",
                AgentClass::Claude,
                Some(false),
                "claude",
                &[pid]
            ),
            Attention::Working
        );
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn no_pids_agent_is_needs_you() {
        assert_eq!(
            c("t-nopid", AgentClass::Claude, Some(false), "claude", &[]),
            Attention::NeedsYou
        );
    }
}
