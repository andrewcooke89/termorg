# termorg

**Terminal Organiser** — a small control plane for busy terminal setups.

When you run many agent CLIs (Claude Code, Grok, Codex, Kilo) and shells across
**Kitty tabs** and/or **tmux windows**, termorg helps you **see**, **prioritise**,
and **jump** to what needs you next.

| | |
|--|--|
| **Panel** | Live groups, action queue, filter, launch, path hints |
| **CLI** | Scriptable list / focus / queue / launch / hooks |
| **Attention** | Needs-you / working / idle — prefer **agent hooks** over CPU guessing |
| **Providers** | **Kitty** + **tmux** (auto, or force with `--provider`) |

![Status](https://img.shields.io/badge/status-pre--release-yellow)
![License](https://img.shields.io/badge/license-MIT-blue)

## Requirements

- Linux (developed on Ubuntu + GNOME Wayland)
- [Rust](https://rustup.rs/) (1.75+)
- At least one of:
  - [Kitty](https://sw.kovidgoyal.net/kitty/) with remote control
  - [tmux](https://github.com/tmux/tmux) with a running server

### Kitty config

```conf
allow_remote_control socket-only
listen_on unix:${HOME}/.cache/kitty/control-{kitty_pid}.sock
```

Restart Kitty after changing this. Each OS window gets its own control socket;
termorg discovers all of them under `~/.cache/kitty/`.

### Tmux

No special config required. termorg talks to the default tmux server (`-L default`),
or override with:

| Env | Meaning |
|-----|---------|
| `TERMORG_TMUX_SOCKET` | Socket name for `tmux -L` (default `default`) |
| `TERMORG_TMUX_SOCKET_PATH` | Absolute path for `tmux -S` |

One termorg session = one **tmux window** (the tab analogue). Active pane cwd/command
drive agent classification; `TMUX_PANE` is recorded by hooks for precise matching.

## Install

```bash
git clone <repo-url> termorg
cd termorg
cargo install --path .
# or: cargo build --release && cp target/release/termorg ~/.local/bin/
```

## Quick start

```bash
termorg list                 # sessions (Kitty tabs + tmux windows when both available)
termorg list --provider tmux
termorg list -q claude       # filter
termorg panel                # ops UI (toggle / single instance)
termorg queue                # what needs you
termorg next                 # focus next queue item
termorg launch -a shell -C ~/src/myproj
termorg launch -a claude -C ~/src/myproj -g Trading
termorg launch --provider tmux -a shell -C ~/src/myproj
```

| Flag / env | Meaning |
|------------|---------|
| `--provider kitty\|tmux\|all` | Backend selection (`TERMORG_PROVIDER`, default `all`) |
| `--kitty-to unix:…` | Explicit Kitty remote-control socket |

State lives in `~/.config/termorg/` (`state.json`, signals, notify/ambient config).

## Provider capability matrix

| Capability | Kitty | tmux |
|------------|:-----:|:----:|
| List sessions | tab per OS window | window across sessions |
| Focus | tab + best-effort OS raise | `select-window` + `switch-client` |
| Launch | new tab via RC | `new-window` / `new-session` |
| Ambient color | `set-tab-color` | `window-style` / `window-active-style` |
| Ambient title | `set-tab-title` | `rename-window` |
| Hook match keys | `KITTY_PID` + `KITTY_WINDOW_ID` | `TMUX_PANE` (+ cwd fallback) |

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

Ambient colors/titles: panel or `watch` (disable with `TERMORG_AMBIENT=0`).

## Architecture

```
termorg (CLI)
   └── termorg lib
         ├── provider::TerminalProvider
         │     ├── KittyProvider
         │     ├── TmuxProvider
         │     └── MultiProvider (kitty | tmux | both)
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

1. **OS-window raise** (Kitty) — often blocked on GNOME Wayland; **tab** focus works.
2. **tmux client attach** — focus switches the attached client when possible; detached
   sessions are listed but need attach elsewhere.
3. Attention without hooks is best-effort.

## License

[MIT](LICENSE)

## Provenance

Extracted from a private multi-workflow experiment; see [ORIGIN.md](ORIGIN.md).
