# Skill Templates

Reusable page templates for authoring and refactoring Skills. Every Skill page is
one page type with a fixed structure and a fixed set of allowed `##` headings.
Start from the matching template instead of inventing structure.

## Page types

| Page type | Where it lives | Template |
| --- | --- | --- |
| Root Skill | `skills/<skill>/SKILL.md` | [`root-skill.template.md`](root-skill.template.md) |
| Router Reference | `skills/<skill>/references/**/router.md` | [`router-reference.template.md`](router-reference.template.md) |
| Business / Policy Reference | `skills/<skill>/references/<module>.md` | [`business-policy-reference.template.md`](business-policy-reference.template.md) |
| CLI Reference | `skills/<skill>/references/<module>-cli-reference.md` | [`cli-reference.template.md`](cli-reference.template.md) |
| Troubleshooting Reference | `skills/<skill>/references/<module>-troubleshooting.md` | [`troubleshooting-reference.template.md`](troubleshooting-reference.template.md) |
| Shared Reference | `skills/<skill>/_shared/<topic>.md` | [`shared-reference.template.md`](shared-reference.template.md) |
| WebSocket Protocol Reference | `skills/<skill>/references/<domain>-ws-protocol.md` | [`websocket-protocol-reference.template.md`](websocket-protocol-reference.template.md) |

## How to use

- Pick the page type by the file name (what the file *is*), not the business domain.
- Copy the template; replace every `{placeholder}` and guidance comment.
- Keep the template's heading structure. Map an equivalent `##` heading onto an
  allowed heading (e.g. `Execution Flow → Flow`); follow each template's notes
  for optional `###` and `####` sections.
- No free body text directly after `# Title`.

## Route tables

Only these four headers are valid:

| Route type | Header | Where |
| --- | --- | --- |
| Natural language | `User intent \| Reference` | any |
| Trusted state | `Input or state \| Reference` | any |
| CLI continuation | `Action ID \| Reference` | any |
| Atomic hot command | `User intent \| Reference \| Hot command` | Business / Policy only |

A Hot command is listed only for a single, input-complete, unambiguous atomic
intent that does not bypass confirmation/risk/freshness/result rules. Broad or
incomplete intents go through the Flow.

## Responsibilities

| Page | Owns | Keep out |
| --- | --- | --- |
| Root Skill | scope, routing, global CLI access + result contract, cross-path security, render rules | module-specific funding/recovery logic |
| Router Reference | choosing the next Reference | business execution, funding confirmation, full recovery |
| Business / Policy Reference | Flow, parameters, execution, result handling, module security, output rules, troubleshooting, CLI Reference links | the command contract itself |
| CLI Reference | syntax, parameters, return fields, examples, command-level errors, command constraints | intent routing, business confirmation, recovery, login, credentials, preflight |
| Troubleshooting Reference | error codes, diagnostics, recovery, FAQ | the normal flow |
| Shared Reference | a shared capability list or one shared Flow | module-specific business/CLI contracts |

## CLI Reference Access

The Root Skill template's `## CLI Reference Access` section is fixed — keep its
four sentences verbatim.

## Validation

Review each page against its template: its allowed heading structure; the route
table contract; no free body text after `# Title`; fixed CLI Reference Access
copy; and required CLI Reference and Troubleshooting links. Judge semantic
ownership and optional protocol sections from the page's actual purpose.
