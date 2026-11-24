# Install OnchainOS for OpenCode

## Prerequisites

- Git
- Node.js (includes `npx`)
- OpenCode

Install the OnchainOS CLI, skills, and A2A runtime together:

```bash
npx -y @okxweb3/onchainos-installer install
```

To use the beta channel:

```bash
npx -y @okxweb3/onchainos-installer install --beta
```

Restart OpenCode after installation so it discovers the installed skills.

## Verify

```bash
onchainos --version
```

## Update

Run the same command again:

```bash
npx -y @okxweb3/onchainos-installer install
```
