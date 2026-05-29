# okx-agent-identity — Architecture & Efficiency Report

Generated: 2026-05-29  
Skill path: `skills/okx-agent-identity`  
Total .md files: 26 (excluding workspace/iteration dirs)  
Total bytes: 311,875 (~78,000 tokens at ~4 bytes/token estimate)

---

## Self-Check Loop Results

### Loop 1 (Broken paths): Round 1 — PASS

Command run:
```
python3 -c "... re.finditer backtick-ref check ..."
```
Result: `PASS: 0 broken`

All internal `.md` references resolve. Cross-skill references (`after-agent-list-changed.md`, `okx-agent-task/references/evaluator-staking.md`) are correctly exempted.

---

### Loop 2 (core/ upward refs): Round 1 — PASS

Command run:
```
grep -rn "playbooks/\|modules/" .../core/ | grep "\`.*playbooks/\|\`.*modules/"
```
Result: empty output — no upward references from `core/` to `playbooks/` or `modules/`.

`core/` files reference only peer `core/` files and `troubleshooting.md`. Correct layering maintained.

---

### Loop 3 (Reference depth <= 2): Round 1 — PASS

BFS from `SKILL.md`. Result: **Max depth = 1**.

Every file referenced in the skill is reachable in exactly 1 hop from `SKILL.md`. The `SKILL.md` Resources section explicitly lists all 26 files, making them all depth-1. This is optimal — no depth-2 or deeper files exist in the graph (from the perspective of the SKILL.md anchor).

---

### Loop 4 (No orphans): Round 1 — PASS

Result: `0 orphans`

Every `.md` file (excluding workspace) has in-degree >= 1. All files are reachable from at least one other file.

---

## Reference Graph Summary

The graph is intentionally flat (all resources listed directly in `SKILL.md §Resources`), with a secondary hub structure:

**High in-degree nodes (heavily referenced — core infrastructure):**
| File | Approximate in-degree |
|---|---|
| `SKILL.md` | ~20 (back-refs from almost every file) |
| `core/ux-lexicon.md` | ~15 |
| `core/display-formats.md` | ~14 |
| `troubleshooting.md` | ~12 |
| `core/cli-reference.md` | ~8 |
| `core/field-specs.md` | ~5 |
| `core/choice-prompts.md` | ~5 |

**Note on back-references to `SKILL.md`:** Many files in `playbooks/`, `modules/`, and `core/` reference `SKILL.md` (e.g., `SKILL.md §Step 3`, `SKILL.md §UX Output Red Lines`). This is an intentional cross-reference pattern — these files cite canonical rules that live in `SKILL.md` rather than duplicating them. This is correct architecture; however it means these files have implicit circular dependencies with `SKILL.md`. Since `SKILL.md` is always loaded first (it IS the skill entry), these back-refs are safe.

---

## Loading Analysis

Token cost estimate methodology: bytes / 4 (rough English-prose average; Chinese content is denser, so actual token counts for files with heavy Chinese may be ~20% higher).

### Operation 1: Provider Registration (most expensive path)

**Files loaded:**
1. `SKILL.md` (16,799 B / ~4,200 tok) — entry, routing, gates, command index
2. `playbooks/README.md` (17,129 B / ~4,282 tok) — pre-check rules, role router, confirmation card spec, one-Q-per-turn rule
3. `playbooks/provider.md` (19,330 B / ~4,833 tok) — Phase 1 Q&A, confirmation, post-success, endpoint anti-pattern
4. `playbooks/provider-services.md` (13,014 B / ~3,254 tok) — Phase 2 service Q&A loop
5. `core/field-specs.md` (11,554 B / ~2,889 tok) — four-segment field specs inlined per Q
6. `core/ux-lexicon.md` (12,727 B / ~3,182 tok) — term translations (loaded per Red line 1)
7. `core/display-formats.md` (25,005 B / ~6,251 tok) — confirmation card + detail card templates
8. `core/display-detail.md` (16,580 B / ~4,145 tok) — §3 confirmation/diff card
9. `core/cli-create.md` (9,412 B / ~2,353 tok) — create params, return schema, agentId algorithm
10. `core/choice-prompts.md` (3,790 B / ~948 tok) — numbered options pattern
11. `core/cost-disclosure.md` (2,631 B / ~658 tok) — gas policy (P0, loaded before any mutation)
12. `modules/avatar-upload.md` (6,453 B / ~1,613 tok) — avatar Q decision matrix (Q3 in Phase 1)
13. `modules/pre-listing-qa.md` (11,036 B / ~2,759 tok) — pre-listing QA before activate (if provider runs activate after create)
14. `_shared/preflight.md` (4,913 B / ~1,228 tok) — wallet preflight checks
15. `_shared/no-polling.md` (8,885 B / ~2,221 tok) — one-call discipline
16. `troubleshooting.md` (18,085 B / ~4,521 tok) — error translations (loaded on error)
17. `playbooks/consent.md` (5,099 B / ~1,275 tok) — consent card (if first-time create triggers consent)

**Core path (without error/consent branches):** files 1–15  
Core path total: ~168,508 B / **~42,127 tokens**

**With error handling + consent:** add files 16–17  
Full path total: ~191,692 B / **~47,923 tokens**

---

### Operation 2: Requester Registration (fast path)

**Files loaded:**
1. `SKILL.md` (~4,200 tok)
2. `playbooks/README.md` (~4,282 tok)
3. `playbooks/requester.md` (15,507 B / ~3,877 tok)
4. `core/field-specs.md` (~2,889 tok)
5. `core/ux-lexicon.md` (~3,182 tok)
6. `core/display-formats.md` (~6,251 tok)
7. `core/display-detail.md` (~4,145 tok)
8. `core/cli-create.md` (~2,353 tok)
9. `core/choice-prompts.md` (~948 tok)
10. `core/cost-disclosure.md` (~658 tok)
11. `modules/avatar-upload.md` (~1,613 tok)
12. `_shared/preflight.md` (~1,228 tok)
13. `_shared/no-polling.md` (~2,221 tok)

Total: ~136,649 B / **~37,848 tokens** (core path, no error handling)

---

### Operation 3: Agent Search (lightest read-only path)

**Files loaded:**
1. `SKILL.md` (~4,200 tok)
2. `modules/agent-search.md` (11,311 B / ~2,828 tok) — verbatim passthrough rules, 4-dimension split, worked examples
3. `core/cli-search-feedback.md` (12,415 B / ~3,104 tok) — search command params + return schema
4. `core/display-lists.md` (17,864 B / ~4,466 tok) — §6 search result display format
5. `core/ux-lexicon.md` (~3,182 tok)
6. `_shared/no-polling.md` (~2,221 tok)

Total: ~88,207 B / **~19,201 tokens** — lightest non-trivial path

---

### Operation 4: Feedback Submit

**Files loaded:**
1. `SKILL.md` (~4,200 tok)
2. `modules/feedback.md` (17,079 B / ~4,270 tok) — full 7-step decision tree, star validation, confirmation
3. `core/cli-search-feedback.md` (~3,104 tok) — feedback-submit params
4. `core/display-formats.md` (~6,251 tok) — confirmation card template
5. `core/display-detail.md` (~4,145 tok) — diff card
6. `core/ux-lexicon.md` (~3,182 tok)
7. `core/choice-prompts.md` (~948 tok) — used when creator-id selection is multi-agent
8. `core/cli-reference.md` (12,959 B / ~3,240 tok) — `agent get` for creator-id ladder 2
9. `_shared/no-polling.md` (~2,221 tok)
10. `troubleshooting.md` (~4,521 tok) — error handling

Total: ~134,044 B / **~31,082 tokens**

---

### Operation 5: View Agent List / Detail (pure read)

**Files loaded:**
1. `SKILL.md` (~4,200 tok)
2. `core/cli-reference.md` (~3,240 tok) — get command schema
3. `core/display-formats.md` (~6,251 tok) — §1 list format
4. `core/display-detail.md` (~4,145 tok) — §2 detail card (if user drills into one)
5. `core/ux-lexicon.md` (~3,182 tok)

Total: ~82,670 B / **~20,667 tokens** — cheapest useful path

---

### Operation 6: Evaluator Registration

**Files loaded:**
1. `SKILL.md` (~4,200 tok)
2. `playbooks/README.md` (~4,282 tok)
3. `playbooks/evaluator.md` (14,216 B / ~3,554 tok)
4. `core/field-specs.md` (~2,889 tok)
5. `core/ux-lexicon.md` (~3,182 tok)
6. `core/display-formats.md` (~6,251 tok)
7. `core/display-detail.md` (~4,145 tok)
8. `core/cli-create.md` (~2,353 tok)
9. `core/choice-prompts.md` (~948 tok)
10. `core/cost-disclosure.md` (~658 tok)
11. `modules/avatar-upload.md` (~1,613 tok)
12. `_shared/preflight.md` (~1,228 tok)
13. `_shared/no-polling.md` (~2,221 tok)
14. Cross-skill: `okx-agent-task/references/evaluator-staking.md` (loaded post-create via Step 5)

Total (identity side only): ~124,949 B / **~35,737 tokens**

---

## Lazy-Load Candidates

The following files are only needed in specific branches and could be deferred until those branches are actually entered. Currently all files listed in `SKILL.md §Resources` are available at read time, which is intentional given the flat-depth architecture. However from a pure token-efficiency standpoint:

| File | Size | When actually needed | Current load point |
|---|---|---|---|
| `playbooks/consent.md` (5,099 B) | ~1,275 tok | Only when `agent create` returns non-null `consent` field — rare first-time-only event | Listed in SKILL.md (depth 1) — loaded on demand per Consent Gate |
| `modules/pre-listing-qa.md` (11,036 B) | ~2,759 tok | Only when provider runs `agent activate` (not on initial create, which is auto-active) | Listed in SKILL.md; `activate` branch only |
| `modules/avatar-upload.md` (6,453 B) | ~1,613 tok | Only when user reaches the picture Q (Q3 in create flow, or standalone upload) | Loaded at Q3 turn |
| `cross-skill-workflows.md` (6,215 B) | ~1,554 tok | Reference documentation only; not needed for any live operation | Listed in SKILL.md; never explicitly loaded during operations |
| `playbooks/consent.md` | See above | First-time consent only | Low risk — small file |
| `troubleshooting.md` (18,085 B) | ~4,521 tok | Only on CLI error | Currently loaded at error time (correct) |

The existing on-demand loading discipline is already correct for most files. The architecture does NOT bulk-load all 26 files upfront — it loads them progressively per operation branch. No structural change needed here.

---

## Findings & Recommendations

### Finding 1: Architecture is sound — flat depth is intentional and correct

All four loop checks pass on the first round. The flat BFS depth-1 design (all resources listed directly in `SKILL.md §Resources`) is deliberate: it gives the AI a complete manifest at skill load time without requiring multi-hop traversal. This avoids the "forgot to follow the chain" failure mode.

### Finding 2: `_shared/no-polling.md` has back-refs to `SKILL.md` that create nominal cycles

`_shared/no-polling.md` references `SKILL.md` (3 times), `core/display-formats.md`, and several other files. Since `SKILL.md` also references `_shared/no-polling.md`, there is a nominal cycle. This is benign (both are loaded at skill-init time, neither is conditionally deferred), but the back-refs in `no-polling.md` are redundant — the rules it cites are enforced by `SKILL.md`, not by `no-polling.md` reading `SKILL.md` again. Recommendation: strip the `SKILL.md` back-references from `_shared/no-polling.md` on the next refactor pass to reduce confusion. (No functional change required now.)

### Finding 3: `core/display-formats.md` is the largest single file and most widely referenced

At 25,005 B (~6,251 tokens), `core/display-formats.md` is the heaviest single file and has the highest in-degree among non-SKILL.md files (~14 references). It covers 8 sections (§1–§8) covering list, detail, diff, service-list, feedback-list, search, error, and post-success templates. For operations that only need §6 (search), §5 (feedback-list), or §1 (agent list), the entire file is still loaded.

Recommendation (future): split `core/display-formats.md` into two files:
- `core/display-formats-create.md` — §1, §2, §2.5, §3 (create/update/get flows)
- `core/display-formats-query.md` — §4, §5, §6, §7, §8 (search, feedback, service-list, error, post-success)

This would save ~3,000 tokens on pure search/read-only paths. However, the global-rules section (untrusted content, Pattern B service types, URL literals, `#<id>` placeholder rule) must be retained in both files or extracted to a shared preamble. This is a non-trivial refactor — defer unless token pressure is observed in production.

### Finding 4: Provider registration is the most token-expensive operation (~42–48K tokens)

The provider create path (SKILL.md + README + provider + provider-services + field-specs + ux-lexicon + display-formats + display-detail + cli-create + choice-prompts + cost-disclosure + avatar-upload + pre-listing-qa + preflight + no-polling) accounts for roughly 42K tokens of context before any user input. This is expected given the complexity of Phase 1 + Phase 2 Q&A loops. No optimization is recommended without first observing context-window pressure in practice.

### Finding 5: `cross-skill-workflows.md` is a documentation-only file with no live operation path

`cross-skill-workflows.md` (6,215 B) documents Workflows A–D but is never loaded as part of any live operation step — it's purely a human-readable reference. It is listed in `SKILL.md §Resources`, which is correct (it should be discoverable), but no operation flow step points to it. This is fine as-is.

### Finding 6: No broken refs, no upward-leaking core/ files, no orphans

The reference graph is clean. The `core/` directory correctly contains only downward-pointing or peer references (no refs to `playbooks/` or `modules/`). All 26 files have at least one incoming reference. No file is unreachable from `SKILL.md`.

---

## Summary Table

| Check | Result | Round Fixed |
|---|---|---|
| Loop 1 — Broken paths | PASS (0 broken) | Round 1 |
| Loop 2 — core/ upward refs | PASS (0 violations) | Round 1 |
| Loop 3 — Reference depth | PASS (max depth = 1) | Round 1 |
| Loop 4 — Orphans | PASS (0 orphans) | Round 1 |

| Operation | Files loaded | Approx tokens |
|---|---|---|
| Provider registration (core path) | 15 files | ~42,127 |
| Provider registration (full incl. errors + consent) | 17 files | ~47,923 |
| Requester registration | 13 files | ~37,848 |
| Evaluator registration | 13 files + 1 cross-skill | ~35,737 |
| Feedback submit | 10 files | ~31,082 |
| Agent search | 6 files | ~19,201 |
| Agent list/detail (read-only) | 5 files | ~20,667 |
