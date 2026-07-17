# Contributing to termorg

Thanks for your interest. This project is early: Kitty is the primary terminal
backend; multi-provider support is planned.

## Development

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

### Kitty for integration testing

Remote control must be enabled (see README). Unit tests do not require a live
Kitty instance; live commands (`list`, `panel`, `launch`) do.

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
| `src/provider/` | Terminal backends (`kitty/` is the reference) |
| `src/ui/` | Ops panel (eframe) |
| `src/attention.rs` | Attention classification |
| `src/signals.rs` | Agent hook signal store |
| `src/store.rs` | Persisted groups/prefs/hints |
| `docs/product/` | Product design freeze |
| `examples/` | Hook config snippets |

## License

By contributing, you agree that your contributions are licensed under the MIT
License (see `LICENSE`).
