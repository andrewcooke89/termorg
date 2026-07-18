//! CLI command handlers.

use std::io::{self, Read, Write};
use std::thread;
use std::time::Duration;

use super::args::{GroupCmd, HintsCmd, QueueCmd};
use crate::ambient;
use crate::error::Result;
use crate::filter;
use crate::hints;
use crate::notify;
use crate::provider::{self, LaunchKind, LaunchRequest, TerminalProvider};
use crate::queue;
use crate::signals::{self, SignalState};
use crate::store::{
    self, build_display_sections, load_and_rebind, DisplaySection, Priority, UserState,
};

pub(crate) fn cmd_hook(state: Option<&str>, reason: Option<&str>, list: bool) -> Result<()> {
    if list {
        let sigs = signals::list_signals();
        if sigs.is_empty() {
            println!("(no active agent signals)");
            println!("path: {}", signals::signals_path().display());
            return Ok(());
        }
        for s in sigs {
            println!(
                "{:<10}  kitty={}:{:?}  tmux={}  cwd={}  reason={}  src={}  sess={}",
                match s.state {
                    SignalState::NeedsYou => "needs_you",
                    SignalState::Working => "working",
                    SignalState::Idle => "idle",
                },
                s.kitty_pid.as_deref().unwrap_or("-"),
                s.kitty_window_id,
                s.tmux_pane.as_deref().unwrap_or("-"),
                s.cwd.as_deref().unwrap_or("-"),
                s.reason.as_deref().unwrap_or("-"),
                s.source.as_deref().unwrap_or("-"),
                s.agent_session_id.as_deref().unwrap_or("-"),
            );
        }
        println!("path: {}", signals::signals_path().display());
        return Ok(());
    }

    if let Some(st) = state {
        let st =
            SignalState::parse(st).ok_or_else(|| crate::error::TermorgError::ProviderCommand {
                message: format!("unknown state `{st}` (needs_you|working|idle)"),
            })?;
        let sig = signals::record_manual(st, reason.unwrap_or("manual"))?;
        eprintln!(
            "termorg: signal {:?} (kitty={:?}:{:?} cwd={:?})",
            sig.state, sig.kitty_pid, sig.kitty_window_id, sig.cwd
        );
        return Ok(());
    }

    let mut raw = String::new();
    io::stdin().read_to_string(&mut raw)?;
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(crate::error::TermorgError::ProviderCommand {
            message: "hook: empty stdin — pass Claude hook JSON or use --state".into(),
        });
    }
    let sig = signals::ingest_hook_json(raw)?;
    // FS11: immediate desktop alert when a hook says needs_you (works without panel).
    if let Some(ref s) = sig {
        notify::notify_from_signal(s);
    }
    // Codex Stop requires JSON on stdout when exit 0; others ignore it.
    // Always emit a minimal success object so multi-agent hooks stay happy.
    println!("{{}}");
    Ok(())
}

pub(crate) fn cmd_group(action: GroupCmd) -> Result<()> {
    let mut state = UserState::load()?;
    match action {
        GroupCmd::List => {
            if state.manual_groups.is_empty() {
                println!("(no manual groups)");
            } else {
                for g in state.ordered_groups() {
                    println!("{:<16}  {}", g.id, g.title);
                }
            }
            println!("state: {}", store::state_path().display());
        }
        GroupCmd::Create { title } => {
            let g = state.create_group(&title);
            state.save()?;
            println!("created {}  ({})", g.title, g.id);
        }
        GroupCmd::Rename {
            id_or_title,
            new_title,
        } => {
            state.rename_group(&id_or_title, &new_title)?;
            state.save()?;
            println!("renamed ok");
        }
        GroupCmd::Delete { id_or_title } => {
            state.delete_group(&id_or_title)?;
            state.save()?;
            println!("deleted group `{id_or_title}` — any tabs were unassigned (not closed)");
        }
    }
    Ok(())
}

pub(crate) fn cmd_assign(
    provider: &dyn TerminalProvider,
    session_id: &str,
    group: &str,
) -> Result<()> {
    let sessions = provider.list_sessions()?;
    let session = sessions
        .iter()
        .find(|s| s.id == session_id)
        .ok_or_else(|| crate::error::TermorgError::ProviderCommand {
            message: format!("no session `{session_id}` — run `termorg list`"),
        })?;
    let mut state = UserState::load()?;
    state.assign(session, group)?;
    state.save()?;
    println!("assigned {} → {}", session.id, group);
    Ok(())
}

pub(crate) fn cmd_unassign(provider: &dyn TerminalProvider, session_id: &str) -> Result<()> {
    let sessions = provider.list_sessions()?;
    let session = sessions
        .iter()
        .find(|s| s.id == session_id)
        .ok_or_else(|| crate::error::TermorgError::ProviderCommand {
            message: format!("no session `{session_id}` — run `termorg list`"),
        })?;
    let mut state = UserState::load()?;
    state.unassign(session);
    state.save()?;
    println!("unassigned {}", session.id);
    Ok(())
}

pub(crate) fn cmd_queue(provider: &dyn TerminalProvider, action: QueueCmd) -> Result<()> {
    let sessions = provider.list_sessions()?;
    let state = load_and_rebind(&sessions).unwrap_or_default();
    let q = queue::build_action_queue(&sessions, &state);

    match action {
        QueueCmd::List => {
            if q.is_empty() {
                println!("(action queue empty)");
                return Ok(());
            }
            println!(
                "◎ Action queue ({} item{})",
                q.len(),
                if q.len() == 1 { "" } else { "s" }
            );
            for (i, s) in q.iter().enumerate() {
                let pri = match state.priority_for(s) {
                    Priority::Important => "★",
                    Priority::Muted => "m",
                    Priority::Normal => " ",
                };
                println!(
                    "  {:>2}. {pri} {:<8} {:<10}  {:<16}  {}",
                    i + 1,
                    s.agent.label(),
                    s.attention.label(),
                    s.id,
                    s.title.replace('\n', " ")
                );
            }
            println!("hint: termorg next | termorg queue prev | termorg queue go N");
        }
        QueueCmd::Next => {
            let cur = read_queue_cursor();
            let target = queue::queue_next(&q, cur.as_deref()).ok_or_else(|| {
                crate::error::TermorgError::ProviderCommand {
                    message: "action queue is empty".into(),
                }
            })?;
            provider.focus(target)?;
            write_queue_cursor(&target.id);
            println!("focused next: {} ({})", target.id, target.attention.label());
        }
        QueueCmd::Prev => {
            let cur = read_queue_cursor();
            let target = queue::queue_prev(&q, cur.as_deref()).ok_or_else(|| {
                crate::error::TermorgError::ProviderCommand {
                    message: "action queue is empty".into(),
                }
            })?;
            provider.focus(target)?;
            write_queue_cursor(&target.id);
            println!("focused prev: {} ({})", target.id, target.attention.label());
        }
        QueueCmd::Go { index } => {
            if index == 0 || index > q.len() {
                return Err(crate::error::TermorgError::ProviderCommand {
                    message: format!("index {index} out of range 1..{}", q.len()),
                });
            }
            let target = &q[index - 1];
            provider.focus(target)?;
            write_queue_cursor(&target.id);
            println!("focused #{}: {}", index, target.id);
        }
    }
    Ok(())
}

fn queue_cursor_path() -> std::path::PathBuf {
    store::state_path().with_file_name("queue_cursor")
}

fn read_queue_cursor() -> Option<String> {
    std::fs::read_to_string(queue_cursor_path())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn write_queue_cursor(id: &str) {
    let path = queue_cursor_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, id);
}

pub(crate) fn cmd_priority(
    provider: &dyn TerminalProvider,
    session_id: &str,
    level: &str,
) -> Result<()> {
    let priority =
        Priority::parse(level).ok_or_else(|| crate::error::TermorgError::ProviderCommand {
            message: format!("unknown priority `{level}` — use important, normal, or muted"),
        })?;
    let sessions = provider.list_sessions()?;
    let session = sessions
        .iter()
        .find(|s| s.id == session_id)
        .ok_or_else(|| crate::error::TermorgError::ProviderCommand {
            message: format!("no session `{session_id}` — run `termorg list`"),
        })?;
    let mut state = UserState::load()?;
    state.set_priority(session, priority);
    state.save()?;
    println!("priority {} → {}", session.id, priority.as_str());
    Ok(())
}

pub(crate) fn cmd_focus(provider: &dyn TerminalProvider, id: &str) -> Result<()> {
    let sessions = provider.list_sessions()?;
    let Some(session) = sessions.iter().find(|s| s.id == id) else {
        let known: Vec<_> = sessions.iter().map(|s| s.id.as_str()).collect();
        return Err(crate::error::TermorgError::ProviderCommand {
            message: format!(
                "no session with id `{id}`.\nKnown ids:\n  {}",
                if known.is_empty() {
                    "(none — is Kitty remote control up?)".into()
                } else {
                    known.join("\n  ")
                }
            ),
        });
    };
    provider.focus(session)?;
    println!("focused {}", session.id);
    Ok(())
}

pub(crate) fn cmd_list(
    provider: &dyn TerminalProvider,
    json: bool,
    flat: bool,
    filter_q: Option<&str>,
    hide_idle_shells: Option<bool>,
) -> Result<()> {
    let sessions = provider.list_sessions()?;
    let state = match load_and_rebind(&sessions) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("termorg: warning: could not load state ({e}); using empty prefs");
            UserState::default()
        }
    };
    let hide = filter::hide_idle_shells_enabled(hide_idle_shells);
    let sessions = filter::apply_noise_filter(&sessions, hide);
    let sessions = match filter_q {
        Some(q) if !q.trim().is_empty() => filter::filter_sessions(&sessions, &state, q),
        _ => sessions,
    };

    if json {
        print_json(&sessions, &state);
        return Ok(());
    }

    if sessions.is_empty() && state.manual_groups.is_empty() {
        if filter_q.is_some_and(|q| !q.trim().is_empty()) {
            println!(
                "No sessions match filter (provider: {}).",
                provider.provider_id()
            );
        } else {
            println!("No sessions found (provider: {}).", provider.provider_id());
        }
        return Ok(());
    }

    if flat {
        print_flat(&sessions, &state);
    } else {
        print_sections(sessions, &state, filter_q);
    }

    Ok(())
}

pub(crate) fn print_flat(sessions: &[provider::ProviderSession], state: &UserState) {
    println!(
        "{:<8} {:<4} {:<8} {:<10} {:<16} {:<6} TITLE",
        "PROVIDER", "PRI", "AGENT", "ATTN", "ID", "FOCUS"
    );
    println!("{}", "-".repeat(92));
    for s in sessions {
        let focus = if s.is_focused { "*" } else { "" };
        let pri = match state.priority_for(s) {
            Priority::Important => "★",
            Priority::Muted => "m",
            Priority::Normal => "",
        };
        println!(
            "{:<8} {:<4} {:<8} {:<10} {:<16} {:<6} {}",
            s.provider,
            pri,
            s.agent.label(),
            s.attention.label(),
            s.id,
            focus,
            s.title.replace('\n', " ")
        );
    }
    println!();
    println!("{} session(s)", sessions.len());
}

pub(crate) fn print_sections(
    sessions: Vec<provider::ProviderSession>,
    state: &UserState,
    filter_q: Option<&str>,
) {
    let total = sessions.len();
    let sections = build_display_sections(sessions, state);
    let mut n_manual = 0usize;
    let mut n_auto = 0usize;

    for sec in &sections {
        match sec {
            DisplaySection::Manual { group, sessions } => {
                if sessions.is_empty() {
                    continue;
                }
                n_manual += 1;
                println!();
                println!(
                    "◆ {}  (manual · {} tab{})",
                    group.title,
                    sessions.len(),
                    if sessions.len() == 1 { "" } else { "s" }
                );
                for s in sessions {
                    let focus = if s.is_focused { "*" } else { " " };
                    let pri = pri_mark(state, s);
                    let path = s
                        .cwd
                        .as_deref()
                        .map(collapse_hint)
                        .unwrap_or_else(|| "—".into());
                    println!(
                        "  {focus}{pri} {:<8} {:<10}  {:<14}  {}  ·  {}",
                        s.agent.label(),
                        s.attention.label(),
                        s.id,
                        short_title(&s.title, &path),
                        path
                    );
                }
            }
            DisplaySection::Auto {
                title,
                path_hint,
                sessions,
            } => {
                if sessions.is_empty() {
                    continue;
                }
                n_auto += 1;
                println!();
                println!(
                    "▶ {}  ({} tab{})",
                    title,
                    sessions.len(),
                    if sessions.len() == 1 { "" } else { "s" }
                );
                if path_hint != title && !path_hint.is_empty() {
                    println!("  {}", collapse_hint(path_hint));
                }
                for s in sessions {
                    let focus = if s.is_focused { "*" } else { " " };
                    let pri = pri_mark(state, s);
                    let cwd = s.cwd.as_deref().unwrap_or("?");
                    println!(
                        "  {focus}{pri} {:<8} {:<10}  {:<14}  {}",
                        s.agent.label(),
                        s.attention.label(),
                        s.id,
                        short_title(&s.title, cwd)
                    );
                }
            }
        }
    }

    println!();
    if let Some(q) = filter_q.filter(|q| !q.trim().is_empty()) {
        println!("{total} match(es) for `{q}` · {n_manual} manual · {n_auto} path group(s)");
    } else {
        println!("{total} session(s) · {n_manual} manual group(s) · {n_auto} path group(s)");
    }
}

fn pri_mark(state: &UserState, s: &provider::ProviderSession) -> &'static str {
    match state.priority_for(s) {
        Priority::Important => "★",
        Priority::Muted => "·",
        Priority::Normal => " ",
    }
}

fn short_title(title: &str, cwd: &str) -> String {
    let t = title.replace('\n', " ").trim().to_string();
    if t.is_empty() || t == "?" {
        return collapse_hint(cwd);
    }
    t
}

fn collapse_hint(path: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    if !home.is_empty() && path.starts_with(&home) {
        format!("~{}", &path[home.len()..])
    } else {
        path.to_string()
    }
}

pub(crate) fn print_json(sessions: &[provider::ProviderSession], state: &UserState) {
    let raw = crate::list_json::sessions_to_json_string(sessions, state);
    println!("{raw}");
}

pub(crate) fn cmd_hints(provider: &dyn TerminalProvider, action: HintsCmd) -> Result<()> {
    let sessions = provider.list_sessions()?;
    let mut state = load_and_rebind(&sessions).unwrap_or_default();
    state.prune_stale_hints();

    match action {
        HintsCmd::List => {
            let sug = hints::suggestions(&sessions, &state);
            if sug.is_empty() {
                println!("(no path→group suggestions)");
                if state.path_hints.is_empty() {
                    println!("hint: assign a tab to a manual group to teach path associations");
                } else {
                    println!(
                        "learned {} path association(s); none match unassigned tabs right now",
                        state.path_hints.len()
                    );
                }
                return Ok(());
            }
            println!("Path suggestions (FS15):");
            for (i, s) in sug.iter().enumerate() {
                println!("  {:>2}. {}", i + 1, hints::suggestion_label(s));
            }
            println!("accept: termorg hints accept <session_id>");
            println!("dismiss: termorg hints dismiss <session_id|path_key>");
        }
        HintsCmd::Accept { session_id } => {
            let session = sessions
                .iter()
                .find(|s| s.id == session_id)
                .ok_or_else(|| crate::error::TermorgError::ProviderCommand {
                    message: format!("no session `{session_id}`"),
                })?;
            let sug = hints::suggestions(std::slice::from_ref(session), &state);
            let s = sug
                .first()
                .ok_or_else(|| crate::error::TermorgError::ProviderCommand {
                    message: format!("no suggestion for `{session_id}`"),
                })?;
            state.assign(session, &s.group_id)?;
            state.save()?;
            println!("accepted: {} → ◆ {}", session.id, s.group_title);
        }
        HintsCmd::Dismiss { path_or_session } => {
            let key = if let Some(s) = sessions.iter().find(|s| s.id == path_or_session) {
                hints::path_key_for_session(s)
                    .map(|(k, _)| k)
                    .ok_or_else(|| crate::error::TermorgError::ProviderCommand {
                        message: "session has no path key".into(),
                    })?
            } else {
                path_or_session.clone()
            };
            state.dismiss_path_hint(&key);
            state.save()?;
            println!("dismissed suggestions for path `{key}`");
        }
        HintsCmd::Rebuild => {
            hints::rebuild_hints_from_prefs(&mut state, &sessions);
            state.prune_stale_hints();
            state.save()?;
            println!(
                "rebuilt {} path hint(s) from existing assignments",
                state.path_hints.len()
            );
            for h in &state.path_hints {
                let gtitle = state
                    .manual_groups
                    .iter()
                    .find(|g| g.id == h.group_id)
                    .map(|g| g.title.as_str())
                    .unwrap_or("?");
                println!("  {} → {} ({}×)", h.path_key, gtitle, h.hits);
            }
        }
    }
    Ok(())
}

pub(crate) fn cmd_launch(
    provider: &dyn TerminalProvider,
    agent: &str,
    cwd: Option<&str>,
    group: Option<&str>,
    endpoint: Option<&str>,
    title: Option<&str>,
) -> Result<()> {
    let kind =
        LaunchKind::parse(agent).ok_or_else(|| crate::error::TermorgError::ProviderCommand {
            message: format!("unknown agent `{agent}` (shell|claude|grok|kilo|codex)"),
        })?;
    let cwd = cwd
        .map(|s| s.to_string())
        .or_else(|| std::env::var("PWD").ok());
    let endpoint = endpoint
        .map(|s| s.to_string())
        .or_else(|| provider.prefer_launch_endpoint(cwd.as_deref()));
    let req = LaunchRequest {
        kind,
        cwd: cwd.clone(),
        endpoint,
        tab_title: title.map(|s| s.to_string()),
    };
    let result = provider.launch(&req)?;
    println!(
        "launched {}  win={:?}  native={:?}  cwd={}  via {}",
        kind.as_str(),
        result.window_id,
        result.native_id,
        result.cwd.as_deref().unwrap_or("—"),
        result.endpoint
    );

    if let Some(g) = group {
        // Retry exact native-id rematch; never assign via cwd guess.
        let mut found = None;
        for attempt in 0..8 {
            if attempt > 0 {
                thread::sleep(Duration::from_millis(150));
            } else {
                thread::sleep(Duration::from_millis(350));
            }
            if let Ok(sessions) = provider.list_sessions() {
                if let Some(session) = find_launched_session(&sessions, &result) {
                    found = Some(session.clone());
                    break;
                }
            }
        }
        if let Some(session) = found {
            let mut state = UserState::load()?;
            state.assign(&session, g)?;
            state.save()?;
            println!("assigned {} → {g}", session.id);
        } else {
            eprintln!(
                "termorg: launched but exact session rematch pending (native={:?}) — assign manually",
                result.native_id
            );
        }
    }
    Ok(())
}

fn find_launched_session<'a>(
    sessions: &'a [provider::ProviderSession],
    result: &provider::LaunchResult,
) -> Option<&'a provider::ProviderSession> {
    // Exact provider/endpoint/native identity only — never cwd fallback.
    let mut hits: Vec<_> = sessions
        .iter()
        .filter(|s| provider::session_matches_launch(s, result))
        .collect();
    if hits.len() == 1 {
        return Some(hits.remove(0));
    }
    // If multiple (rare), prefer matching launch kind agent class.
    hits.sort_by_key(|s| {
        let agent_ok = match result.kind {
            LaunchKind::Shell => s.agent.as_str() == "shell" || s.agent.as_str() == "unknown",
            LaunchKind::Claude => s.agent.as_str() == "claude",
            LaunchKind::Grok => s.agent.as_str() == "grok",
            LaunchKind::Kilo => s.agent.as_str() == "kilo",
            LaunchKind::Codex => s.agent.as_str() == "codex",
        };
        (!agent_ok, s.id.as_str())
    });
    hits.into_iter().next()
}

pub(crate) fn cmd_watch(
    provider: &dyn TerminalProvider,
    interval_secs: u64,
    do_notify: bool,
) -> Result<()> {
    let interval = Duration::from_secs(interval_secs.max(1));
    let mut previous: Vec<String> = Vec::new();
    let mut notifier = if do_notify {
        notify::ensure_default_config();
        Some(notify::NotifyTracker::new())
    } else {
        None
    };
    ambient::ensure_default_config();
    let mut ambient_applier = ambient::AmbientApplier::new();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    writeln!(
        out,
        "termorg watch — provider={} interval={}s notify={} ambient=on (Ctrl-C to stop)",
        provider.provider_id(),
        interval.as_secs(),
        if do_notify { "on" } else { "off" }
    )?;

    loop {
        match provider.list_sessions() {
            Ok(sessions) => {
                let state = load_and_rebind(&sessions).unwrap_or_default();
                if let Some(ref mut n) = notifier {
                    n.process(&sessions, &state);
                }
                // FS12: paint Kitty tab bar from agent/attention.
                ambient_applier.apply_all(provider, &sessions);
                let ids: Vec<String> = sessions.iter().map(|s| s.id.clone()).collect();
                writeln!(out, "\n--- {} ---", chrono_like_now())?;
                let sections = build_display_sections(sessions, &state);
                for sec in sections {
                    match sec {
                        DisplaySection::Manual { group, sessions } => {
                            writeln!(out, "◆ {}", group.title)?;
                            for s in sessions {
                                let mark = if s.is_focused { "*" } else { " " };
                                writeln!(
                                    out,
                                    "  {mark} [{:}] [{:}] {}  {}",
                                    s.agent.label(),
                                    s.attention.label(),
                                    s.id,
                                    s.title.replace('\n', " ")
                                )?;
                            }
                        }
                        DisplaySection::Auto {
                            title, sessions, ..
                        } => {
                            writeln!(out, "▶ {}", title)?;
                            for s in sessions {
                                let mark = if s.is_focused { "*" } else { " " };
                                writeln!(
                                    out,
                                    "  {mark} [{:}] [{:}] {}  {}",
                                    s.agent.label(),
                                    s.attention.label(),
                                    s.id,
                                    s.title.replace('\n', " ")
                                )?;
                            }
                        }
                    }
                }
                for id in ids.iter().filter(|id| !previous.contains(id)) {
                    writeln!(out, "  + opened {id}")?;
                }
                for id in previous.iter().filter(|id| !ids.contains(id)) {
                    writeln!(out, "  - closed {id}")?;
                }
                previous = ids;
                out.flush()?;
            }
            Err(e) => {
                writeln!(out, "termorg: {e}")?;
                out.flush()?;
            }
        }
        thread::sleep(interval);
    }
}

fn chrono_like_now() -> String {
    use std::time::SystemTime;
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => format!("unix+{}s", d.as_secs()),
        Err(_) => "now".into(),
    }
}
