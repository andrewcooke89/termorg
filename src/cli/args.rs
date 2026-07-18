//! Clap definitions for the termorg CLI.

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "termorg",
    about = "Terminal Organiser — list and organise terminal sessions",
    version
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Commands,

    /// Terminal backend: kitty | tmux | all (default: auto-detect both).
    #[arg(long, global = true, env = "TERMORG_PROVIDER", default_value = "all")]
    pub(crate) provider: String,

    /// Kitty remote-control address (unix:/path or tcp:…).
    #[arg(long, global = true, env = "TERMORG_KITTY_LISTEN_ON")]
    pub(crate) kitty_to: Option<String>,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Commands {
    /// List live terminal sessions (manual groups first, then path groups).
    List {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        flat: bool,
        /// Filter by title / path / agent / attention / group (FS10). Multi-word = AND.
        #[arg(long, short = 'q')]
        filter: Option<String>,
        /// Hide idle shell sessions from the list (not the action queue).
        #[arg(long, conflicts_with = "show_idle_shells")]
        hide_idle_shells: bool,
        /// Show idle shells even if TERMORG_HIDE_IDLE_SHELLS=1.
        #[arg(long, conflicts_with = "hide_idle_shells")]
        show_idle_shells: bool,
    },
    Watch {
        #[arg(long, default_value_t = 1)]
        interval: u64,
        /// Disable FS11 desktop notifications (on by default).
        #[arg(long)]
        no_notify: bool,
    },
    Panel,
    Focus {
        id: String,
    },
    /// Manage manual groups (FS6).
    Group {
        #[command(subcommand)]
        action: GroupCmd,
    },
    /// Assign a session to a manual group.
    Assign {
        session_id: String,
        group: String,
    },
    /// Remove a session from its manual group.
    Unassign {
        session_id: String,
    },
    /// Set user priority: important | normal | muted.
    Priority {
        session_id: String,
        level: String,
    },
    /// Action queue (FS9): list or focus next/prev item needing attention.
    Queue {
        #[command(subcommand)]
        action: Option<QueueCmd>,
    },
    /// Shorthand: focus next item in the action queue.
    Next,
    /// Shorthand: focus previous item in the action queue.
    Prev,
    /// Path→group suggestions (FS15).
    Hints {
        #[command(subcommand)]
        action: Option<HintsCmd>,
    },
    /// Launch a new tab/window (FS13): shell or agent in a cwd / group.
    Launch {
        /// shell | claude | grok | kilo | codex (default: shell)
        #[arg(long, short = 'a', default_value = "shell")]
        agent: String,
        /// Working directory (default: $PWD)
        #[arg(long, short = 'C')]
        cwd: Option<String>,
        /// Assign the new tab to this manual group (id or title) after launch
        #[arg(long, short = 'g')]
        group: Option<String>,
        /// Provider endpoint: Kitty `unix:…` or tmux session name. Default: auto.
        #[arg(long)]
        endpoint: Option<String>,
        /// Tab/window title override
        #[arg(long)]
        title: Option<String>,
    },
    /// Ingest an agent lifecycle hook (Claude Code Notification/Stop/… on stdin).
    ///
    /// Wire from `~/.claude/settings.json` hooks so needs_you is event-driven
    /// instead of CPU-guessed. Safe to call with empty stdin if `--state` is set.
    Hook {
        /// Force state: needs_you | working | idle (skip stdin event parsing).
        #[arg(long)]
        state: Option<String>,
        /// Optional reason tag stored with the signal.
        #[arg(long)]
        reason: Option<String>,
        /// List active signals and exit.
        #[arg(long)]
        list: bool,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum QueueCmd {
    /// List the action queue (default).
    List,
    /// Focus the next queue item (wraps).
    Next,
    /// Focus the previous queue item (wraps).
    Prev,
    /// Focus the item at 1-based index.
    Go { index: usize },
}

#[derive(Subcommand, Debug)]
pub(crate) enum HintsCmd {
    /// List current suggestions (default).
    List,
    /// Accept suggestion for session id (assign to suggested group).
    Accept { session_id: String },
    /// Dismiss suggestions for a path key (or session id to resolve path).
    Dismiss { path_or_session: String },
    /// Rebuild learned map from existing assignments.
    Rebuild,
}

#[derive(Subcommand, Debug)]
pub(crate) enum GroupCmd {
    /// List manual groups.
    List,
    /// Create a manual group.
    Create { title: String },
    /// Rename a group (id or title).
    Rename {
        id_or_title: String,
        new_title: String,
    },
    /// Delete a group (id or title).
    Delete { id_or_title: String },
}
