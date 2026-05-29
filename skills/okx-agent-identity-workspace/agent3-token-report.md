# Agent 3 — Token Efficiency Audit

## Token Counts (top 10 heaviest files, before optimization)

| File | Lines | ~Tokens |
|---|---|---|
| core/display-formats.md | 255 | ~5,794 |
| playbooks/provider.md | 240 | ~4,353 |
| core/display-lists.md | 172 | ~4,181 |
| troubleshooting.md | 86 | ~3,991 |
| modules/feedback.md | 179 | ~3,987 |
| SKILL.md | 227 | ~3,950 |
| core/display-detail.md | 232 | ~3,730 |
| playbooks/README.md | 186 | ~3,530 |
| playbooks/requester.md | 221 | ~3,526 |
| playbooks/evaluator.md | 196 | ~3,260 |

**Total across all 26 files: ~71,552 tokens**

---

## Redundancy Found

### A. Exact Duplicates (≥ 2 files with same text > 80 chars)

1. **Service-type footnote line** (`> Service types: API service = pay-per-call, fixed price; agent-to-agent = negotiated / off-chain pricing.`) appears in **4 files**: `core/display-formats.md`, `core/display-detail.md`, `core/display-lists.md`, `playbooks/provider.md`. This is by design (Pattern B footnote rendered on first occurrence per `core/ux-lexicon.md §Service-type`). Each is an independent rendered template — removing the duplicates would break rendering in those contexts.

2. **`⛔ After the visible line, this turn is NOT over.`** preamble blockquote — byte-identical in `playbooks/requester.md` and `playbooks/provider.md` (277 chars each).

3. **`#<id>` substitution rule source list** — items 1 and 3 are identical across all three role files (`playbooks/requester.md`, `playbooks/evaluator.md`, `playbooks/provider.md`); item 2 differs per role.

4. **`## STRICT — one question per turn`** section in role files — all three point to the same canonical rule in `playbooks/README.md §STRICT`, but `requester.md` (5 lines) and `evaluator.md` (3 lines) partially restate it rather than just pointing to it. `provider.md` (5 lines) has slightly more unique content (mentions service sub-fields) so it is kept as-is.

### B. Boilerplate

1. **`## Table of Contents`** in `playbooks/provider.md` — 13-line navigation table listing section names that are all already clear headings in a ~240-line file. Pure navigational overhead with no behavioral content.

2. **`## STRICT` mini-sections** in `requester.md` and `evaluator.md` — these 3–5 line sections just partially restate the 20-line canonical rule in `playbooks/README.md §STRICT`. With a pointer, the reader loads the full spec from the single canonical location.

### C. Cross-file Repetition

1. **Provider confirmation tables** (CN + EN, 2-service example) in `playbooks/provider.md §Confirmation` partially overlap with the single-service example in `core/display-detail.md §3 Create variant`. However, the provider.md version shows the 2-service + A2A pattern plus maintainer notes that are not in display-detail.md — therefore not safely removable.

2. **`Do NOT mention the okx-agent-chat/after-agent-list-changed.md path`** — byte-identical in `playbooks/requester.md` and `playbooks/provider.md` (single sentence, ~190 chars). Too small to compress without losing the behavioral cue at the point-of-use.

3. **`##Confirmation` intro blockquotes** in `requester.md` and `provider.md` — different wording, both pointing back to SKILL.md. Keep as-is.

---

## Optimizations Applied (with self-check results)

### 1. `playbooks/requester.md` — STRICT section compressed (5 lines → 2 lines)

**Removed:** "Every field is asked in its own message. Never list "请提供 1. Name 2. Description 3. ...". If the user volunteered multiple values in one sentence, you may capture them, but the confirmation table still renders each field on its own row. Field definitions live in `core/field-specs.md`. When prompting, inline the four segments..."

**Replaced with:** Single-line pointer to `playbooks/README.md §STRICT`

**Self-check:**
- "one question per turn" preserved in `playbooks/README.md` ✅
- "core/field-specs.md" preserved in `playbooks/README.md` ✅
- "four segments" preserved in `playbooks/README.md` ✅

### 2. `playbooks/evaluator.md` — STRICT section compressed (3 lines → 2 lines)

**Removed:** "Fields defined in `core/field-specs.md`. Inline the four segments (`用途 / 可见范围 / 请注意 / 示例` for Chinese; `Purpose / Visibility / Please note / Example` for English) when asking, in the user's language only."

**Replaced with:** Single-line pointer to `playbooks/README.md §STRICT`

**Self-check:**
- "one question per turn" preserved in `playbooks/README.md` ✅
- "core/field-specs.md" preserved in `playbooks/README.md` ✅
- "four segments" preserved in `playbooks/README.md` ✅

### 3. `playbooks/provider.md` — Table of Contents removed (13 lines)

**Removed:** 9-row navigation table listing STRICT / Phase 1 / Phase 2 / Good bad cases / Confirmation / Execute / Post-success / Error recovery / Endpoint Anti-Pattern

**Rationale:** All sections are clearly labeled `##` headings in a ~240-line file. The ToC adds navigational weight with zero behavioral content.

**Self-check:**
- All section headings verified present after removal ✅
- No rule content was part of the ToC ✅

---

## Lines Saved: ~21 lines total
## Estimated Token Savings: ~270 tokens (0.4%)

**Updated totals (after optimization):**
| File | Before | After | Saved |
|---|---|---|---|
| playbooks/requester.md | 221 lines, ~3,526 tok | 219 lines, ~3,440 tok | ~86 tok |
| playbooks/evaluator.md | 196 lines, ~3,260 tok | 196 lines, ~3,238 tok | ~22 tok |
| playbooks/provider.md | 240 lines, ~4,353 tok | 226 lines, ~4,191 tok | ~162 tok |
| **Total** | **~71,552** | **~71,282** | **~270** |

---

## Remaining Issues (not safe to remove automatically)

### High-value but structurally necessary duplicates

1. **Service-type footnote line** in 4 files — necessary in each context as a Pattern B rendered footnote; removing from any display file breaks template completeness.

2. **`⛔ After the visible line, this turn is NOT over.`** preamble in requester.md and provider.md — byte-identical (277 chars), but this is a P0 behavioral gate that must be present at the point of use in each role file. Consolidating to a pointer risks the model skipping the gate when loading only the role file.

3. **`#<id>` substitution rule** in all 3 role files — items 1 and 3 are identical (~180 chars combined). Item 2 varies per role. The entire block could theoretically reference `core/display-formats.md §#<id> placeholder rule` for items 1/3, keeping only role-specific item 2. However, this cross-file reference chain adds cognitive load and the risk of missed resolution outweighs the ~90-token saving per file.

4. **`Do NOT mention the okx-agent-chat/...` sentence** in requester.md and provider.md — too short to compress further (single sentence), and the point-of-use placement is intentional.

5. **`Description — do NOT prompt`** note in requester.md and evaluator.md — near-identical (~530 chars each) but role-specific noun substitutions make a shared pointer awkward without a template variable. The content is a critical behavioral constraint.

### Large files relative to information density

- **`core/display-formats.md`** (255 lines, ~5,794 tok): Dense but almost entirely unique canonical templates and rules. The §1 agent list section (~84 lines) is the main body; the remainder are moved-section notices and §4/§7/§8 templates. No safe compression found.

- **`modules/feedback.md`** (179 lines, ~3,987 tok): The Step 3 score-origin precondition blockquote (~31 lines) is a unique anti-hallucination rule with no equivalent elsewhere. The Step 2 ladder logic (~32 lines) is also unique. No safe compression found.

- **`troubleshooting.md`** (86 lines, ~3,991 tok): Dense table format — almost every character is unique error→translation mapping. The §2 backend errors section (17 rows) is the heaviest; each row is unique. No safe compression found.

### Architecture observation

The skill is well-structured with clear separation of concerns. The main token weight comes from:
1. Worked examples (CN + EN variants for every template) — necessary for the model to produce correct output
2. Anti-hallucination blockquotes — necessary for P0 behavioral constraints
3. Dual-language templates — language-matching is a core requirement

The 0.4% token savings achieved reflect that most content is genuinely unique and non-redundant. The skill has already undergone modular decomposition (display-detail.md / display-lists.md / cli-search-feedback.md split from their parents) which effectively controlled file size. Further compression would require accepting trade-offs in behavioral fidelity.
