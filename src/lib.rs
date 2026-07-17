//! termorg — Terminal Organiser library.
//!
//! Domain logic, providers, attention, and the ops panel. The CLI binary is a thin
//! wrapper around [`cli::run`].

pub mod agent;
pub mod ambient;
pub mod attention;
pub mod cli;
pub mod error;
pub mod filter;
pub mod hints;
pub mod notify;
pub mod path_group;
pub mod provider;
pub mod queue;
pub mod signals;
pub mod store;
pub mod ui;

pub use error::{Result, TermorgError};
pub use provider::{KittyProvider, LaunchKind, LaunchRequest, ProviderSession, TerminalProvider};
