# Kilo → termorg hooks

Kilo’s hook/plugin shape varies by version. The contract for termorg is simple:

1. On agent lifecycle events (notification / permission / stop / pre-tool / session end), run:

   ```bash
   termorg hook
   ```

   with the event JSON on **stdin** (same fields as Claude: `hook_event_name` or `hookEventName`, `cwd`, `session_id`, …).

2. Run the agent inside Kitty or tmux so match keys exist:

   - Kitty: `KITTY_PID`, `KITTY_WINDOW_ID`
   - tmux: `TMUX_PANE`

3. Verify:

   ```bash
   termorg hook --list
   termorg list -q needs
   ```

If Kilo only supports a shell command string without stdin, use:

```bash
termorg hook --state needs_you --reason kilo-notification
```

for stop/permission events, and `--state working` for tool start.
