# Data model (core)

Logical model for domain code. Persistence format is an implementation detail; fields below are the contract.

**Aligned with frozen decisions** in `DECISIONS.md` (especially D4–D9, D21–D24).

**Session grain:** one domain session = one terminal **tab** (D21).

---

## ProviderSession (from provider adapter)

| Field | Notes |
|-------|--------|
| `provider` | e.g. `"kitty"` |
| `id` | Provider-local stable id (**tab** id for Kitty) |
| `title` | Best-effort |
| `cwd` | Best-effort absolute path |
| `focus_handle` | Opaque to domain (or mapped only inside provider) |

## Session (domain, live)

Merged live provider data + persisted user metadata.

| Field | Notes |
|-------|--------|
| `key` | `{provider, id}` while live |
| `display_name` | title or collapsed cwd |
| `cwd` | optional |
| `auto_group_id` | derived: git root path or collapsed cwd key |
| `manual_group_id` | optional; user assignment |
| `agent` | `claude\|grok\|kilo\|codex\|shell\|other\|unknown` |
| `attention` | `needs_you\|working\|idle\|unknown` |
| `priority` | `normal\|important\|muted` (persisted) |
| `attention_since` | timestamp when current attention entered (for queue age) |
| `last_seen` | last successful list |

## AutoGroup (derived, not necessarily persisted)

| Field | Notes |
|-------|--------|
| `id` | stable string: git root path or path key |
| `title` | repo name or collapsed path |
| `kind` | `auto_path` |

## ManualGroup (persisted)

| Field | Notes |
|-------|--------|
| `id` | uuid or slug |
| `title` | e.g. `"Trading"` |
| `sort_index` | user order among manual groups |
| `path_hints` | optional list of path prefixes that **suggest** assignment (not auto-force in v1 unless enabled) |

**v1 assignment model:** session → at most **one** `manual_group_id`.  
Path hints are optional helpers later; v1 can be pure drag/assign without hints.

## PersistedUserState

| Field | Notes |
|-------|--------|
| `manual_groups` | list of ManualGroup |
| `session_prefs` | list of SessionPref |
| `schema_version` | int |

### SessionPref (persisted identity)

Used to re-apply priority / manual group after restart.

| Field | Notes |
|-------|--------|
| `provider` | |
| `match` | strategy payload (see below) |
| `manual_group_id` | optional |
| `priority` | |
| `updated_at` | |

**Match strategies (v1):**

1. `provider_id` — exact provider session id if still valid  
2. `fingerprint` — `{ cwd, title_normalized }` last resort  

When multiple live sessions match one pref, apply to best match; leave others default; do not clone priority to all without user action.

## Attention queue entry

Not stored; computed:

```text
Queue = filter(sessions, include_rules)
        .sort_by(priority_rank, attention_rank, attention_since asc, name)
```

### include_rules (frozen v1 — D22)

```text
include if:
  priority != muted AND attention == needs_you
  OR
  priority == important AND attention in {needs_you, working, unknown}
```

Implications:

- `important` + `idle` → **excluded** (star in group list only)  
- `muted` + `needs_you` → **excluded** (mute suppresses queue)

### ranks / order (frozen v1 — D23)

```text
priority_rank: important=0, normal=1, muted=2
attention_rank: needs_you=0, working=1, unknown=2, idle=3
then: attention_since ascending (oldest first)
then: display_name
```

---

## Provider trait (Rust-shaped sketch)

```rust
// Conceptual — not a final API surface
trait TerminalProvider: Send + Sync {
    fn provider_id(&self) -> &str;
    fn capabilities(&self) -> Capabilities;
    fn list_sessions(&self) -> Result<Vec<ProviderSession>, ProviderError>;
    fn focus(&self, session_id: &str) -> Result<(), ProviderError>;
    fn launch(&self, req: LaunchRequest) -> Result<String, ProviderError> {
        Err(ProviderError::Unsupported)
    }
}
```

Host/proc helpers stay **outside** the trait (shared): resolve cwd from pid, walk process tree, classify agent — provider supplies identity + focus + whatever pid/cwd it can.

---

## UI binding (views)

| View | Source |
|------|--------|
| Action queue | computed queue (D22/D23) |
| Manual group sections | `manual_groups` ordered + sessions with that id |
| Auto-only sections | sessions with `manual_group_id == None`, grouped by `auto_group_id` |
| Row | Session fields + actions (focus, set priority, assign group) |
