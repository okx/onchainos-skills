# Progressive Disclosure — Token Consumption Model

How skills load content in stages, paying only for what the query needs.

## Flow

```
User Query
    │
    ▼
┌─────────────────────────────────────────────┐
│  Stage 0: Skill Routing                     │
│  Agent scans skill descriptions             │
│  Cost: ~150 tokens per description          │
│  (only the YAML frontmatter, not the body)  │
└─────────────┬───────────────────────────────┘
              │ matched
              ▼
┌─────────────────────────────────────────────┐
│  Stage 1: SKILL.md (always loaded)          │
│                                             │
│  ┌───────────┬────────┐                     │
│  │ Skill     │ Tokens │                     │
│  ├───────────┼────────┤                     │
│  │ market    │ 2,090  │                     │
│  │ signal    │ 2,099  │                     │
│  │ token     │ 2,510  │                     │
│  │ trenches  │ 1,606  │                     │
│  └───────────┴────────┘                     │
│                                             │
│  Contains: Commands table, param rules,     │
│  display rules, suggest next steps          │
└─────────────┬───────────────────────────────┘
              │
              ▼
┌─────────────────────────────────────────────┐
│  Stage 2: Pre-flight (always loaded)        │
│                                             │
│  preflight.md      1,056 tokens             │
│  chain-support.md     95 tokens             │
│  ─────────────────────────                  │
│  Subtotal:         1,151 tokens             │
│                                             │
│  Install check, version verify, rate limit  │
└─────────────┬───────────────────────────────┘
              │
    ┌─────────┴──────────┐
    │                    │
    ▼                    ▼
  English              Chinese (中文)
  query                query
    │                    │
    │         ┌──────────┴──────────────────┐
    │         │  Stage 3a: Keyword Glossary │
    │         │  (loaded on demand)         │
    │         │                             │
    │         │  ┌───────────┬────────┐     │
    │         │  │ Skill     │ Tokens │     │
    │         │  ├───────────┼────────┤     │
    │         │  │ market    │   300  │     │
    │         │  │ signal    │   444  │     │
    │         │  │ token     │   810  │     │
    │         │  │ trenches  │   525  │     │
    │         │  └───────────┴────────┘     │
    │         │                             │
    │         │  Maps Chinese terms →       │
    │         │  correct CLI commands        │
    │         └─────────────┬───────────────┘
    │                       │
    └───────────┬───────────┘
                │
                ▼
┌─────────────────────────────────────────────┐
│  Stage 3b: CLI Reference (loaded on demand) │
│  Agent greps for specific command section   │
│                                             │
│  Per-command grep: ~125 tokens              │
│                                             │
│  vs full file read (old behavior):          │
│  ┌───────────┬────────────────────┐         │
│  │ Skill     │ Full file (tokens) │         │
│  ├───────────┼────────────────────┤         │
│  │ market    │ 2,848              │         │
│  │ signal    │ 2,435              │         │
│  │ token     │ 5,680              │         │
│  │ trenches  │ 3,032              │         │
│  └───────────┴────────────────────┘         │
│                                             │
│  Saving: ~2,300–5,500 tokens per call       │
└─────────────┬───────────────────────────────┘
              │
              ▼  (signal skill only, WS scripts)
┌─────────────────────────────────────────────┐
│  Stage 4: WS Protocol (rare, on demand)     │
│                                             │
│  ws-protocol.md    1,634 tokens             │
│                                             │
│  Only loaded when user wants to write       │
│  a WebSocket subscription script            │
└─────────────────────────────────────────────┘
```

## Token Cost Summary

### Best case: English user, single command (e.g. "price of ETH")

| Stage | What | Tokens |
|---|---|---|
| 0 | Skill routing (description scan) | ~150 |
| 1 | SKILL.md | ~2,090 |
| 2 | Pre-flight + chain-support | 1,151 |
| 3b | CLI reference (1 command grep) | ~125 |
| | **Total** | **~3,516** |

### Typical case: English user, 2 commands

| Stage | What | Tokens |
|---|---|---|
| 0 | Skill routing | ~150 |
| 1 | SKILL.md | ~2,090 |
| 2 | Pre-flight + chain-support | 1,151 |
| 3b | CLI reference (2 command greps) | ~250 |
| | **Total** | **~3,641** |

### Chinese user, single command

| Stage | What | Tokens |
|---|---|---|
| 0 | Skill routing | ~150 |
| 1 | SKILL.md | ~2,090 |
| 2 | Pre-flight + chain-support | 1,151 |
| 3a | Keyword glossary | ~300 |
| 3b | CLI reference (1 command grep) | ~125 |
| | **Total** | **~3,816** |

### Worst case: Full exploration (all references loaded)

| Stage | What | Tokens (token skill) |
|---|---|---|
| 0 | Skill routing | ~150 |
| 1 | SKILL.md | 2,510 |
| 2 | Pre-flight + chain-support | 1,151 |
| 3a | Keyword glossary | 810 |
| 3b | CLI reference (full) | 5,680 |
| | **Total** | **~10,301** |

### Before vs After comparison

| Scenario | Before (eager load) | After (progressive) | Saved |
|---|---|---|---|
| English, 1 command | ~7,500 | ~3,500 | **53%** |
| English, 2 commands | ~7,500 | ~3,650 | **51%** |
| Chinese, 1 command | ~7,500 | ~3,800 | **49%** |
| Full exploration | ~10,300 | ~10,300 | 0% (no regression) |
