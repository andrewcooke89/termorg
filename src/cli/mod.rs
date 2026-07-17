//! Command-line interface.

mod args;
mod commands;

use clap::Parser;

use crate::provider::{MultiProvider, ProviderKind};
use args::{Cli, Commands, HintsCmd, QueueCmd};
use commands::{
    cmd_assign, cmd_focus, cmd_group, cmd_hints, cmd_hook, cmd_launch, cmd_list, cmd_priority,
    cmd_queue, cmd_unassign, cmd_watch,
};

/// Parse CLI args and execute the requested command.
pub fn run() {
    let cli = Cli::parse();

    // Hook path must stay fast and independent of terminal providers (agent hooks).
    if let Commands::Hook {
        state,
        reason,
        list,
    } = &cli.command
    {
        if let Err(e) = cmd_hook(state.as_deref(), reason.as_deref(), *list) {
            eprintln!("termorg: {e}");
            std::process::exit(1);
        }
        return;
    }

    let kind = ProviderKind::parse(&cli.provider).unwrap_or_else(|| {
        eprintln!(
            "termorg: unknown --provider `{}` (use kitty|tmux|all)",
            cli.provider
        );
        std::process::exit(2);
    });
    let provider = match build_provider(kind, cli.kitty_to.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("termorg: {e}");
            std::process::exit(1);
        }
    };

    let result = match cli.command {
        Commands::List { json, flat, filter } => cmd_list(&provider, json, flat, filter.as_deref()),
        Commands::Watch {
            interval,
            no_notify,
        } => cmd_watch(&provider, interval, !no_notify),
        Commands::Panel => {
            if let Err(e) = crate::ui::run_panel(provider, kind, cli.kitty_to.clone()) {
                eprintln!("termorg: {e}");
                std::process::exit(1);
            }
            return;
        }
        Commands::Focus { id } => cmd_focus(&provider, &id),
        Commands::Group { action } => cmd_group(action),
        Commands::Assign { session_id, group } => cmd_assign(&provider, &session_id, &group),
        Commands::Unassign { session_id } => cmd_unassign(&provider, &session_id),
        Commands::Priority { session_id, level } => cmd_priority(&provider, &session_id, &level),
        Commands::Queue { action } => cmd_queue(&provider, action.unwrap_or(QueueCmd::List)),
        Commands::Next => cmd_queue(&provider, QueueCmd::Next),
        Commands::Prev => cmd_queue(&provider, QueueCmd::Prev),
        Commands::Launch {
            agent,
            cwd,
            group,
            endpoint,
            title,
        } => cmd_launch(
            &provider,
            &agent,
            cwd.as_deref(),
            group.as_deref(),
            endpoint.as_deref(),
            title.as_deref(),
        ),
        Commands::Hints { action } => cmd_hints(&provider, action.unwrap_or(HintsCmd::List)),
        Commands::Hook { .. } => unreachable!("handled above"),
    };

    if let Err(e) = result {
        eprintln!("termorg: {e}");
        std::process::exit(1);
    }
}

fn build_provider(kind: ProviderKind, kitty_to: Option<&str>) -> crate::error::Result<MultiProvider> {
    MultiProvider::from_kind(kind, kitty_to)
}
