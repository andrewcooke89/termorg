# Contributing to termorg

Thanks for your interest. termorg is a personal open-source **session control
plane** for Kitty tabs and tmux windows (not a terminal emulator).

## Development

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

Requires **Rust 1.85+** (see `rust-version` in `Cargo.toml`).

### Integration testing

| Backend | Notes |
|---------|--------|
| **Kitty** | Remote control sockets required for live `list`/`panel`/`launch` (see README). Unit tests do not need Kitty. |
| **tmux** | Isolated-server unit test creates a disposable `-L` socket. Live CLI: `TERMORG_TMUX_SOCKET`. |

## Pull requests

1. Keep changes focused; one concern per PR when practical.
2. Add or update tests for behaviour changes.
3. Do not commit `target/` or personal config under `~/.config/termorg`.
4. Avoid machine-specific paths in code and examples.

## Code layout

| Path | Role |
|------|------|
| `src/lib.rs` | Library root |
| `src/cli/` | CLI args and command handlers |
| `src/provider/` | Terminal backends (`kitty/`, `tmux/`, `multi`) |
| `src/ui/` | Ops panel (eframe) |
| `src/attention.rs` | Attention classification |
| `src/signals.rs` | Agent hook signal store |
| `src/store.rs` | Persisted groups/prefs/sticky path rules |
| `src/list_json.rs` | Stable `list --json` contract |
| `src/persist.rs` | Locked atomic JSON writes |
| `docs/product/` | Product design freeze |
| `examples/` | Hook config snippets |
