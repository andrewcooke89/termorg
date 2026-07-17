# Feature sets — build order and pass bars

Terminal Organiser is built as **digestible feature sets**.  
Each set is small enough to finish, demo, and judge before the next begins.

**Pass rule:** natural language only — *what a person can do*.  
If you cannot demo the “You should be able to…” list on a real machine, the set is not done.

**Architecture note:** From FS1, session access goes through a **terminal provider** abstraction, with **Kitty as the default provider**. Later sets must not bypass that boundary.

**Session grain:** one listed session = one **tab** (not one OS window with many tabs collapsed).

**Product freeze:** queue inclusion/order, grouping, and related rules are frozen in `DECISIONS.md` / `DATA-MODEL.md` for FS1–FS9.

---

## Overview

```
FS1 Session pickup
  → FS2 Path grouping
    → FS3 Ops panel
      → FS4 Focus / jump
        → FS5 Agent identity
          → FS6 Manual groups
            → FS7 Needs-you detection
              → FS8 User priority
                → FS9 Action queue
                  → FS10+ Later
```

| Set | Depends on | Leaves you with |
|-----|------------|-----------------|
| FS1 | — | A live inventory of terminal sessions |
| FS2 | FS1 | Inventory organized by project path |
| FS3 | FS2 | Same inventory in a real panel UI |
| FS4 | FS3 | Jump from panel to the actual terminal |
| FS5 | FS3 | Know what agent/tool is in each session |
| FS6 | FS4 | Cross-repo workstream groups (e.g. Trading) |
| FS7 | FS5 | See which sessions are waiting for you |
| FS8 | FS6 | Pin importance / mute noise (persists) |
| FS9 | FS7 + FS8 | Ordered action queue + step through it |
| FS10+ | FS9 | Polish and expansion |

FS5 may start once FS3 works (parallel to FS4 is fine).  
FS7 needs agent identity (FS5) to be meaningful.  
FS8 needs a place to attach priority (sessions in UI; best after FS6 so groups + stars coexist).  
FS9 needs both detection (FS7) and priority (FS8).

---

## FS1 — Session pickup

**Intent:** Simply pick up terminal sessions from the environment.

**Includes**

- Default **Kitty** provider wired behind a provider interface  
- List currently open sessions — **one entry per tab** — with a stable id and a readable name/title  
- Refresh so new/closed sessions appear without restarting the process  
- Sensible failure when Kitty/remote control is missing (“can’t see terminals” — not a crash)

**Out of this set:** grouping, UI panel polish, focus, agents, queue.

### Pass when you can…

1. Open several Kitty **tabs** (including across windows if you use them) and **see each tab listed** by the tool (CLI is enough).  
2. Close one tab and, after a short wait, **see it disappear** from the list.  
3. Open a new tab and **see it appear**.  
4. Understand from the output **which session is which** (title or similar).  
5. With Kitty remote control off/broken, get a **clear explanation**, not a stack trace or hang.

**Demo:** three tabs → list shows three → close one → list shows two → break remote control → clear error.

---

## FS2 — Path grouping

**Intent:** Organize sessions by **where they are** (folder / project).

**Includes**

- Resolve each session’s working directory (best effort)  
- Prefer **git root** as the project boundary when available  
- Otherwise group by a readable collapsed path  
- Unknown location goes to a clear bucket (e.g. “Unknown”)

**Out of this set:** manual groups, pretty panel (plain grouped dump is fine).

### Pass when you can…

1. Open shells in **two different git repos** and see them under **two different project groups**.  
2. Open two tabs in the **same repo** (different subdirs ok) and see them under the **same** group.  
3. Open a shell **outside any git repo** and still see a **sensible path-based** group.  
4. Glance at the list and answer: **“which projects have terminals open?”** without reading every path character-by-character.

**Demo:** repo A + repo B + `~/scratch` → grouped listing matches reality.

---

## FS3 — Ops panel

**Intent:** Use a real **panel UI** as the place you look — not only a CLI dump.

**Includes**

- Toggleable panel (hotkey and/or simple launch command)  
- Shows sessions **under path groups** from FS2  
- Updates live (or near-live) as sessions change  
- Calm layout: groups and rows readable at a glance  

**Out of this set:** click-to-focus (FS4), agent colors (FS5), manual groups.

### Pass when you can…

1. **Open the panel** without hunting through a terminal log.  
2. **Hide it** and bring it back easily.  
3. See the **same path grouping** as FS2, but scannable as a UI.  
4. Leave terminals running, create/close tabs, and **watch the panel catch up** without restarting the app.  
5. Use it as the place you’d actually look when you have many sessions open.

**Demo:** hotkey/command → panel with groups → new tab appears → hide panel.

---

## FS4 — Focus / jump

**Intent:** The panel is not just a mirror — it **takes you** to the terminal.

**Includes**

- Activate a session (click or keyboard) → that Kitty tab/window comes to the front  
- Works for sessions in different OS windows if that’s how you work  

**Out of this set:** queue navigation, spawn new terminals.

### Pass when you can…

1. From the panel, pick a session in the background and **land in that terminal ready to type**.  
2. Jump to a **different** session the same way and land in the right place.  
3. Trust that “the row I chose” is **the terminal I got** (no wrong-tab surprises in normal use).

**Demo:** two projects visible → focus A → focus B → confirm each time.

---

## FS5 — Agent identity

**Intent:** Know **what is running** in each session without reading the scrollback.

**Includes**

- Classify into at least: Claude, Grok, Kilo, Codex, shell/other/unknown  
- **Distinct color + short label** per known agent class  
- Updates when you start/stop an agent in a tab  

**Out of this set:** whether they need input (FS7).

### Pass when you can…

1. Start Claude in one tab, Codex (or another agent) in another, leave a bare shell — and **tell them apart instantly** from the panel.  
2. Stop the agent and return to a shell — and see the row **go back to shell/neutral**.  
3. Answer “where is my Claude?” by **color/label + group**, not by alt-tabbing through windows.

**Demo:** three tabs (claude / other agent / shell) → panel shows three clear identities.

---

## FS6 — Manual groups

**Intent:** Group by **how you think about work**, not only by folder — e.g. *Trading* spanning several repos.

**Includes**

- Create, rename, **delete** a manual group  
- Assign / remove a session (tab) to a manual group — **one tab at a time**, not whole OS window  
- **Delete group:** removes the group and **unassigns** any tabs in it; does **not** close or destroy terminals/tabs  
- Manual groups appear as sections in the panel (user-sensible order)  
- A manually grouped session appears **once** under that group (path as secondary context)  
- Unassigned sessions still sit under path groups  
- **Survives restart** of the app (best-effort reattach to the same live sessions)

**Out of this set:** action queue, automatic “needs you.”

### Pass when you can…

1. Create a group called **Trading** (or similar).  
2. Put terminals from **two different project paths** into it.  
3. Open the panel and **see those sessions together** under Trading, not only under separate path headers.  
4. Restart the app and find **Trading still there** with sessions reattached (or clearly marked if a session vanished).  
5. Remove a session from Trading and see it **return to path grouping**.  
6. **Delete** the Trading group: group is gone; former members reappear under path groups; **tabs still exist**.

**Demo:** `trading-brain` + `prop_challenges` tabs → assign both → restart → still grouped → delete group → tabs remain under path groups.

---

## FS7 — Needs-you detection

**Intent:** Detect when a session is **waiting for your input** vs busy vs quiet.

**Includes**

- System states: needs you / working / idle / unknown (wording in UI can be friendlier)  
- Visible badge or equivalent on each row  
- Bias toward **not crying wolf** on “needs you”  
- Updates as the session’s behavior changes (within a short time)

**Out of this set:** ordered queue (FS9); user pin/mute (FS8).

### Pass when you can…

1. Leave an agent **clearly waiting** for you (permission prompt, “your turn”, etc.) and see the panel mark it as **needing you**.  
2. While an agent is **obviously busy**, see it as **working** (or at least not “needs you”).  
3. Leave a **quiet shell** and see it as **idle** (or equivalent calm state).  
4. Trust the signal enough that you **look at “needs you” first** when you come back to the machine — even if it is not perfect.

**Demo:** one waiting agent, one busy, one idle shell → three different, sensible markings.

---

## FS8 — User priority

**Intent:** You decide what is **important** or **background noise**.

**Includes**

- Mark a session **important**, **normal**, or **muted**  
- Clear visual for important (and muted) in the group list  
- Preference **persists** across app restart (best-effort session match)  
- Muted sessions stay findable under groups but feel de-emphasized  

**Out of this set:** full action queue (FS9 uses these marks).

### Pass when you can…

1. Star/mark a terminal **important** and spot it immediately in a crowded panel.  
2. **Mute** a noisy but non-urgent session so it stops competing for attention.  
3. Restart the app and see those marks **still applied** to the right sessions (or honest “couldn’t reattach”).  
4. Change your mind (important → normal) in one action.

**Demo:** mark one important, one muted → restart → marks hold → clear important.

---

## FS9 — Action queue

**Intent:** An **ordered queue** of what to handle next — not just badges on a flat list.

**Includes**

- Dedicated queue section or mode at the top of the workflow  
- Inclusion/order per frozen rules (D22/D23): needs-you (not muted); important when not idle; mute always out; idle+important out of queue  
- Focus a queue item; move to **next** / **previous** in the queue  

**Out of this set:** notifications, auto-dismiss policies beyond natural state changes.

### Pass when you can…

1. With several sessions open, open the panel and **see a short ordered list of what needs you**.  
2. Answer **“what should I handle first?”** from the queue alone.  
3. Jump to item 1, deal with it (or switch away), then go to **next** without re-scanning all groups.  
4. Mark something important (and not idle) and see it **rise appropriately** in the queue.  
5. Mute something that needs you and see it **drop out** of the queue (mute always suppresses queue).  
6. Confirm an **important but idle** session is **starred in groups** but **not** in the queue.

**Demo:** two needs-you + one important working + one muted needs-you + one important idle → queue matches frozen rules → next/focus.

---

## FS10+ — Later feature sets (planned, not core gate)

Each should get the same treatment when started: short intent + “you should be able to…”.

| Set | Intent (sketch) | Pass sketch |
|-----|-----------------|-------------|
| **FS10 Search / filter** | Find a session by name/path/agent when the list is long | Type a few characters and only matching sessions remain |
| **FS11 Notifications** | Optional alert when something newly needs you | Be away from the panel and still learn that something needs you (without spam) |
| **FS12 Ambient Kitty cues** | Tab title/color reflects agent/attention without the panel open | Glance at Kitty tabs and get the same story as the panel |
| **FS13 Spawn / launch** | Start a shell or agent into a path or manual group from the panel | Create a new session in the right project without leaving the panel |
| **FS14 Second provider** | Same product over another terminal emulator | Unplug the “Kitty-only” assumption in daily use for at least list+focus |
| **FS15 Path hints for manual groups** | Suggest “this path often belongs in Trading” | Accept a suggestion instead of only manual assign |

Do not start FS10+ until FS9 passes, unless explicitly prototyping off-path.

---

## Cross-cutting rules for every set

1. **No silent architecture regressions** — provider boundary stays intact.  
2. **Degrade honestly** — if something cannot be known (cwd, agent, reattach), say so in the UI/CLI.  
3. **Prefer a thin vertical slice** — working path on real Kitty sessions beats elaborate structure.  
4. **One set at a time** — unfinished sets do not count as passed because a later set “kind of includes them.”  
5. **Demo on the real machine** — fixtures can help, but pass is lived experience with your terminals.

---

## Progress checklist

| Set | Status | Passed date | Notes |
|-----|--------|-------------|-------|
| FS1 Session pickup | not started | | |
| FS2 Path grouping | not started | | |
| FS3 Ops panel | not started | | |
| FS4 Focus / jump | not started | | |
| FS5 Agent identity | not started | | |
| FS6 Manual groups | not started | | |
| FS7 Needs-you detection | not started | | |
| FS8 User priority | not started | | |
| FS9 Action queue | not started | | |
| FS10+ | later | | |
