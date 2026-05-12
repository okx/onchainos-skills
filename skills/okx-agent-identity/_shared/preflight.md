# Shared Pre-flight Checks

> This file is shared across all onchainos skills. Follow these steps before the first `onchainos` command each session.

Every time before running any `onchainos` command, always follow these steps in order. Do not echo routine command output to the user; only provide a brief status update when installing, updating, or handling a failure.

1. **Resolve latest stable version**: Fetch the latest stable release tag from the GitHub API:
   ```
   curl -sSL "https://api.github.com/repos/okx/onchainos-skills/releases/latest"
   ```
   Extract the `tag_name` field (e.g., `v1.0.5`) into `LATEST_TAG`.
   If the API call fails and `onchainos` is already installed locally, skip steps 2-3
   and proceed to run the command (the user may be offline or rate-limited; a stale
   binary is better than blocking). If `onchainos` is **not** installed, **stop** and
   tell the user to check their network connection or install manually from
   https://github.com/okx/onchainos-skills.

2. **Install or update**: If `onchainos` is not found, or if the cache at `~/.onchainos/last_check` (`$env:USERPROFILE\.onchainos\last_check` on Windows) is older than 12 hours:
   - Download the installer and its checksum file from the latest release tag:
     - **macOS/Linux**:
       `curl -sSL "https://raw.githubusercontent.com/okx/onchainos-skills/${LATEST_TAG}/install.sh" -o /tmp/onchainos-install.sh`
       `curl -sSL "https://github.com/okx/onchainos-skills/releases/download/${LATEST_TAG}/installer-checksums.txt" -o /tmp/installer-checksums.txt`
     - **Windows**:
       `Invoke-WebRequest -Uri "https://raw.githubusercontent.com/okx/onchainos-skills/${LATEST_TAG}/install.ps1" -OutFile "$env:TEMP\onchainos-install.ps1"`
       `Invoke-WebRequest -Uri "https://github.com/okx/onchainos-skills/releases/download/${LATEST_TAG}/installer-checksums.txt" -OutFile "$env:TEMP\installer-checksums.txt"`
   - Verify the installer's SHA256 against `installer-checksums.txt`. On mismatch, **stop** and warn — the installer may have been tampered with.
   - Execute: `sh /tmp/onchainos-install.sh` (or `& "$env:TEMP\onchainos-install.ps1"` on Windows).
     The installer handles version comparison internally and only downloads the binary if needed.
   - On other failures, point to https://github.com/okx/onchainos-skills.

3. **Verify binary integrity** (once per session): Run `onchainos --version` to get the installed
   version (e.g., `1.0.5` or `2.0.0-beta.0`). Construct the installed tag as `v<version>`.
   Download `checksums.txt` for the **installed version's tag** (not necessarily LATEST_TAG):
   `curl -sSL "https://github.com/okx/onchainos-skills/releases/download/v<version>/checksums.txt" -o /tmp/onchainos-checksums.txt`
   Look up the platform target and compare the installed binary's SHA256 against the checksum.
   On mismatch, reinstall (step 2) and re-verify. If still mismatched, **stop** and warn.
   - Platform targets — macOS: `arm64`->`aarch64-apple-darwin`, `x86_64`->`x86_64-apple-darwin`; Linux: `x86_64`->`x86_64-unknown-linux-gnu`, `aarch64`->`aarch64-unknown-linux-gnu`, `i686`->`i686-unknown-linux-gnu`, `armv7l`->`armv7-unknown-linux-gnueabihf`; Windows: `AMD64`->`x86_64-pc-windows-msvc`, `x86`->`i686-pc-windows-msvc`, `ARM64`->`aarch64-pc-windows-msvc`
   - Hash command — macOS/Linux: `shasum -a 256 ~/.local/bin/onchainos`; Windows: `(Get-FileHash "$env:USERPROFILE\.local\bin\onchainos.exe" -Algorithm SHA256).Hash.ToLower()`

4. **Check for skill version drift** (once per session): If `onchainos --version` is newer
   than the `version` field under `metadata:` in the active skill's YAML frontmatter (e.g., `version: "1.0.0"` between the `---` markers at the top of SKILL.md), display a one-time notice that the skill may be
   outdated and suggest the user re-install skills via their platform's method. Do not block.
5. **Do NOT auto-reinstall on command failures.** Report errors and suggest
   `onchainos --version` or manual reinstall from https://github.com/okx/onchainos-skills.
6. **Rate limit errors.** If a command hits rate limits, the shared API key may
   be throttled. Suggest creating a personal key at the
   [OKX Developer Portal](https://web3.okx.com/onchain-os/dev-portal). If the
   user creates a `.env` file, remind them to add `.env` to `.gitignore`.

## Identity-specific preconditions

Before any `onchainos agent …` command from this skill:

1. **Wallet login** — run `onchainos wallet status` (non-interactive). If not logged in, stop this skill and guide the user to `okx-agentic-wallet` for login. Do NOT attempt to log in from here.
2. **XLayer address present + captured** — the current wallet must have an XLayer (chainIndex `196`) address. If missing, redirect to `okx-agentic-wallet` (`wallet add` / `wallet switch`). If present, **capture the exact XLayer address value from this `wallet status` call into session state** — downstream rules use it as `<currently selected XLayer wallet address>` to:
   - filter `agent get`'s **double-layer envelope** (only the `list[*]` wrapper whose `ownerAddress == <captured address>` is counted for K=1/K≥2 / uniqueness pre-check — see `references/role-playbook.md §Pre-check`);
   - locate the wrapper for post-create `agentList` envelope diff when recovering a freshly-minted `agentId` (see `references/role-{requester,provider,evaluator}.md §Post-success` source 2 + `references/cli-reference.md §1` "Finding the newly-minted `agentId`");
   - resolve `--creator-id` candidates in `feedback-guide.md §Step 2` ladder 2.

   The address is stable for the rest of the session **unless** the user explicitly switches wallets via `okx-agentic-wallet` (`wallet switch` / `wallet add` followed by select). If you suspect a mid-session switch happened (user mentions a different account, `wallet status` output looks different), **re-run `wallet status` and refresh the captured address** before applying any of the rules above. Stale capture → wrong-wallet filtering → either over-counts K (treats other wallets' agents as the current wallet's) or fails to find the newly-minted agent in the diff.
3. **Chain is fixed** — every command in this skill operates on XLayer. Never present the user with a chain-selection prompt. Never mention other chains for identity operations.
4. **Scope reminder** — this skill only handles ERC-8004 identity (register / update / activate / deactivate / search / feedback / services). Redirect to the correct skill if the user asks for:
   - Task lifecycle (publish / accept / deliver / dispute) → `okx-agent-task`
   - Wallet login, balance, transfer, message signing → `okx-agentic-wallet`
   - OKB staking (onboarding / top-up / unstake / claim / query) → follow `/skills/okx-agent-task/references/evaluator-staking.md`
