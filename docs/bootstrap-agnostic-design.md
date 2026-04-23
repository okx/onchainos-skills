# Bootstrap & State Management — Pinata vs OnchainOS Design, and the Agnostic Path

## 1. The Core Problem

| | Pinata (OpenClaw) | OnchainOS Bootstrap Design Doc |
|---|---|---|
| **Runtime** | OpenClaw only | Claude Code, Cursor, Codex, OpenClaw, custom agents |
| **Boot control** | Platform owns the boot — runtime force-loads `BOOTSTRAP.md` before LLM starts | No runtime control — must rely on agent calling a tool |
| **State storage** | Markdown workspace files | JSON files via MCP tools (schema-enforced in Rust) |
| **Schema enforcement** | Prompt-level (LLM interprets .md) | Rust struct typing in MCP server |
| **Concurrency** | Serial session (OpenClaw enforces) | Optimistic versioning (`expected_version`) |
| **Bootstrap guarantee** | Hard — runtime cannot skip it | Soft (prompt) + Hard (MCP server gate) |

Pinata's approach works perfectly **within** the OpenClaw ecosystem. The design doc solves the problem for **every** runtime. The agnostic solution needs both.

---

## 2. How Pinata's Bootstrap Works

```
User deploys template on Pinata
    ↓
Pinata runs scripts.build (setup.sh)
    ↓
OpenClaw starts → force-loads workspace/BOOTSTRAP.md
    ↓
BOOTSTRAP.md runs silently: check wallet status, greet user
    ↓ passive, loaded by runtime
LLM begins with context already injected
```

**Strength**: Guaranteed — the runtime cannot skip it.
**Weakness**: OpenClaw-only. Does nothing on Claude Code, Cursor, Codex.

---

## 3. How the Design Doc's Bootstrap Works

```
Agent starts (any runtime)
    ↓
Platform adapter file is read (.claude-plugin/CLAUDE.md, .cursorrules, etc.)
    ↓ thin pointer: "call onchainos_bootstrap first"
Agent calls onchainos_bootstrap MCP tool
    ↓ active, called by agent
Rust binary reads keyring + strategy state files → assembles context
    ↓
Returns: wallet status, strategies, risk profile, environment
    ↓
LLM now has full context, proceeds with informed decisions
```

**Strength**: Works on any MCP-capable runtime. Logic lives in Rust (server-side).
**Weakness**: Relies on the adapter file prompt being obeyed. LLMs can skip prompts.

**Layer 1 hard guarantee**: MCP server gates all tools behind `bootstrapped: AtomicBool`. Any tool called before `onchainos_bootstrap` returns: *"SESSION NOT INITIALIZED. Call onchainos_bootstrap first."*

---

## 4. Gap Analysis — What Pinata Is Missing vs the Design Doc

| Feature | Pinata template (current) | Design doc requirement | Gap |
|---|---|---|---|
| Wallet status on startup | ✅ BOOTSTRAP.md checks `wallet status` | ✅ `onchainos_bootstrap` returns `WalletContext` | Different mechanism, same result |
| Strategy state context | ❌ Not present | ✅ `onchainos_bootstrap` returns active strategies | Missing entirely |
| Risk profile injection | ❌ Not present | ✅ Bootstrap returns `RiskProfile` | Missing entirely |
| Dynamic instructions | ❌ Static BOOTSTRAP.md | ✅ Bootstrap generates `instructions[]` based on state | Missing |
| State mutation safety | ❌ Agent can write files directly | ✅ `strategy_state_update` with version check | Missing |
| Drift detection | ❌ Not present | ✅ `strategy_state_reconcile` | Missing |
| Runtime-agnostic | ❌ OpenClaw only | ✅ Any MCP client | Gap |
| Bootstrap guarantee | ✅ Runtime-enforced | ✅ MCP server gate | Pinata stronger here |

---

## 5. The Agnostic Implementation — Two Complementary Layers

The solution is not "Pinata OR the design doc" — it is **both layers working together**:

```
┌─────────────────────────────────────────────────────────────┐
│  Layer A: Passive adapter files (per-runtime thin pointers)  │
│                                                              │
│  workspace/BOOTSTRAP.md  (Pinata/OpenClaw)                  │
│  → "call onchainos_bootstrap before anything else"          │
│                                                              │
│  .claude-plugin/CLAUDE.md  (Claude Code)                    │
│  .cursorrules              (Cursor)                          │
│  .codex/AGENTS.md          (Codex CLI)                      │
└──────────────────────────────┬──────────────────────────────┘
                               │ agent reads adapter, calls tool
                               ▼
┌─────────────────────────────────────────────────────────────┐
│  Layer B: Active MCP tools (runtime-agnostic, Rust)         │
│                                                              │
│  onchainos_bootstrap         → session init, full context   │
│  strategy_state_init         → create strategy + risk params│
│  strategy_state_read         → read current state           │
│  strategy_state_update       → atomic mutation + validation │
│  strategy_state_reconcile    → detect + fix on-chain drift  │
│                                                              │
│  Server-side gate: all tools blocked until bootstrap called │
└─────────────────────────────────────────────────────────────┘
```

**Layer A** is the best-effort per-runtime instruction. For Pinata/OpenClaw, it is force-loaded by the runtime — a hard guarantee. For other runtimes, it is a prompt.

**Layer B** is the hard guarantee for all runtimes. The MCP server gate means even if a runtime skips the adapter file, the first tool call will fail with a clear error message forcing the agent to bootstrap first.

---

## 6. What Needs to Be Built

### 6.1 In the Rust CLI (onchainos MCP server)

Already specified in the design doc. Key additions to `cli/src/mcp/mod.rs`:

| MCP Tool | Purpose | Status |
|---|---|---|
| `onchainos_bootstrap` | Session init: wallet + strategies + risk + environment | ❌ Not built |
| `strategy_state_init` | Create strategy JSON with budget + slots | ❌ Not built |
| `strategy_state_read` | Read strategy JSON, optional reconcile | ❌ Not built |
| `strategy_state_update` | Atomic mutation with version check | ❌ Not built |
| `strategy_state_reconcile` | Drift detection vs on-chain | ❌ Not built |

Server-side bootstrap gate (`bootstrapped: Arc<AtomicBool>`): ❌ Not built

### 6.2 In the Pinata template (`workspace/BOOTSTRAP.md`)

Current state: checks wallet status, offers login flow.

**Needs to be updated** to call `onchainos_bootstrap` MCP tool instead of (or in addition to) running `onchainos wallet status` directly:

```markdown
## Startup sequence

1. Read SOUL.md, AGENTS.md, TOOLS.md silently
2. Call MCP tool: onchainos_bootstrap
   → Returns wallet status, strategies, risk profile, environment
   → If wallet not logged in: run the login onboarding flow
   → If strategies have warnings: surface them to the user
3. Proceed with the appropriate greeting based on bootstrap result
```

This makes the Pinata template both work natively on OpenClaw AND correctly integrate with the MCP bootstrap layer when the Rust tools are implemented.

### 6.3 Platform adapter files (for non-Pinata runtimes)

| File | Runtime | Content |
|---|---|---|
| `.claude-plugin/CLAUDE.md` | Claude Code | "Call `onchainos_bootstrap` before any operation" |
| `.cursorrules` | Cursor | Same, one line |
| `.codex/AGENTS.md` | Codex CLI | Same, one line |
| `workspace/BOOTSTRAP.md` | Pinata/OpenClaw | Full onboarding flow using `onchainos_bootstrap` result |

All of these are **thin pointers** — the real logic is in the Rust MCP server.

---

## 7. Pinata-Specific Additions Needed

Once the MCP tools are built, the Pinata template needs only one change to be fully aligned:

**Update `workspace/BOOTSTRAP.md`** — replace the direct `onchainos wallet status` call with `onchainos_bootstrap` MCP tool call. The result already contains wallet status, so the login onboarding flow stays the same — it just reads from the bootstrap context instead of a direct CLI call.

The `manifest.json` and `setup.sh` stay unchanged. The skills and workflows stay unchanged.

---

## 8. Implementation Priority

| Item | Priority | Effort | Impact |
|---|---|---|---|
| `onchainos_bootstrap` MCP tool | P0 | Medium | Unblocks everything |
| Bootstrap server gate (`AtomicBool`) | P0 | Small | Hard guarantee for all runtimes |
| `strategy_state_init/read/update` | P1 | Large | Required for trading strategies |
| `strategy_state_reconcile` | P1 | Medium | Health/drift detection |
| Update `workspace/BOOTSTRAP.md` to use MCP tool | P1 | Small | Pinata alignment |
| Platform adapter files (Claude Code, Cursor, Codex) | P2 | Small | Non-Pinata runtime support |

---

## 9. Summary

**Pinata** gives us the runtime bootstrap guarantee for the OpenClaw ecosystem.
**The design doc** gives us runtime-agnostic state management and MCP-enforced correctness.

They are complementary, not competing:
- Pinata's `BOOTSTRAP.md` → calls → `onchainos_bootstrap` MCP tool
- MCP server gate → blocks → all tools until bootstrap is called
- Any runtime (Claude Code, Cursor, Codex) → reads adapter file → calls same MCP tool

The agnostic path is: **thin adapter files per runtime + thick Rust logic in the MCP server**.
