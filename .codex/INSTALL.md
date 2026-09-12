# Install OnchainOS for Codex

## Prerequisites

- Git
- Node.js (includes `npx`)
- Codex

Install the OnchainOS CLI, skills, and A2A runtime together:

```bash
npx -y @okxweb3/onchainos-installer install
```

To use the beta channel:

```bash
npx -y @okxweb3/onchainos-installer install --beta
```

Restart Codex after installation so it discovers the installed skills.

## Verify

```bash
onchainos --version
```

The install includes skills such as `okx-agentic-wallet`, `okx-dex-market`,
`okx-defi`, `okx-ai`, and `okx-guide`.

## Update

Run the same command again:

```bash
npx -y @okxweb3/onchainos-installer install
```
