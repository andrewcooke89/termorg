# Terminal Organiser — Product Plan

**Status:** FROZEN for core (FS1–FS9) — 2026-07-17  
**Name:** Terminal Organiser · binary **`termorg`**

Related: [`FEATURE-SETS.md`](./FEATURE-SETS.md) · [`DECISIONS.md`](./DECISIONS.md) · [`DATA-MODEL.md`](./DATA-MODEL.md)

---

## 1. Problem

You run many terminals every day. It is hard to:

1. See which sessions belong together (**by project path**, and by **ad‑hoc workstreams** that span several repos)
2. See **what is running** (Claude Code, Grok Build, Kilo, Codex, shell, …)
3. Know **what needs you next** — an ordered action queue, not a flat sea of windows
4. Keep important sessions visible without hunting

Kitty is the current host. The product must not assume it is the only host forever.

## 2. Product goal

A Linux-native **ops panel** for terminal sessions:

| Capability | Intent |
|------------|--------|
| **Path grouping** | Auto-group by project location (git root / cwd) |
| **Manual grouping** | User-defined groups spanning multiple paths (e.g. *Trading* → `trading-brain` + `prop_challenges`) |
| **Agent identity** | Color + label by what is running |
| **Attention queue** | Ordered list of sessions waiting for you |
| **User priority** | Mark sessions important / elevate them in the queue and UI |
| **Provider-agnostic core** | Domain model does not hard-depend on kitty; **Kitty is the default provider** |

**Non-goals (near term):** replace the terminal emulator; re-implement GPU scrollback; multi-user SaaS; perfect semantic understanding of every agent UI.

## 3. Product shape

**Primary UI:** hotkey-toggle **ops panel** (mission control), not a full terminal host.

```
┌─ termorg panel ─────────────────────────────────────────┐
│ ◎ Attention (3)     [queue: needs you + elevated]       │
│   1. trading-brain · claude · needs_you        ★        │
│   2. prop_challenges · codex · needs_you                │
│   3. flowforge · grok · working (pinned)       ★        │
│─────────────────────────────────────────────────────────│
│ ▼ Trading                          (manual group)       │
│     trading-brain    claude   needs_you        ★        │
│     prop_challenges  codex    needs_you                 │
│     hl-logs          shell    idle                      │
│ ▼ terminal_organiser               (auto · path)        │
│     tab “detect”     shell    idle                      │
│ ▼ ~/scratch                        (auto · path)        │
│     …                                                   │
└─────────────────────────────────────────────────────────┘
```

### Surfaces

1. **Action queue** — top of panel (or dedicated mode): what to handle next  
2. **Group list** — manual groups first, then auto path groups  
3. **Session row** — one per **tab**; name, agent chip (color), attention badge, user priority mark  
4. **Actions** — focus session; assign/remove manual group; set priority; (later) spawn  

### Interaction principles

- Scan in **seconds**; act in one click/key  
- **Color = agent class**; **badge/order = attention + user priority** (two channels)  
- Noise control: under-flag automated `needs_you`; user priority is explicit and sticky  
- Panel stays out of the way (toggle); does not steal the terminal experience  

## 4. Feature set

### 4.1 Session discovery (provider)

Each **terminal provider** can:

- List sessions (stable ids within that provider; **one session = one tab**)
- Report best-effort cwd / title
- Focus a session (bring to front / select that tab)
- Optionally: launch a session (FS13+, not core)

**Default provider: Kitty** (remote control). Core code depends on a **provider interface**, not Kitty APIs.

### 4.2 Path (auto) grouping

- Resolve session **cwd** (provider + `/proc` fallbacks as needed)
- **Git root** if available → group key / title = repo name  
- Else collapsed path (`~/…`)  
- Sessions with unknown cwd → *Unknown* / *Ungrouped* auto bucket  

Auto groups are derived; they do not require user setup.

### 4.3 Manual grouping

User-defined groups for **workstreams that span paths**.

Examples:

- *Trading* → projects/paths under `trading-brain`, `prop_challenges`, maybe a logs dir  
- *Inbox* → temporary holding group  

**Core behavior (frozen):**

- Create / rename / delete manual groups  
- Assign session → **at most one** manual group (path remains subtitle; no multi-tag in core)  
- Manual groups appear as first-class sections (**before** auto path groups; user order preserved)  
- Manually grouped sessions appear **once** under that group (not also under path)  
- Membership **persists** across restarts (provider id, then fingerprint; never silent ambiguous apply)

**Matching sessions after restart:** provider stable id when available; else fingerprint (cwd + title). Clear unmatched UX. Persistence must degrade gracefully.

### 4.4 Agent identity

| Class | Intent | Visual |
|-------|--------|--------|
| `claude` | Claude Code | Distinct hue |
| `grok` | Grok Build / grok CLI | Distinct hue |
| `kilo` | Kilo Code | Distinct hue |
| `codex` | Codex | Distinct hue |
| `shell` | Bare / unknown tool | Neutral |
| `other` | Other known long-runner | Muted |
| `unknown` | No signal | Neutral |

Source: process tree / cmdline associated with the session (+ optional title heuristics). Match rules documented and easy to extend.

### 4.5 Attention state (system)

Derived (heuristic), not perfect:

| State | Meaning |
|-------|---------|
| `needs_you` | Likely waiting for user input / approval |
| `working` | Agent active / recent output |
| `idle` | Quiet |
| `unknown` | Insufficient signal |

Signals (combined; prefer **precision** on `needs_you`):

1. Known agent in process tree  
2. TTY wait when observable  
3. Output/title heuristics (prompts, permission gates)  
4. Inactivity timers  

### 4.6 User priority (explicit)

User-set, sticky metadata on a session (persisted):

| Priority | Meaning |
|----------|---------|
| `normal` | Default |
| `important` | Elevated — easy to find; elevates queue ranking when active |
| `muted` | Still listed under groups; **never** enters the action queue (including `needs_you`) |

Three levels only in core. Numeric rank / snooze are later.

### 4.7 Action queue

An **ordered** list of sessions that deserve user attention **now** (frozen — D22/D23).

**Inclusion:**

- `needs_you` and not muted, **or**  
- `important` and attention ∈ {`needs_you`, `working`, `unknown`}  

**Not in queue:** `important` + `idle` (star in groups only); anything `muted`.

**Ordering:** important first → needs_you rank → age in state (oldest first) → display name.

**Actions:**

- Activate / focus item  
- Next / previous in queue  
- Set priority from queue or row  
- No separate “dismiss” state — clear by handling the terminal, muting, or heuristics leaving `needs_you`

### 4.8 Focus & navigation

- Click or key → provider **focus** that tab  
- Jump **next / previous** in the action queue (FS9)  
- Filter/search → FS10+

### 4.9 Spawn (later — not core)

Spawn shell/agent into a path or manual group via optional provider `launch`. Not required for FS1–FS9; trait may stub as unsupported.

## 5. Architecture

### 5.1 Layering

```
┌──────────────────────────────────────────────────────────┐
│  UI (GTK4 + libadwaita)                                  │
│  queue view · group view · actions                        │
└────────────────────────────┬─────────────────────────────┘
                             │
┌────────────────────────────▼─────────────────────────────┐
│  Domain / app core                                       │
│  Session · Group (auto|manual) · Priority · Attention    │
│  Queue ranking · Persistence · Agent classification      │
└───────────────┬────────────────────────────▲─────────────┘
                │ uses                       │ snapshots
┌───────────────▼─────────────┐   ┌──────────┴─────────────┐
│  TerminalProvider trait     │   │  Proc / host helpers   │
│  list · focus · (launch)    │   │  cwd, tree, cmdline    │
└───────────────┬─────────────┘   └────────────────────────┘
                │
        ┌───────▼────────┐
        │ KittyProvider  │  ← default, first implementation
        └────────────────┘
        │ future: WezTerm, Ghostty, GNOME Console, …
        ▼
```

**Rule:** UI and domain never call kitty remote control directly. Only a provider implementation does.

### 5.2 TerminalProvider (conceptual)

```text
TerminalProvider
  id() -> "kitty" | …
  list_sessions() -> [ProviderSession]
  focus(session_id) -> Result
  launch(request) -> Result<session_id>   # optional capability
  capabilities() -> { focus, launch, tabs, ... }
```

`ProviderSession` (provider-facing):

- `provider_id`, `session_id` (stable as far as provider allows)
- `title`, optional `cwd`
- enough handles to focus (implementation-defined, not leaked past adapter)

Domain maps provider sessions → **Session** entities, merges persistence (priority, manual group), runs classifiers, builds queue.

### 5.3 Stack (implementation default)

- **Rust** domain + binary  
- **GTK4 + libadwaita** UI  
- Single process OK (background refresh loop)  
- Persistence: local file under `~/.config/termorg/` (JSON or similar)

Stack can be revisited; **provider boundary and domain model cannot**.

### 5.4 Refresh

Poll ~0.5–1s or hybrid events. UI must tolerate partial provider failures (kitty down → clear error, no crash).

## 6. Build as digestible feature sets

Work ships as **small, completable feature sets** — each one leaves a usable increment. Do not combine sets to “go faster”; finish and **pass** one before starting the next.

Canonical detail (pass bars, dependencies, demos): **[`FEATURE-SETS.md`](./FEATURE-SETS.md)**.

### Order (summary)

| # | Feature set | In one line |
|---|-------------|-------------|
| **FS1** | Session pickup | See live terminal sessions from the default provider (Kitty) |
| **FS2** | Path grouping | Sessions cluster by project/location |
| **FS3** | Ops panel | Browse that inventory in a toggleable panel |
| **FS4** | Focus / jump | Open the real terminal for a listed session |
| **FS5** | Agent identity | Tell what tool is running by label/color |
| **FS6** | Manual groups | Build workstreams like *Trading* across several paths |
| **FS7** | Needs-you detection | See which sessions are waiting for input |
| **FS8** | User priority | Mark sessions important or muted |
| **FS9** | Action queue | Handle “what needs me” in a clear order |
| **FS10+** | Later | Search, notifications, spawn, second provider, polish |

### How a feature set “passes”

Passing is **capability-based**, described in plain language:

> After this set, a person should be able to …

not “function X returns Y.” Demo steps live in `FEATURE-SETS.md`.

Architecture constraints still apply throughout (especially **provider boundary** from FS1, so later providers do not require a rewrite).

## 7. UX details (frozen)

| Topic | Rule |
|-------|------|
| Session grain | One row per **tab** |
| Manual vs auto display | Manual groups first (user order). Unassigned → auto path groups only. |
| Session in manual group | Single row under that group; path as subtitle; no path-section duplicate |
| Multi-provider UI | Kitty only through FS9 |
| Empty queue | Calm empty state, not alarm |
| Hotkey | Default **Super+Shift+Space**; documented command fallback if global grab fails |

## 8. Success criteria (product)

End state (through **FS9**):

- Find any session by **path or manual workstream** in seconds  
- Answer **“what needs me next?”** from the action queue without scanning every window  
- Elevate important terminals deliberately; mute noise  
- Kitty works well today; adding another provider does not require rewriting UI/domain  
- Does not force abandoning kitty for actual typing/work  

Per-set pass bars: [`FEATURE-SETS.md`](./FEATURE-SETS.md).

## 9. Risks

| Risk | Mitigation |
|------|------------|
| Session identity unstable across restart | Soft fingerprints; re-link UX; don’t corrupt groups silently |
| `needs_you` false positives | Precision bias; user mute; tune after FS7 |
| Provider API gaps | Capability flags; degrade per provider |
| Scope blow-up | Strict FS order; each set must pass alone |
| Manual group UX complexity | One primary manual group per session; no nesting in core |

## 10. Out of scope (for now)

- Embedding terminal widgets as the main UI  
- Requiring tmux/zellij  
- Cloud sync of layout  
- Windows/macOS as primary targets  
- Automatic “smart” workstream inference (can be explored later; manual is source of truth)

## 11. Workflow experiment (secondary)

If used as a multi-workflow build experiment, arms must implement **this** frozen product definition. Experiment mechanics live in `EXPERIMENT.md` and must not shrink core features — only an arm’s *current feature-set cut* may be partial if explicitly labeled (e.g. “through FS4 only”).
