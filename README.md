# termorg

**Terminal Organiser** — a small control plane for busy terminal setups.

When you run many agent CLIs (Claude Code, Grok, Codex, Kilo) and shells across
Kitty tabs, termorg helps you **see**, **prioritise**, and **jump** to what needs
you next.

| | |
|--|--|
| **Panel** | Live groups, action queue, filter, launch, path hints |
| **CLI** | Scriptable list / focus / queue / launch / hooks |
| **Attention** | Needs-you / working / idle — prefer **agent hooks** over CPU guessing |
| **Provider** | **Kitty** today · multi-provider next |

![Status](https://img.shields.io/badge/status-pre--release-yellow)
![License](https://img.shields.io/badge/license-MIT-blue)

## Requirements

- Linux (developed on Ubuntu + GNOME Wayland)
- [Rust](https://rustup.rs/) (1.75+)
- [Kitty](https://sw.kovidgoyal.net/kitty/) with remote control

### Kitty config

```conf
allow_remote_control socket-only
listen_on unix:${HOME}/.cache/kitty/control-{kitty_pid}.sock
```

Restart Kitty after changing this. Each OS window gets its own control socket;
termorg discovers all of them under `~/.cache/kitty/`.

## Install

```bash
git clone <repo-url> termorg
cd termorg
cargo install --path .
# or: cargo build --release && cp target/release/termorg ~/.local/bin/
```

## Quick start

```bash
termorg list                 # sessions by path / manual group
termorg list -q claude       # filter
termorg panel                # ops UI (toggle / single instance)
termorg queue                # what needs you
termorg next                 # focus next queue item
termorg launch -a shell -C ~/src/myproj
termorg launch -a claude -C ~/src/myproj -g Trading
```

State lives in `~/.config/termorg/` (`state.json`, signals, notify/ambient config).

## Attention (needs you)

CPU sampling is a weak signal for agent CLIs (they sleep on the network; MCP
servers look “busy”). Prefer **hooks**:

```bash
termorg hook                 # stdin: agent lifecycle JSON
termorg hook --list          # debug signals
termorg hook --state needs_you
```

Wire agents with the snippets under [`examples/`](examples/) and
[`docs/hooks/`](docs/hooks/).

| Event (typical) | Attention |
|-----------------|-----------|
| Notification / Permission / Stop | needs you |
| UserPromptSubmit / PreToolUse | working |

Desktop alerts: rising-edge needs-you via `notify-send` (panel / `watch` / hooks).
Disable with `TERMORG_NOTIFY=0` or `~/.config/termorg/notify.json`.

Ambient Kitty tab colors/titles: panel or `watch` (disable with `TERMORG_AMBIENT=0`).

## Architecture

```
termorg (CLI)
   └── termorg lib
         ├── provider::TerminalProvider  (Kitty impl)
         ├── attention + signals         (hooks + process fallback)
         ├── store                       (groups, priority, path hints)
         ├── queue                       (D22/D23 rules)
         └── ui                          (eframe panel)
```

Design notes (product freeze from the original research plan):
[`docs/product/`](docs/product/).

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md).

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

## Known limitations

1. **Kitty only** — second providers planned.
2. **OS-window raise** — often blocked on GNOME Wayland; **tab** focus works.
3. Attention without hooks is best-effort.

## License

[MIT](LICENSE)

## Provenance

Extracted from a private multi-workflow experiment; see [ORIGIN.md](ORIGIN.md).
