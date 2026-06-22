# Skill File Session-Split Implementation Plan

## Goal
Reduce token consumption per task lifecycle by ~1100L, targeting three independent optimizations.

---

## Solution 1: buyer.md — delete user-session-only content (-78L)

### Rationale
buyer.md is a sub-session file (backup + job). Sections §3.1/§3.2/§3.3/Routing-table/Resolve-rule are user-session content already duplicated in buyer-user.md. Sub sessions never execute these flows.

### Changes

**buyer.md:**
1. Quick Navigation table: remove rows `§3.1`, `§3.2–3.3`, `§3.6.1–3.8`
2. Delete body content: §3.1 redirect → §3.3 x402 + error handling → routing table → resolve rule (L86–L176, ~90 lines)
3. Delete body content: §3.6.1+3.7+3.8 redirect (3 lines)

**No other files affected** — buyer-user.md already has identical copies of all deleted content.

---

## Solution 2: buyer-actions.md split + display-formats §3 inline (-460L for publish flow)

### Rationale
buyer-actions.md (290L) bundles 4 unrelated operations. Publishing (§1, 114L) is the most frequent. Model reads the full file every time.

### Changes

**New file: `buyer-actions-publish.md` (~154L)**
- Source: buyer-actions.md §1 (L1–L139) + display-formats.md §3 template inlined as appendix
- Contains: pre-validation → confirmation form → create-task → error handling → draft operations → confirmation card template

**Modified: `buyer-actions.md` (~155L)**
- Remove §1, keep §2 Attachment + §3 Terms + §4 Deliverables
- Update Quick Navigation to reflect remaining sections only
- Keep preamble (pre-requisite / localization / confirmation rule)

**Reference updates (8 places):**

| File | Line | Change |
|---|---|---|
| `SKILL-user.md` L23 | `buyer-actions.md` → `buyer-actions-publish.md` + `buyer-actions.md` |
| `SKILL-user.md` L72 | Reading Order: update description |
| `SKILL-user.md` L96 | `buyer-actions.md §1` → `buyer-actions-publish.md` |
| `buyer-user.md` L19 | `buyer-actions.md §1` → `buyer-actions-publish.md` |
| `buyer-user.md` L42 | `buyer-actions.md §1` → `buyer-actions-publish.md` |
| `buyer-user.md` L91 | `buyer-actions.md §1` → `buyer-actions-publish.md` |
| `buyer-user.md` L92 | `buyer-actions.md §1.4` → `buyer-actions-publish.md §1.4` |
| `SKILL.md` L333 | `buyer-actions.md §1` → `buyer-actions-publish.md` |

buyer.md references are already gone after Solution 1.

---

## Solution 3: SKILL.md — backup session fast-path (+5L instruction)

### Rationale
Backup session loads SKILL.md (404L) but only needs Activation + sessionKey Discrimination (~90L). Adding a fast-path instruction tells the model to skip irrelevant sections.

### Changes

**SKILL.md — Reading Order section:**
Insert before existing item 1:
```
> **Backup session shortcut** (sessionKey contains `:backup:`):
> Read ONLY: `Activation` + `sessionKey Discrimination` + `Anti-hallucination` (≤90L).
> Then load `buyer-backup.md` (future) or the role file's backup-relevant sections.
> Skip: Session Communication Contract, Communication Boundary, User Intent Routing, Cross-Skill Routing.
```

---

## Execution Order

All three are independent — can execute in parallel.

1. **Solution 1** → edit buyer.md (delete)
2. **Solution 2** → create buyer-actions-publish.md, edit buyer-actions.md, update 8 references
3. **Solution 3** → edit SKILL.md Reading Order

## Verification

```bash
# All references point to valid files
grep -rn "buyer-actions-publish\|buyer-actions\.md" skills/okx-agent-task/ --include="*.md"

# No broken section references
grep -rn "buyer-actions\.md.*§1" skills/okx-agent-task/ --include="*.md"
# Should return 0 results after implementation

# buyer.md no longer contains user-session routing
grep -n "Designated-Provider\|Publishing a task\|Resolve.*execution\|routing table" skills/okx-agent-task/buyer.md
# Should return 0 results
```
