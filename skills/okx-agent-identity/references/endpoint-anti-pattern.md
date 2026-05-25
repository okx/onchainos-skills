# Endpoint Anti-Pattern (P0)

Surfaces from Endpoint Inquiry trigger in skill description AND from in-flow Q5 in `references/role-provider.md`.

## Absolute requirements

A2MCP `endpoint` MUST be:
1. `https://` scheme (not `http://`).
2. **公网可达** — publicly reachable from the open internet by the buyer's agent.
3. A real deployed service — not a placeholder, Mock URL, or doc-only example.

The CLI does NOT validate (2) or (3). Bad endpoints will be accepted and minted permanently on-chain. The skill must catch these at the suggestion / Q&A layer.

## Forbidden patterns

| Pattern | Why forbidden |
|---|---|
| `http://...` (no `s`) | Insecure; many buyer agents will refuse non-TLS endpoints |
| `http://localhost` / `https://localhost` | `localhost` = buyer's own machine; buyer gets connection-refused |
| `http://127.0.0.1` / `https://127.0.0.1` | Same reason as `localhost` |
| `http://192.168.x.x` / `192.168.*` | Private RFC-1918 IP, only reachable inside provider's LAN |
| `http://10.0.x.x` / `10.*` | Private RFC-1918 IP |
| `http://172.16.x.x` ~ `172.31.x.x` | Private RFC-1918 IP |
| `*.local` / `*.internal` | mDNS / corporate-internal hostnames, no public DNS |
| `https://internal-api.<company>.com` | Corporate-internal domain, no public DNS |
| Mock service URLs (Swagger UI / Postman Mock / mockable.io) | Time-limited; will expire into a dead endpoint |
| Placeholder strings (`https://TODO.example.com` / "暂时填这个") | Each change requires another on-chain `agent update` write |

## "No endpoint yet" response

User: "我没有 https 接口" / "我还没部署服务" / "I don't have a deployed API yet".

> 中文: 「接口地址必须是公网可达的 `https://` URL — 你的服务上链后，其他 agent 会**从公网调用**这个地址。如果你还没部署，可以等部署好了再创建 — 上链一次后再改接口地址需要重走一次更新流程。用任何能提供公网 https URL 的 PaaS 部署你的 MCP server，拿到正式 URL 再回来创建。」
>
> English: "The endpoint must be a publicly reachable `https://` URL — other agents will call it from the open internet after your service is on-chain. Deploy first, create afterwards (changing the endpoint later requires another on-chain `agent update`). Deploy your MCP server to any PaaS that gives you a public https URL, then come back to create the agent."

⛔ Never suggest:
- `localhost` / 127.0.0.1 / private IP "while testing"
- `http://` without TLS "for now"
- Mock services / Postman Mock / Swagger UI demos
- Placeholder strings ("先写 `https://TODO.com`，回头改")
- "Maybe try a self-signed cert" (other agents will reject)

The cost of one extra round-trip is far below the cost of a permanent dead on-chain service NFT.
