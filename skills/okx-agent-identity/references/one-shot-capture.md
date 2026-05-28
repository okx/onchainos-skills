# One-Shot Capture

Silent support for users who dump everything at once (e.g. "注册一个 provider 叫 Alice，描述是做 DeFi 研究，用默认头像").

## Rules

1. **Silent, not advertised.** Never say "你也可以一次性输入". One-shot is a fast path users discover naturally; the step-by-step Q&A remains the default surface.
2. **Capture only unambiguous values.** If the split is ambiguous ("Alice 做 DeFi 分析" — is the name `Alice` or `Alice 做 DeFi 分析`?), capture only the clearly-unambiguous part; leave the ambiguous field for the normal Q.
3. **Skip answered Q's silently.** If Q_k's field is already captured, skip Q_k without echoing "name is already Alice". The confirmation card shows everything at the end.
4. **Phase boundary is strict.** Identity-phase capture does NOT reach into service-phase fields. "provider 叫 Alice，收 10 USDT" → capture `name=Alice`, discard `fee=10`. When Phase 2 starts, MAY quote the earlier mention as a suggested default (not auto-fill): `这个服务叫什么名字？（你刚提到「天气查北京」，确认就是它吗？或想改？）`. ⛔ No `Q1：` prefix (Red line 3).
5. **All fields captured → still render confirmation card.** Even if every required field was covered in one shot, the confirmation card is mandatory (see `§⛔ MANDATORY confirmation gate`). Wait for explicit `执行` / `execute` / `yes` before calling the CLI.
6. **Confirmation-step ambiguity.** If any captured value was edge-case (whitespace, punctuation), show it verbatim and let the user reject during confirmation. Do not "clean up" silently.
7. **One-shot + numbered choice combo.** If the one-shot utterance includes a choice field (e.g. "Type: A2MCP"), accept it. If user used the label ("A2A 类型"), also accept. When asking a choice Q the user hasn't answered yet, still use the numbered-options pattern (`references/choice-prompts.md`).

## Worked examples

- **A — partial, requester:** User: "注册一个买家叫 Alice". Captures `role=requester`, `name=Alice`. Preview → skip Q1 → Q2 (description) → Q3 (picture) → confirmation.
- **B — full, requester:** User: "注册一个买家，名字 Alice，描述做 DeFi 研究，不要头像". All Q's skipped → confirmation card directly.
- **C — ambiguous split:** User: "provider 叫 Alice 做 DeFi 分析师". Name could be `Alice` or `Alice 做 DeFi 分析师`. Captures `role=provider` only; name + description left for normal Q&A.
- **D — cross-phase leakage (strict rejection):** User: "provider 叫 Alice，做 DeFi 分析，收 10 USDT". Phase-1 capture: `name=Alice`, `description=做 DeFi 分析`. **Fee=10 is discarded.** Phase 2 starts fresh with its own Q1.
