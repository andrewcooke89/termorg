# Agent hooks for termorg

`termorg hook` reads agent lifecycle JSON on stdin and records attention signals
under `~/.config/termorg/signals.json`.

Install `termorg` on `PATH` first (`cargo install --path .`).

## Match keys (auto from environment)

| Env | Provider | Match quality |
|-----|----------|---------------|
| `KITTY_PID` + `KITTY_WINDOW_ID` | Kitty | exact window |
| `TMUX_PANE` | tmux | exact pane (preferred) |
| `PWD` / payload `cwd` | both | fallback when host keys missing |

Hooks work the same inside Kitty or tmux — termorg records whichever location
keys are present when the hook runs.

```bash
termorg hook                 # stdin: agent lifecycle JSON
termorg hook --list          # debug active signals
termorg hook --state needs_you
```

## Event → attention

| Event (typical) | Attention |
|-----------------|-----------|
| Notification / PermissionRequest / Stop / StopFailure | needs you |
| UserPromptSubmit / PreToolUse / PostToolUse | working |
| SessionEnd | idle |
| SessionStart / compact | ignored (no clobber) |

## Copy-paste installs

### Claude Code

Merge into `~/.claude/settings.json` (or project settings). See full snippet:

[`examples/claude-settings-hooks-snippet.json`](../../examples/claude-settings-hooks-snippet.json)

Minimal pattern (all lifecycle hooks pipe to termorg):

```json
{
  "hooks": {
    "Notification": [{ "hooks": [{ "type": "command", "command": "termorg hook" }] }],
    "Stop": [{ "hooks": [{ "type": "command", "command": "termorg hook" }] }],
    "PreToolUse": [{ "hooks": [{ "type": "command", "command": "termorg hook" }] }],
    "PostToolUse": [{ "hooks": [{ "type": "command", "command": "termorg hook" }] }],
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "termorg hook" }] }],
    "SessionEnd": [{ "hooks": [{ "type": "command", "command": "termorg hook" }] }]
  }
}
```

Claude runs the command with hook JSON on stdin. `KITTY_*` / `TMUX_PANE` come from the shell environment of that tab.

### Grok Build

Copy [`examples/grok-hooks-termorg.json`](../../examples/grok-hooks-termorg.json) to
`~/.grok/hooks/termorg.json` (or merge into your hooks config). Ensure the command is `termorg hook`.

### Codex

Copy [`examples/codex-hooks-termorg.json`](../../examples/codex-hooks-termorg.json)
into `~/.codex/hooks.json` (or merge). Codex uses a **three-level** shape:
`event → matcher group → nested hooks`. Trust via Codex `/hooks`.

Supported events wired there: `PermissionRequest`, `Stop`, `PreToolUse`,
`PostToolUse`, `UserPromptSubmit` (not every Claude event name exists in Codex).

### Kilo

Add a plugin / hook command that runs `termorg hook` with the lifecycle JSON on stdin on Notification, Stop, PreToolUse, and SessionEnd. Ensure the agent process inherits `TMUX_PANE` or Kitty remote-control env when running inside those hosts.

## Verify

```bash
# Inside a tmux pane or Kitty tab running the agent:
echo '{"hook_event_name":"Notification","session_id":"t","cwd":"'"$PWD"'"}' | termorg hook
termorg hook --list
termorg list -q needs
```

You should see `tmux=%…` or `kitty=…` on the signal and **needs you** on the matching session when the agent class is detected.
