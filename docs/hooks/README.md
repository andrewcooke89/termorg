# Agent hooks for termorg

`termorg hook` reads agent lifecycle JSON on stdin and records attention signals
under `~/.config/termorg/signals.json`.

## Match keys (auto from environment)

| Env | Provider | Match quality |
|-----|----------|---------------|
| `KITTY_PID` + `KITTY_WINDOW_ID` | Kitty | exact window |
| `TMUX_PANE` | tmux | exact pane (preferred) |
| `PWD` / payload `cwd` | both | fallback when host keys missing |

Hooks work the same inside Kitty or tmux — termorg records whichever location
keys are present in the environment when the hook runs.

Install `termorg` on `PATH`, then wire each agent:

| Agent | Config location |
|-------|-----------------|
| Claude Code | `~/.claude/settings.json` hooks |
| Grok Build | `~/.grok/hooks/termorg.json` |
| Codex | `~/.codex/hooks.json` (trust via `/hooks`) |
| Kilo | plugin under config `plugin` array |

See `examples/` for sample JSON.
