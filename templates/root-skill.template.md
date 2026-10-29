<!-- TEMPLATE — Root Skill (skills/<skill>/SKILL.md). Replace every {placeholder}; use only the ## headings below. The four sentences under ## CLI Reference Access are fixed — copy them verbatim. -->
---
name: {skill-name}
description: "{concise, idiomatic trigger description}"
---

# {Skill name}

## Module Description
{Scope, entry boundary, and explicit route-out boundary.}

## Preflight
<!-- Optional: only checks required before every route. -->

## Intent Routing
| User intent | Reference |
| --- | --- |
| ... | [module](references/module.md) |

## State Routing
<!-- Optional: trusted machine input only. -->
| Input or state | Reference |
| --- | --- |
| ... | [router](references/domain/router.md) |

## Action Routing
<!-- Optional: latest trusted CLI action only. -->
| Action ID | Reference |
| --- | --- |
| ... | [module-reference](references/module-reference.md) |

## CLI Reference Access
1. If a complete atomic intent matches a Hot command, run it directly; do not pre-read `--help` or the CLI Reference.
2. Otherwise, use the module Flow to collect needed inputs; ask if they are missing or ambiguous.
3. Read the module's CLI Reference to choose the exact command. It is authoritative for syntax, defaults, and validation.
4. Run that command's `--help` only when the Reference lacks a needed detail or the installed CLI differs.

## CLI Result Contract
1. Treat the latest CLI result as authoritative. Use its declared status, action, message, data, and next-step fields; do not infer success from a transport or HTTP status alone.
2. When the result carries a continuation action, use only the latest trusted action ID to select the next Reference.
3. Preserve module-specific funding, risk, and recovery handling in that module's Flow.

## Security
### Confirmation
### Trust Boundary

## Output Rules
### Render Rules
<!-- Optional. -->
#### Amounts and Prices
#### Identifiers
#### Lists and Status

## Chain Name Support
<!-- Optional. -->
