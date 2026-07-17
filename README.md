# termorg — Terminal Organiser

Ops panel and CLI for terminal sessions: path groups, manual workstreams, agent
identity, needs-you detection, action queue, priority, search, notifications,
ambient tab cues, launch, and path→group hints.

**Status:** extracted for open-source prep. Currently **Kitty-first**. Cleanup and
multi-provider work are planned before a public release.

## Requirements

- Linux (developed on Ubuntu GNOME Wayland)
- [Kitty](https://sw.kovidgoyal.net/kitty/) with remote control enabled
- Rust toolchain (`cargo`)

### Kitty remote control

In `kitty.conf`:

```conf
allow_remote_control socket-only
listen_on unix:${HOME}/.cache/kitty/control-{kitty_pid}.sock
```

Restart Kitty windows after changing this.

## Build

```bash
cargo build --release
# optional
cp target/release/termorg ~/.local/bin/termorg
```

## Quick start

```bash
termorg list
termorg panel
termorg queue
termorg next
```

See command help: `termorg --help`.

Config and state live under `~/.config/termorg/` (e.g. `state.json`, `notify.json`,
`ambient.json`, `signals.json`).

## Agent hooks (attention)

Attention is most accurate when agents call `termorg hook` on lifecycle events
(Stop / Notification / PreToolUse / …). Example snippets: [`docs/hooks/`](docs/hooks/).

## Design docs

Product freeze (from original research plan): [`docs/product/`](docs/product/).

## Provenance

See [`ORIGIN.md`](ORIGIN.md).

## License

MIT — see [`LICENSE`](LICENSE).
