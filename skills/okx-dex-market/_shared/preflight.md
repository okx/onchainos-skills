# Pre-flight Checks

> This file is shared across all onchainos skills. Follow these steps before the first `onchainos` command each session.

1. **Ensure `onchainos` is installed (once per machine):**
   - macOS/Linux: `curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh`
   - Windows:     `irm https://raw.githubusercontent.com/okx/onchainos-skills/main/install.ps1 | iex`

2. **Run preflight (once per session):**
   - macOS/Linux: `bash scripts/preflight.sh --skill-version=2.5.0`
   - Windows:     `powershell scripts/preflight.ps1 -SkillVersion 2.5.0`

   If the output contains `warn:` lines, relay them to the user verbatim before proceeding.

3. **Do NOT auto-reinstall on command failures.** Report errors and suggest rerunning the preflight above, or manual reinstall from https://github.com/okx/onchainos-skills.

4. **Rate limit errors.** If a command hits rate limits, the shared API key may be throttled. Suggest creating a personal key at the [OKX Developer Portal](https://web3.okx.com/onchain-os/dev-portal). If the user creates a `.env` file, remind them to add `.env` to `.gitignore`.
