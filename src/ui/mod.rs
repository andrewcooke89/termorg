//! Ops panel (eframe/egui).

mod rows;
mod theme;

use rows::RowAction;

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use eframe::egui::{self, Color32, RichText};
use eframe::epaint::CornerRadius;

use crate::filter::{self, session_matches};
use crate::hints::{self, PathHintSuggestion};
use crate::notify::NotifyTracker;
use crate::provider::{
    session_matches_native_id, LaunchKind, LaunchRequest, MultiProvider, ProviderKind,
    ProviderSession, TerminalProvider,
};
use crate::queue::build_action_queue;
use crate::store::{
    build_display_sections, load_and_rebind, DisplaySection, ManualGroup, Priority, UserState,
};
use theme::{self as th};

type FocusNote = Arc<Mutex<Option<String>>>;

const REFRESH: Duration = Duration::from_millis(1000);

/// Live sessions + prefs; views are rebuilt with FS10 filter on the UI thread.
type Snapshot = std::result::Result<(Vec<ProviderSession>, UserState), String>;

pub fn run_panel(
    provider: MultiProvider,
    kind: ProviderKind,
    kitty_to: Option<String>,
) -> Result<(), String> {
    let sock = panel_socket_path();
    if let Some(parent) = sock.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    if try_send_cmd(&sock, "show") {
        return Ok(());
    }

    let _ = std::fs::remove_file(&sock);
    let listener = std::os::unix::net::UnixListener::bind(&sock)
        .map_err(|e| format!("could not bind panel socket {}: {e}", sock.display()))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;

    let show_flag = Arc::new(AtomicBool::new(false));
    let quit_flag = Arc::new(AtomicBool::new(false));
    {
        let show_flag = Arc::clone(&show_flag);
        let quit_flag = Arc::clone(&quit_flag);
        thread::spawn(move || loop {
            if quit_flag.load(Ordering::Relaxed) {
                break;
            }
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buf = [0u8; 64];
                    if let Ok(n) = stream.read(&mut buf) {
                        let msg = std::str::from_utf8(&buf[..n]).unwrap_or("").trim();
                        match msg {
                            "show" | "toggle" | "TOGGLE" => {
                                show_flag.store(true, Ordering::Relaxed);
                            }
                            "quit" | "QUIT" => {
                                quit_flag.store(true, Ordering::Relaxed);
                            }
                            _ => {}
                        }
                        let _ = stream.write_all(b"ok\n");
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(100));
                }
                Err(_) => thread::sleep(Duration::from_millis(200)),
            }
        });
    }

    let snapshot: Arc<Mutex<Option<Snapshot>>> = Arc::new(Mutex::new(None));
    let refresh_quit = Arc::clone(&quit_flag);
    {
        let snapshot = Arc::clone(&snapshot);
        thread::spawn(move || {
            crate::notify::ensure_default_config();
            crate::ambient::ensure_default_config();
            let mut notifier = NotifyTracker::new();
            let mut ambient = crate::ambient::AmbientApplier::new();
            while !refresh_quit.load(Ordering::Relaxed) {
                let result = match provider.list_sessions() {
                    Ok(sessions) => {
                        let state = load_and_rebind(&sessions).unwrap_or_default();
                        // FS11: rising-edge needs_you → desktop notify (skip focused/muted).
                        notifier.process(&sessions, &state);
                        // FS12: tab/window color/title from agent + attention.
                        ambient.apply_all(&provider, &sessions);
                        Ok((sessions, state))
                    }
                    Err(e) => Err(e.to_string()),
                };
                if let Ok(mut slot) = snapshot.lock() {
                    *slot = Some(result);
                }
                thread::sleep(REFRESH);
            }
        });
    }

    let focus_note: FocusNote = Arc::new(Mutex::new(None));
    let state = PanelState::new(snapshot, show_flag, quit_flag, focus_note, kind, kitty_to);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Terminal Organiser")
            .with_inner_size([520.0, 700.0])
            .with_min_inner_size([360.0, 360.0])
            .with_app_id("termorg"),
        ..Default::default()
    };

    let sock_cleanup = sock.clone();
    eframe::run_native(
        "Terminal Organiser",
        options,
        Box::new(move |cc| {
            theme::apply_theme(&cc.egui_ctx);
            Ok(Box::new(state))
        }),
    )
    .map_err(|e| e.to_string())?;

    let _ = std::fs::remove_file(&sock_cleanup);
    Ok(())
}

fn panel_socket_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("termorg").join("panel.sock")
}

fn try_send_cmd(sock: &PathBuf, cmd: &str) -> bool {
    match std::os::unix::net::UnixStream::connect(sock) {
        Ok(mut stream) => {
            let _ = stream.set_read_timeout(Some(Duration::from_millis(300)));
            let _ = stream.set_write_timeout(Some(Duration::from_millis(300)));
            let _ = stream.write_all(format!("{cmd}\n").as_bytes());
            let mut resp = String::new();
            let _ = stream.read_to_string(&mut resp);
            true
        }
        Err(_) => false,
    }
}

struct PanelState {
    snapshot: Arc<Mutex<Option<Snapshot>>>,
    /// Unfiltered live sessions (from last refresh).
    all_sessions: Vec<ProviderSession>,
    user_state: UserState,
    sections: Vec<DisplaySection>,
    action_queue: Vec<ProviderSession>,
    error: Option<String>,
    session_count: usize,
    show_flag: Arc<AtomicBool>,
    quit_flag: Arc<AtomicBool>,
    focus_note: FocusNote,
    status: String,
    selected: Option<(usize, usize)>, // section, session index
    /// Selected index in action queue (for n/p navigation).
    queue_sel: Option<usize>,
    new_group_name: String,
    /// Cached group list for menus (refreshed with snapshot).
    manual_groups: Vec<ManualGroup>,
    /// FS10 free-text filter (title / path / agent / …).
    filter_query: String,
    /// Request focus on the filter field (e.g. after pressing `/`).
    focus_filter: bool,
    /// FS13 launch: working directory for new tabs.
    launch_cwd: String,
    /// FS13 optional manual group title/id for new tabs.
    launch_group: String,
    /// FS15 path→group suggestions for unassigned tabs.
    path_suggestions: Vec<PathHintSuggestion>,
    /// Backend selection for focus/launch (rebuild MultiProvider on demand).
    provider_kind: ProviderKind,
    kitty_to: Option<String>,
    /// Expand groups / launch tools in the top bar.
    show_tools: bool,
}

impl PanelState {
    fn new(
        snapshot: Arc<Mutex<Option<Snapshot>>>,
        show_flag: Arc<AtomicBool>,
        quit_flag: Arc<AtomicBool>,
        focus_note: FocusNote,
        provider_kind: ProviderKind,
        kitty_to: Option<String>,
    ) -> Self {
        let mut s = Self {
            snapshot,
            all_sessions: Vec::new(),
            user_state: UserState::default(),
            sections: Vec::new(),
            action_queue: Vec::new(),
            error: None,
            session_count: 0,
            show_flag,
            quit_flag,
            focus_note,
            status: "starting…".into(),
            selected: None,
            queue_sel: None,
            new_group_name: String::new(),
            manual_groups: Vec::new(),
            filter_query: String::new(),
            focus_filter: false,
            launch_cwd: std::env::var("PWD").unwrap_or_default(),
            launch_group: String::new(),
            path_suggestions: Vec::new(),
            provider_kind,
            kitty_to,
            show_tools: false,
        };
        s.apply_snapshot();
        s
    }

    fn pulse(&self, ctx: &egui::Context) -> f32 {
        // 0..1 gentle triangle for needs_you emphasis
        let t = ctx.input(|i| i.time) as f32;
        (t * std::f32::consts::TAU * 0.7).sin().abs()
    }

    fn provider_counts(&self) -> (usize, usize, usize) {
        let mut kitty = 0usize;
        let mut tmux = 0usize;
        let mut other = 0usize;
        for s in &self.all_sessions {
            match s.provider.as_str() {
                "kitty" => kitty += 1,
                "tmux" => tmux += 1,
                _ => other += 1,
            }
        }
        (kitty, tmux, other)
    }

    fn accept_hint(&mut self, session_id: &str) {
        let Some(sug) = self
            .path_suggestions
            .iter()
            .find(|s| s.session_id == session_id)
            .cloned()
        else {
            self.status = "suggestion gone — refresh".into();
            return;
        };
        let Some(session) = self
            .all_sessions
            .iter()
            .find(|s| s.id == session_id)
            .cloned()
        else {
            self.status = format!("session `{session_id}` not found");
            return;
        };
        match UserState::load() {
            Ok(mut st) => {
                if let Err(e) = st.assign(&session, &sug.group_id) {
                    self.status = format!("{e}");
                    return;
                }
                if let Err(e) = st.save() {
                    self.status = format!("save failed: {e}");
                    return;
                }
                self.user_state = st;
                self.status = format!("accepted: {} → ◆ {}", session.id, sug.group_title);
                self.rebuild_views();
            }
            Err(e) => self.status = format!("load state: {e}"),
        }
    }

    fn dismiss_hint(&mut self, path_key: &str) {
        match UserState::load() {
            Ok(mut st) => {
                st.dismiss_path_hint(path_key);
                if let Err(e) = st.save() {
                    self.status = format!("save failed: {e}");
                    return;
                }
                self.user_state = st;
                self.status = format!("dismissed path hint for `{path_key}`");
                self.rebuild_views();
            }
            Err(e) => self.status = format!("load state: {e}"),
        }
    }

    fn launch_kind(&mut self, kind: LaunchKind) {
        let cwd = self.launch_cwd.trim();
        let cwd = if cwd.is_empty() {
            // Prefer selected session cwd.
            self.selected_session()
                .and_then(|s| s.cwd.clone())
                .or_else(|| std::env::var("PWD").ok())
        } else {
            Some(cwd.to_string())
        };
        let provider_kind = self.provider_kind;
        let kitty_to = self.kitty_to.clone();
        let endpoint = {
            let p = MultiProvider::from_kind(provider_kind, kitty_to.as_deref())
                .unwrap_or_else(|_| crate::provider::detect_providers(kitty_to.as_deref()));
            p.prefer_launch_endpoint(cwd.as_deref())
        };
        let group = self.launch_group.trim().to_string();
        let group = if group.is_empty() { None } else { Some(group) };
        let note = Arc::clone(&self.focus_note);
        let req = LaunchRequest {
            kind,
            cwd: cwd.clone(),
            endpoint,
            tab_title: None,
        };
        thread::spawn(move || {
            let provider = MultiProvider::from_kind(provider_kind, kitty_to.as_deref())
                .unwrap_or_else(|_| crate::provider::detect_providers(kitty_to.as_deref()));
            let msg = match provider.launch(&req) {
                Ok(result) => {
                    let mut status = format!(
                        "launched {}  cwd={}",
                        kind.as_str(),
                        result.cwd.as_deref().unwrap_or("—")
                    );
                    if let Some(ref g) = group {
                        thread::sleep(Duration::from_millis(450));
                        if let Ok(sessions) = provider.list_sessions() {
                            let found = sessions
                                .iter()
                                .find(|s| {
                                    result
                                        .native_id
                                        .as_ref()
                                        .is_some_and(|nid| session_matches_native_id(s, nid))
                                })
                                .or_else(|| {
                                    sessions.iter().find(|s| {
                                        result.window_id.is_some()
                                            && s.focus_window_id == result.window_id
                                    })
                                })
                                .or_else(|| sessions.iter().find(|s| s.cwd == result.cwd));
                            if let Some(session) = found {
                                match UserState::load() {
                                    Ok(mut st) => {
                                        if let Err(e) = st.assign(session, g) {
                                            status = format!("{status} · group assign failed: {e}");
                                        } else if let Err(e) = st.save() {
                                            status = format!("{status} · save failed: {e}");
                                        } else {
                                            status = format!("{status} · → {g}");
                                        }
                                    }
                                    Err(e) => {
                                        status = format!("{status} · state load failed: {e}");
                                    }
                                }
                            } else {
                                status = format!("{status} · (group assign pending — refresh)");
                            }
                        }
                    }
                    status
                }
                Err(e) => format!("launch failed: {e}"),
            };
            if let Ok(mut g) = note.lock() {
                *g = Some(msg);
            }
        });
        self.status = format!("launching {}…", kind.as_str());
    }

    fn fill_launch_cwd_from_selection(&mut self) {
        if let Some(s) = self.selected_session() {
            if let Some(c) = &s.cwd {
                self.launch_cwd = c.clone();
            }
        }
    }

    fn rebuild_views(&mut self) {
        let q = self.filter_query.trim();
        let filtering = !q.is_empty();
        let filtered: Vec<ProviderSession> = if filtering {
            filter::filter_sessions(&self.all_sessions, &self.user_state, q)
        } else {
            self.all_sessions.clone()
        };
        let full_queue = build_action_queue(&self.all_sessions, &self.user_state);
        self.action_queue = if filtering {
            full_queue
                .into_iter()
                .filter(|s| session_matches(s, &self.user_state, q, None))
                .collect()
        } else {
            full_queue
        };
        let mut sections = build_display_sections(filtered, &self.user_state);
        if filtering {
            sections.retain(|sec| match sec {
                DisplaySection::Manual { sessions, .. } | DisplaySection::Auto { sessions, .. } => {
                    !sessions.is_empty()
                }
            });
        }
        self.sections = sections;
        self.session_count = self.all_sessions.len();
        self.manual_groups = self
            .user_state
            .ordered_groups()
            .into_iter()
            .cloned()
            .collect();

        if let Some(i) = self.queue_sel {
            if i >= self.action_queue.len() {
                self.queue_sel = if self.action_queue.is_empty() {
                    None
                } else {
                    Some(self.action_queue.len() - 1)
                };
            }
        }
        // Drop selection if it points outside filtered list.
        if let Some((si, mi)) = self.selected {
            let ok = self.sections.get(si).is_some_and(|sec| {
                let n = match sec {
                    DisplaySection::Manual { sessions, .. }
                    | DisplaySection::Auto { sessions, .. } => sessions.len(),
                };
                mi < n
            });
            if !ok {
                self.selected = None;
            }
        }

        let shown: usize =
            self.sections
                .iter()
                .map(|sec| match sec {
                    DisplaySection::Manual { sessions, .. }
                    | DisplaySection::Auto { sessions, .. } => sessions.len(),
                })
                .sum();
        let n_manual = self
            .sections
            .iter()
            .filter(|s| matches!(s, DisplaySection::Manual { .. }))
            .count();
        self.path_suggestions = hints::suggestions(&self.all_sessions, &self.user_state);
        if filtering {
            self.status = format!(
                "filter `{q}` · {shown}/{} · queue {} · hints {} · live",
                self.session_count,
                self.action_queue.len(),
                self.path_suggestions.len()
            );
        } else {
            self.status = format!(
                "{} session(s) · queue {} · {} manual · {} hint(s) · live",
                self.session_count,
                self.action_queue.len(),
                n_manual,
                self.path_suggestions.len()
            );
        }
        self.ensure_selection_if_needed();
    }

    fn focus_session(&self, session: &ProviderSession) {
        let session = session.clone();
        let note = Arc::clone(&self.focus_note);
        let provider_kind = self.provider_kind;
        let kitty_to = self.kitty_to.clone();
        thread::spawn(move || {
            let provider = MultiProvider::from_kind(provider_kind, kitty_to.as_deref())
                .unwrap_or_else(|_| crate::provider::detect_providers(kitty_to.as_deref()));
            let msg = match provider.focus(&session) {
                Ok(()) => format!("focused {}", session.id),
                Err(e) => format!("focus failed: {e}"),
            };
            if let Ok(mut g) = note.lock() {
                *g = Some(msg);
            }
        });
    }

    fn selected_session(&self) -> Option<&ProviderSession> {
        let (si, mi) = self.selected?;
        match self.sections.get(si)? {
            DisplaySection::Manual { sessions, .. } | DisplaySection::Auto { sessions, .. } => {
                sessions.get(mi)
            }
        }
    }

    /// Focus the highlighted row, else the first queue item, else the first
    /// filtered session. Used by Enter (including from the filter field).
    fn focus_current_or_first(&mut self) {
        if let Some(s) = self.selected_session().cloned() {
            self.focus_session(&s);
            self.status = format!("focused {}", s.id);
            return;
        }
        if let Some(i) = self.queue_sel {
            self.queue_focus_index(i);
            return;
        }
        if !self.action_queue.is_empty() {
            self.queue_focus_index(0);
            return;
        }
        for (si, sec) in self.sections.iter().enumerate() {
            let members = match sec {
                DisplaySection::Manual { sessions, .. } | DisplaySection::Auto { sessions, .. } => {
                    sessions
                }
            };
            if let Some(s) = members.first() {
                self.selected = Some((si, 0));
                self.focus_session(s);
                self.status = format!("focused {}", s.id);
                return;
            }
        }
        self.status = "nothing to focus".into();
    }

    /// If nothing is selected after a filter change, highlight the first match.
    fn ensure_selection_if_needed(&mut self) {
        if self.selected.is_some() || self.queue_sel.is_some() {
            return;
        }
        if !self.action_queue.is_empty() {
            self.queue_sel = Some(0);
            return;
        }
        for (si, sec) in self.sections.iter().enumerate() {
            let n = match sec {
                DisplaySection::Manual { sessions, .. } | DisplaySection::Auto { sessions, .. } => {
                    sessions.len()
                }
            };
            if n > 0 {
                self.selected = Some((si, 0));
                return;
            }
        }
    }

    fn move_selection(&mut self, delta: i32) {
        let mut flat: Vec<(usize, usize)> = Vec::new();
        for (si, sec) in self.sections.iter().enumerate() {
            let n = match sec {
                DisplaySection::Manual { sessions, .. } | DisplaySection::Auto { sessions, .. } => {
                    sessions.len()
                }
            };
            for mi in 0..n {
                flat.push((si, mi));
            }
        }
        if flat.is_empty() {
            self.selected = None;
            return;
        }
        let cur = self
            .selected
            .and_then(|s| flat.iter().position(|&x| x == s))
            .unwrap_or(0);
        let next = if delta < 0 {
            cur.saturating_sub((-delta) as usize)
        } else {
            (cur + delta as usize).min(flat.len() - 1)
        };
        self.selected = Some(flat[next]);
    }

    fn apply_snapshot(&mut self) {
        let taken = self.snapshot.lock().ok().and_then(|mut g| g.take());
        let Some(result) = taken else {
            return;
        };
        match result {
            Ok((sessions, state)) => {
                self.all_sessions = sessions;
                self.user_state = state;
                self.error = None;
                self.rebuild_views();
            }
            Err(e) => {
                self.error = Some(e);
                self.all_sessions.clear();
                self.sections.clear();
                self.action_queue.clear();
                self.session_count = 0;
                self.status = "provider error".into();
            }
        }
    }

    fn queue_focus_index(&mut self, index: usize) {
        if let Some(s) = self.action_queue.get(index).cloned() {
            self.queue_sel = Some(index);
            self.focus_session(&s);
            self.status = format!("queue #{} → {} ({})", index + 1, s.id, s.attention.label());
        }
    }

    fn queue_next(&mut self) {
        if self.action_queue.is_empty() {
            self.status = "action queue empty".into();
            return;
        }
        let i = match self.queue_sel {
            Some(i) => (i + 1) % self.action_queue.len(),
            None => 0,
        };
        self.queue_focus_index(i);
    }

    fn queue_prev(&mut self) {
        if self.action_queue.is_empty() {
            self.status = "action queue empty".into();
            return;
        }
        let i = match self.queue_sel {
            Some(0) => self.action_queue.len() - 1,
            Some(i) => i - 1,
            None => 0,
        };
        self.queue_focus_index(i);
    }

    fn assign_selected(&mut self, group_id_or_title: &str) {
        let Some(session) = self.selected_session().cloned() else {
            self.status = "select a session first".into();
            return;
        };
        match UserState::load() {
            Ok(mut st) => {
                if let Err(e) = st.assign(&session, group_id_or_title) {
                    self.status = format!("{e}");
                    return;
                }
                if let Err(e) = st.save() {
                    self.status = format!("save failed: {e}");
                    return;
                }
                self.status = format!("assigned {} → {}", session.id, group_id_or_title);
                self.manual_groups = st.ordered_groups().into_iter().cloned().collect();
            }
            Err(e) => self.status = format!("load state: {e}"),
        }
    }

    fn unassign_selected(&mut self) {
        let Some(session) = self.selected_session().cloned() else {
            self.status = "select a session first".into();
            return;
        };
        match UserState::load() {
            Ok(mut st) => {
                st.unassign(&session);
                if let Err(e) = st.save() {
                    self.status = format!("save failed: {e}");
                    return;
                }
                self.status = format!("unassigned {}", session.id);
            }
            Err(e) => self.status = format!("load state: {e}"),
        }
    }

    fn set_priority_selected(&mut self, priority: Priority) {
        let Some(session) = self.selected_session().cloned() else {
            self.status = "select a session first".into();
            return;
        };
        match UserState::load() {
            Ok(mut st) => {
                st.set_priority(&session, priority);
                if let Err(e) = st.save() {
                    self.status = format!("save failed: {e}");
                    return;
                }
                self.status = format!("priority {} → {}", session.id, priority.as_str());
            }
            Err(e) => self.status = format!("load state: {e}"),
        }
    }

    fn create_group_from_field(&mut self) {
        let name = self.new_group_name.trim().to_string();
        if name.is_empty() {
            self.status = "enter a group name".into();
            return;
        }
        match UserState::load() {
            Ok(mut st) => {
                let g = st.create_group(&name);
                if let Err(e) = st.save() {
                    self.status = format!("save failed: {e}");
                    return;
                }
                self.status = format!("created group {}", g.title);
                self.new_group_name.clear();
                self.manual_groups = st.ordered_groups().into_iter().cloned().collect();
            }
            Err(e) => self.status = format!("load state: {e}"),
        }
    }

    fn delete_group(&mut self, id_or_title: &str) {
        match UserState::load() {
            Ok(mut st) => {
                let title = st
                    .find_group(id_or_title)
                    .map(|g| g.title.clone())
                    .unwrap_or_else(|| id_or_title.to_string());
                if let Err(e) = st.delete_group(id_or_title) {
                    self.status = format!("{e}");
                    return;
                }
                if let Err(e) = st.save() {
                    self.status = format!("save failed: {e}");
                    return;
                }
                self.status = format!("deleted group {title} (tabs unassigned, not closed)");
                self.manual_groups = st.ordered_groups().into_iter().cloned().collect();
            }
            Err(e) => self.status = format!("load state: {e}"),
        }
    }

    fn request_quit(&self, ctx: &egui::Context) {
        self.quit_flag.store(true, Ordering::Relaxed);
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

impl eframe::App for PanelState {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.quit_flag.load(Ordering::Relaxed) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        if self.show_flag.swap(false, Ordering::Relaxed) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        }

        if ctx.input(|i| i.viewport().close_requested()) {
            self.quit_flag.store(true, Ordering::Relaxed);
        }

        self.apply_snapshot();

        // When typing in text fields, don't steal letters for n/p nav.
        // Enter is handled separately so filter Enter still focuses a match.
        let typing = ctx.wants_keyboard_input();
        let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
        ctx.input(|i| {
            if !typing {
                if i.key_pressed(egui::Key::Slash) {
                    self.focus_filter = true;
                }
                if i.key_pressed(egui::Key::ArrowDown) {
                    self.move_selection(1);
                }
                if i.key_pressed(egui::Key::ArrowUp) {
                    self.move_selection(-1);
                }
                if i.key_pressed(egui::Key::N) {
                    self.queue_next();
                }
                if i.key_pressed(egui::Key::P) {
                    self.queue_prev();
                }
            } else {
                // Arrow keys still move selection while filter is focused.
                if i.key_pressed(egui::Key::ArrowDown) {
                    self.move_selection(1);
                }
                if i.key_pressed(egui::Key::ArrowUp) {
                    self.move_selection(-1);
                }
            }
            if i.key_pressed(egui::Key::Escape) && !self.filter_query.is_empty() {
                self.filter_query.clear();
                self.rebuild_views();
            }
        });
        // Enter outside text fields: focus selection (handled below for filter too).
        if enter && !typing {
            self.focus_current_or_first();
        }

        if let Ok(mut g) = self.focus_note.lock() {
            if let Some(msg) = g.take() {
                self.status = msg;
            }
        }

        // Smooth pulse for needs_you rows
        ctx.request_repaint_after(Duration::from_millis(50));
        let pulse = self.pulse(ctx);
        let (n_kitty, n_tmux, _) = self.provider_counts();
        let n_queue = self.action_queue.len();
        let n_manual = self.manual_groups.len();
        let n_hints = self.path_suggestions.len();

        egui::TopBottomPanel::top("top")
            .frame(
                egui::Frame::new()
                    .fill(th::BG_ELEVATED)
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .stroke(egui::Stroke::new(1.0, Color32::from_rgb(36, 40, 56))),
            )
            .show(ctx, |ui| {
                // Title row
                ui.horizontal(|ui| {
                    ui.label(RichText::new("termorg").strong().size(17.0).color(th::BLUE));
                    ui.label(
                        RichText::new("Terminal Organiser")
                            .small()
                            .color(th::FG_DIM),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button(RichText::new("Quit").color(th::FG_DIM))
                            .clicked()
                        {
                            self.request_quit(ctx);
                        }
                        if ui.small_button("Refresh").clicked() {
                            self.apply_snapshot();
                        }
                        let tools_label = if self.show_tools {
                            "Tools ▾"
                        } else {
                            "Tools ▸"
                        };
                        if ui
                            .small_button(tools_label)
                            .on_hover_text("Groups & launch")
                            .clicked()
                        {
                            self.show_tools = !self.show_tools;
                        }
                    });
                });

                // Stats chips
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    th::stat_chip(ui, "sessions", &self.session_count.to_string(), th::FG);
                    if n_kitty > 0 {
                        th::stat_chip(ui, "kitty", &n_kitty.to_string(), th::PROVIDER_KITTY);
                    }
                    if n_tmux > 0 {
                        th::stat_chip(ui, "tmux", &n_tmux.to_string(), th::PROVIDER_TMUX);
                    }
                    th::stat_chip(
                        ui,
                        "queue",
                        &n_queue.to_string(),
                        if n_queue > 0 { th::PINK } else { th::FG_DIM },
                    );
                    th::stat_chip(ui, "groups", &n_manual.to_string(), th::AMBER);
                    if n_hints > 0 {
                        th::stat_chip(ui, "hints", &n_hints.to_string(), th::PURPLE);
                    }
                });

                // Status line
                if !self.status.is_empty() {
                    ui.add_space(4.0);
                    ui.label(RichText::new(&self.status).small().color(th::FG_DIM));
                }

                // Filter — always visible
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("⌕").color(th::BLUE));
                    let te = egui::TextEdit::singleline(&mut self.filter_query)
                        .desired_width(ui.available_width() - 48.0)
                        .hint_text("Filter title · path · agent · provider · Enter focuses")
                        .id_source("termorg_filter");
                    let resp = ui.add(te);
                    if self.focus_filter {
                        resp.request_focus();
                        self.focus_filter = false;
                    }
                    if resp.changed() {
                        self.rebuild_views();
                    }
                    if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.focus_current_or_first();
                    }
                    if !self.filter_query.is_empty()
                        && ui.small_button("✕").on_hover_text("Clear (Esc)").clicked()
                    {
                        self.filter_query.clear();
                        self.rebuild_views();
                    }
                });

                // Collapsible tools: groups + launch
                if self.show_tools {
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(6.0);
                    ui.label(RichText::new("Groups").small().strong().color(th::AMBER));
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_group_name)
                                .desired_width(160.0)
                                .hint_text("New group name"),
                        );
                        if ui.button(RichText::new("Create").small()).clicked() {
                            self.create_group_from_field();
                        }
                    });
                    if !self.manual_groups.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            let groups = self.manual_groups.clone();
                            for g in groups {
                                if ui
                                    .small_button(format!("✕ {}", g.title))
                                    .on_hover_text("Delete group (tabs unassigned)")
                                    .clicked()
                                {
                                    self.delete_group(&g.id);
                                }
                            }
                        });
                    }

                    ui.add_space(8.0);
                    ui.label(RichText::new("Launch").small().strong().color(th::GREEN));
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.launch_cwd)
                                .desired_width(200.0)
                                .hint_text("cwd")
                                .id_source("termorg_launch_cwd"),
                        );
                        if ui
                            .small_button("⇤")
                            .on_hover_text("Use selected cwd")
                            .clicked()
                        {
                            self.fill_launch_cwd_from_selection();
                        }
                        ui.add(
                            egui::TextEdit::singleline(&mut self.launch_group)
                                .desired_width(90.0)
                                .hint_text("group")
                                .id_source("termorg_launch_group"),
                        );
                        if !self.manual_groups.is_empty() {
                            ui.menu_button("▾", |ui| {
                                let groups = self.manual_groups.clone();
                                for g in groups {
                                    if ui.button(&g.title).clicked() {
                                        self.launch_group = g.title.clone();
                                        ui.close_menu();
                                    }
                                }
                                if ui.button("(none)").clicked() {
                                    self.launch_group.clear();
                                    ui.close_menu();
                                }
                            });
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        if ui.small_button("shell").clicked() {
                            self.launch_kind(LaunchKind::Shell);
                        }
                        if ui
                            .small_button(RichText::new("Claude").color(th::PINK))
                            .clicked()
                        {
                            self.launch_kind(LaunchKind::Claude);
                        }
                        if ui
                            .small_button(RichText::new("Grok").color(th::BLUE))
                            .clicked()
                        {
                            self.launch_kind(LaunchKind::Grok);
                        }
                        if ui
                            .small_button(RichText::new("Kilo").color(th::PURPLE))
                            .clicked()
                        {
                            self.launch_kind(LaunchKind::Kilo);
                        }
                        if ui
                            .small_button(RichText::new("Codex").color(th::GREEN))
                            .clicked()
                        {
                            self.launch_kind(LaunchKind::Codex);
                        }
                    });
                }
            });

        egui::TopBottomPanel::bottom("bottom")
            .frame(
                egui::Frame::new()
                    .fill(th::BG_ELEVATED)
                    .inner_margin(egui::Margin::symmetric(12, 6))
                    .stroke(egui::Stroke::new(1.0, Color32::from_rgb(36, 40, 56))),
            )
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let keys = [
                        ("↵", "focus"),
                        ("/", "filter"),
                        ("n/p", "queue"),
                        ("↑↓", "select"),
                        ("Esc", "clear"),
                    ];
                    for (k, v) in keys {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 3.0;
                            th::pill(ui, k, th::BLUE, th::tinted_bg(th::BLUE, 36));
                            ui.label(RichText::new(v).small().color(th::FG_DIM));
                        });
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(th::BG)
                    .inner_margin(egui::Margin::symmetric(12, 10)),
            )
            .show(ctx, |ui| {
                if let Some(err) = &self.error {
                    ui.add_space(12.0);
                    egui::Frame::new()
                        .fill(Color32::from_rgb(48, 30, 36))
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(14.0)
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("Cannot see terminals")
                                    .strong()
                                    .color(th::PINK),
                            );
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(err)
                                    .small()
                                    .color(Color32::from_rgb(192, 160, 170)),
                            );
                        });
                    return;
                }

                // ── Path hints (FS15) ────────────────────────────────────────
                let mut hint_accept: Option<String> = None;
                let mut hint_dismiss: Option<String> = None;
                if !self.path_suggestions.is_empty() {
                    th::section_header(
                        ui,
                        "✦",
                        "Path suggestions",
                        &format!("{}", self.path_suggestions.len()),
                        th::PURPLE,
                    );
                    ui.label(
                        RichText::new("Accept to assign · Dismiss to hide this path")
                            .small()
                            .color(th::FG_DIM),
                    );
                    let sug = self.path_suggestions.clone();
                    for s in sug {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("{} → ◆ {}", s.path_title, s.group_title))
                                    .small()
                                    .color(th::FG),
                            );
                            if ui.small_button("Accept").clicked() {
                                hint_accept = Some(s.session_id.clone());
                            }
                            if ui.small_button("Dismiss").clicked() {
                                hint_dismiss = Some(s.path_key.clone());
                            }
                        });
                    }
                    ui.add_space(8.0);
                }
                if let Some(id) = hint_accept {
                    self.accept_hint(&id);
                }
                if let Some(key) = hint_dismiss {
                    self.dismiss_hint(&key);
                }

                // ── Action queue hero (FS9) ─────────────────────────────────
                let mut queue_click: Option<usize> = None;
                let queue_meta = if self.action_queue.is_empty() {
                    "clear".into()
                } else {
                    format!(
                        "{} item{}",
                        self.action_queue.len(),
                        if self.action_queue.len() == 1 {
                            ""
                        } else {
                            "s"
                        }
                    )
                };
                th::section_header(ui, "◎", "Needs you", &queue_meta, th::PINK);

                if self.action_queue.is_empty() {
                    ui.add_space(2.0);
                    egui::Frame::new()
                        .fill(th::BG_ROW)
                        .corner_radius(CornerRadius::same(7))
                        .inner_margin(egui::Margin::symmetric(12, 10))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("Nothing needs you right now").color(th::FG_SOFT),
                            );
                            ui.label(
                                RichText::new(
                                    "Agent hooks drive this queue · n/p to cycle when busy",
                                )
                                .small()
                                .color(th::FG_DIM),
                            );
                        });
                } else {
                    let qsel = self.queue_sel;
                    let pri_map: std::collections::HashMap<String, Priority> = {
                        let st = UserState::load().unwrap_or_default();
                        self.action_queue
                            .iter()
                            .map(|s| (s.id.clone(), st.priority_for(s)))
                            .collect()
                    };
                    for (qi, s) in self.action_queue.iter().enumerate() {
                        let pri = pri_map.get(&s.id).copied().unwrap_or(Priority::Normal);
                        if rows::queue_row(ui, qi, s, qsel == Some(qi), pri, pulse) {
                            queue_click = Some(qi);
                        }
                    }
                }
                ui.add_space(10.0);

                if let Some(qi) = queue_click {
                    self.queue_focus_index(qi);
                }

                if self.sections.is_empty() && self.action_queue.is_empty() {
                    let (title, hint) = if !self.filter_query.trim().is_empty() {
                        (
                            "No sessions match filter",
                            "Esc clears · try agent name or path fragment",
                        )
                    } else {
                        (
                            "No sessions",
                            "Start Kitty with remote control or a tmux session",
                        )
                    };
                    th::empty_state(ui, title, hint);
                    return;
                }

                th::section_header(
                    ui,
                    "☰",
                    "Sessions",
                    &format!("{}", self.session_count),
                    th::BLUE,
                );
                ui.add_space(2.0);

                let mut clicked: Option<ProviderSession> = None;
                let mut assign_to: Option<(ProviderSession, String)> = None;
                let mut unassign_s: Option<ProviderSession> = None;
                let mut set_pri: Option<(ProviderSession, Priority)> = None;
                let selected = self.selected;
                let groups_for_menu = self.manual_groups.clone();
                let pri_lookup: std::collections::HashMap<String, Priority> = {
                    let st = UserState::load().unwrap_or_default();
                    let mut m = std::collections::HashMap::new();
                    for sec in &self.sections {
                        let members = match sec {
                            DisplaySection::Manual { sessions, .. }
                            | DisplaySection::Auto { sessions, .. } => sessions,
                        };
                        for s in members {
                            m.insert(s.id.clone(), st.priority_for(s));
                        }
                    }
                    m
                };

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (si, sec) in self.sections.iter().enumerate() {
                            ui.add_space(6.0);
                            match sec {
                                DisplaySection::Manual { group, sessions } => {
                                    let tab_meta = if sessions.len() == 1 {
                                        "1 tab · manual".to_string()
                                    } else {
                                        format!("{} tabs · manual", sessions.len())
                                    };
                                    th::section_header(
                                        ui,
                                        "◆",
                                        &group.title,
                                        &tab_meta,
                                        th::AMBER,
                                    );
                                    for (mi, s) in sessions.iter().enumerate() {
                                        let is_sel = selected == Some((si, mi));
                                        let pri = pri_lookup
                                            .get(&s.id)
                                            .copied()
                                            .unwrap_or(Priority::Normal);
                                        let action = rows::session_row(
                                            ui,
                                            s,
                                            is_sel,
                                            true,
                                            pri,
                                            &groups_for_menu,
                                            pulse,
                                        );
                                        match action {
                                            RowAction::Focus => clicked = Some(s.clone()),
                                            RowAction::Assign(gid) => {
                                                assign_to = Some((s.clone(), gid));
                                            }
                                            RowAction::Unassign => unassign_s = Some(s.clone()),
                                            RowAction::SetPriority(p) => {
                                                set_pri = Some((s.clone(), p));
                                            }
                                            RowAction::None => {}
                                        }
                                    }
                                }
                                DisplaySection::Auto {
                                    title,
                                    path_hint,
                                    sessions,
                                } => {
                                    let tab_meta = if sessions.len() == 1 {
                                        "1 tab".to_string()
                                    } else {
                                        format!("{} tabs", sessions.len())
                                    };
                                    th::section_header(ui, "▶", title, &tab_meta, th::BLUE);
                                    if path_hint != title && !path_hint.is_empty() {
                                        ui.label(
                                            RichText::new(rows::collapse_home(path_hint))
                                                .small()
                                                .color(th::FG_DIM),
                                        );
                                    }
                                    for (mi, s) in sessions.iter().enumerate() {
                                        let is_sel = selected == Some((si, mi));
                                        let pri = pri_lookup
                                            .get(&s.id)
                                            .copied()
                                            .unwrap_or(Priority::Normal);
                                        let action = rows::session_row(
                                            ui,
                                            s,
                                            is_sel,
                                            false,
                                            pri,
                                            &groups_for_menu,
                                            pulse,
                                        );
                                        match action {
                                            RowAction::Focus => clicked = Some(s.clone()),
                                            RowAction::Assign(gid) => {
                                                assign_to = Some((s.clone(), gid));
                                            }
                                            RowAction::Unassign => unassign_s = Some(s.clone()),
                                            RowAction::SetPriority(p) => {
                                                set_pri = Some((s.clone(), p));
                                            }
                                            RowAction::None => {}
                                        }
                                    }
                                }
                            }
                        }
                        ui.add_space(12.0);
                    });

                fn select_session(panel: &mut PanelState, s: &ProviderSession) {
                    for (si, sec) in panel.sections.iter().enumerate() {
                        let members = match sec {
                            DisplaySection::Manual { sessions, .. }
                            | DisplaySection::Auto { sessions, .. } => sessions,
                        };
                        if let Some(mi) = members.iter().position(|m| m.id == s.id) {
                            panel.selected = Some((si, mi));
                            break;
                        }
                    }
                }

                if let Some(s) = clicked {
                    select_session(self, &s);
                    self.focus_session(&s);
                }
                if let Some((s, gid)) = assign_to {
                    select_session(self, &s);
                    self.assign_selected(&gid);
                }
                if let Some(s) = unassign_s {
                    select_session(self, &s);
                    self.unassign_selected();
                }
                if let Some((s, p)) = set_pri {
                    select_session(self, &s);
                    self.set_priority_selected(p);
                }
            });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.quit_flag.store(true, Ordering::Relaxed);
    }
}
