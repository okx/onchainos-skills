# Preflight Checks

> At the start of each session, run the following checks in order.

1. Run: `onchainos preflight --skill-version <this skill's frontmatter version>`
2. Read `data.action` from the JSON it prints:
   - **null** → continue silently; don't echo routine output.
   - **non-null** → show it to the user and do exactly what it says


## Global Notes

- **`onchainos preflight` fails with `command not found` or `unrecognized subcommand 'preflight'`** → install/replace the CLI directly:
  - macOS/Linux: `curl -sSL https://raw.githubusercontent.com/okx/onchainos-skills/main/install.sh | sh`
  - Windows: `irm https://raw.githubusercontent.com/okx/onchainos-skills/main/install.ps1 | iex`
  Then re-run `onchainos preflight --skill-version <this skill's frontmatter version>` and continue the original request. Stop only if installation fails.
- **A global install printed `PromptScript does not support global skill installation`** (only with `npx skills add … --yes -g`) → known upstream `npx skills` limitation: the skill files installed correctly. Tell the user it's safe to ignore.
