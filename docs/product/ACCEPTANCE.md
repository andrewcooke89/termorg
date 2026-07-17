# Acceptance

Feature-set passing is defined in plain language in:

→ **[`FEATURE-SETS.md`](./FEATURE-SETS.md)**

That document is authoritative for “is this increment done?”

Product rules (queue, grouping, session grain, etc.) are frozen in **`DECISIONS.md`**.

### Quick reference

| Done means… | Feature set |
|-------------|-------------|
| I can see my live **tabs** listed | FS1 |
| They cluster by project/path | FS2 |
| I browse them in a panel | FS3 |
| I can jump to the real terminal | FS4 |
| I can see what agent is running | FS5 |
| I can build a *Trading*-style group across repos | FS6 |
| I can see what is waiting for my input | FS7 |
| I can mark important / muted | FS8 |
| I have an ordered action queue and can step through it | FS9 |

Do **not** treat unit-test names or internal function contracts as the pass bar.  
Tests may support a set; they do not replace the “you should be able to…” demo.
