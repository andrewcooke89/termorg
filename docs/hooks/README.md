# Agent hooks for termorg

`termorg hook` reads agent lifecycle JSON on stdin and records attention signals
under `~/.config/termorg/signals.json`. Match tabs via `KITTY_PID` + `KITTY_WINDOW_ID`.

Install `termorg` on `PATH`, then wire each agent:

| Agent | Config location |
|-------|-----------------|
| Claude Code | `~/.claude/settings.json` hooks |
| Grok Build | `~/.grok/hooks/termorg.json` |
| Codex | `~/.codex/hooks.json` (trust via `/hooks`) |
| Kilo | plugin under config `plugin` array |

See `examples/` for sample JSON.
