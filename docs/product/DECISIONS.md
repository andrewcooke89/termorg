# Decisions

**Status: FROZEN** for core product (through FS9) — 2026-07-17  

Product plan is source of truth. Workflow-experiment process is secondary and must not override these.

No open product decisions remain for FS1–FS9. Deferred items are listed under **Later (post-FS9)** and are explicitly out of core.

---

## Product — locked

### D1 — Control plane, not terminal host
Organiser over real terminal emulators. Does **not** embed a full terminal as the primary experience through FS9.

### D2 — Ops panel UX
Hotkey-toggle mission-control panel: action queue + groups + rows + focus.

### D3 — Dual grouping
1. **Auto path groups** — git root / collapsed cwd  
2. **Manual groups** — user-defined workstreams spanning multiple paths (e.g. Trading)  

Both are first-class. Manual membership is persisted.

### D4 — Single primary manual group (core)
A session has at most **one** manual group. No nested groups. Path remains subtitle/context.

### D5 — Row placement
If manually grouped → show **once** under that manual group (path as subtitle).  
If not → show under auto path group. **No duplicate rows.**

### D6 — Agent color vs attention encoding
**Color/label → agent class.** **Badge + queue order → attention and user priority.**

### D7 — System attention states
`needs_you` | `working` | `idle` | `unknown`. Prefer **low false `needs_you`**.

### D8 — User priority
`normal` | `important` | `muted`, user-set, persisted. Influences queue and scanability.

### D9 — Attention / action queue
First-class ordered queue. Supports focus + next/previous navigation.  
Include/order rules: **D22–D24** and `DATA-MODEL.md`.

### D10 — Provider abstraction
Domain/UI depend on **`TerminalProvider`**, not Kitty.  
**KittyProvider** is the default and first implementation.  
Additional providers are additive (post-FS9 unless needed earlier for a labeled experiment).

### D11 — Host process intel is shared
cwd / process-tree / agent classification may use `/proc` and shared helpers.  
Providers supply session identity and focus (and optional native cwd/title).

### D12 — Default provider: Kitty
Require Kitty remote control for the first shippable path. Document setup.

### D13 — Persistence
User data under `~/.config/termorg/`.  
Persists: manual groups + session prefs (priority, membership).

### D14 — Naming
- Display: **Terminal Organiser**  
- Binary / CLI: **`termorg`**  
- Config dir: **`~/.config/termorg/`**

### D15 — Platform first target
Ubuntu GNOME Wayland; Linux-native.

### D16 — Digestible feature sets
Build order and pass bars: **`FEATURE-SETS.md`** (FS1…FS9 core, FS10+ later).  
Each set passes on **natural-language capability** (“you should be able to…”), not internal API checklists.  
Manual groups, needs-you, priority, and action queue are **core product**; feature sets only sequence delivery.

### D17 — Spawn / launch
Optional provider capability. **Not required for FS1–FS9.**  
Scheduled as later feature set (FS13 sketch in `FEATURE-SETS.md`).

### D18 — Notifications
**Not required for FS1–FS9.** Optional later (FS11 sketch).

### D19 — Implementation default stack
**Rust** + **GTK4** + **libadwaita**.  
Stack is the intended implementation default; provider boundary and domain model outrank stack choice.

### D20 — Session restore matching
Best-effort: provider id, then fingerprint.  
**Never** silently assign priority/group when the match is ambiguous.

### D21 — Session grain = tab
One domain **session** / panel **row** = one terminal **tab** (Kitty: one tab).  
Not one row per OS window when that window has multiple tabs.  
Focus targets that tab.

### D22 — Queue inclusion (frozen v1)
Include a session in the action queue when:

1. `priority != muted` **and** `attention == needs_you`, **or**  
2. `priority == important` **and** `attention` ∈ {`needs_you`, `working`, `unknown`}

Therefore:

- **Idle + important** → **not** in queue (starred in group list only)  
- **Muted + needs_you** → **not** in queue (muted suppresses the queue)

### D23 — Queue ordering (frozen v1)
Sort by, in order:

1. Priority rank: important → normal → muted  
2. Attention rank: needs_you → working → unknown → idle  
3. Age in current attention state (oldest first)  
4. Display name (stable tie-break)

### D24 — Manual vs auto section order
Panel lists **manual groups first** (user `sort_index` order), then **auto path groups** for sessions with no manual group.

### D25 — Default hotkey
Suggested global toggle: **Super+Shift+Space**.  
If global grab fails on GNOME: documented **launch/toggle command** still satisfies FS3.  
Exact binding may be a code constant; must be documented in the app README.

### D26 — Single provider in core UI
Through FS9, only the default provider (Kitty) is required in the UI.  
No multi-provider chrome until a second provider exists.

### D27 — Path hints / multi-tag / auto workstreams
**Out of core (FS1–FS9):**

- Path-prefix auto-suggest into manual groups  
- Multiple manual tags per session  
- Automatic inference of workstreams  

Pure manual assign only for FS6.

### D28 — Plan change control
After freeze, change product decisions only by editing `DECISIONS.md` + related base docs with a decision-log entry.  
Implementation must not silently diverge from frozen rules.

---

## Explicitly rejected (core)

| Idea | Why |
|------|-----|
| Kitty-only architecture with RC calls in UI/domain | Blocks expansion; violates D10 |
| Path grouping only | User needs cross-repo workstreams |
| Attention badges without a queue | Queue is the “what next” answer |
| Auto-only priority | User must pin importance deliberately |
| Full VTE re-host through FS9 | Wrong cost profile |
| Nested manual groups (core) | Complexity without proven need |
| Multiple manual groups per session (core) | Keep model simple; revisit post-FS9 only if needed |
| Idle+important in queue (v1) | Queue stays “act now,” not “everything starred” |
| Muted still appears in queue on needs_you (v1) | Mute means “don’t pull me into the queue” |
| Requiring tmux/zellij | Must not force mux migration |
| Free product scope per implementation workflow | Breaks “same system” if used experimentally |

---

## Later (post-FS9 only — not open for core)

These are **deferred**, not undecided for FS1–FS9:

| ID | Topic | Earliest |
|----|--------|----------|
| L1 | Path hints auto-suggest into manual groups | FS15 sketch |
| L2 | Revisit idle+important in queue | After FS9 daily use |
| L3 | Revisit muted+needs_you alerts | After FS9 daily use |
| L4 | Second terminal provider | FS14 sketch |
| L5 | Multi-manual tags per session | Only if D4 proves too tight |
| L6 | Notifications | FS11 |
| L7 | Spawn/launch from panel | FS13 |
| L8 | Kitty ambient tab title/color sync | FS12 |
| L9 | Search/filter | FS10 |

---

## Decision log

| Date | Note |
|------|------|
| 2026-07-17 | Initial control-plane / kitty-centric MVP sketch |
| 2026-07-17 | Product refocus: manual groups, queue + priority, TerminalProvider + Kitty default |
| 2026-07-17 | Digestible feature sets FS1–FS9 with natural-language pass bars |
| 2026-07-17 | **Freeze tidy:** D17 fixed (no Phase D); D21 session=tab; D22–D24 queue/layout frozen; O1–O6 closed into locked or Later; D25–D28 added |
