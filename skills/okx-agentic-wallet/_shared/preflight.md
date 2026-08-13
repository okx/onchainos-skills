# Preflight Checks

> At the start of each session, run the following checks in order.

1. Run: `onchainos preflight --skill-version <this skill's frontmatter version>`
2. Read `data.action` from the JSON it prints:
   - **null** → continue silently; don't echo routine output.
   - **non-null** → show it to the user and do exactly what it says


## Global Notes

- **`onchainos preflight` fails with `command not found` or `unrecognized subcommand 'preflight'`** → install the CLI:
  1. Determine `LATEST_TAG`: use the latest stable release, unless this skill's frontmatter `version` contains `-beta`, in which case use the newest release including pre-releases.
  2. Download the installer and checksum manifest from that exact tag:
     - macOS/Linux:
       `curl -sSL "https://raw.githubusercontent.com/okx/onchainos-skills/${LATEST_TAG}/install.sh" -o /tmp/onchainos-install.sh`
       `curl -sSL "https://github.com/okx/onchainos-skills/releases/download/${LATEST_TAG}/installer-checksums.txt" -o /tmp/installer-checksums.txt`
     - Windows:
       `Invoke-WebRequest -Uri "https://raw.githubusercontent.com/okx/onchainos-skills/${LATEST_TAG}/install.ps1" -OutFile "$env:TEMP\onchainos-install.ps1"`
       `Invoke-WebRequest -Uri "https://github.com/okx/onchainos-skills/releases/download/${LATEST_TAG}/installer-checksums.txt" -OutFile "$env:TEMP\installer-checksums.txt"`
  3. Verify the downloaded installer's SHA256 against its exact `install.sh` or `install.ps1` entry in `installer-checksums.txt`. If the entry is missing or the hash differs, **stop** and warn that the installer may have been tampered with. Never execute an unverified installer.
  4. Execute `sh /tmp/onchainos-install.sh` on macOS/Linux or `& "$env:TEMP\onchainos-install.ps1"` on Windows. Append `--beta` when this skill's frontmatter `version` contains `-beta`.
  5. Re-run `onchainos preflight --skill-version <this skill's frontmatter version>` and continue the original request. Stop only if verification or installation fails.
- **A global install printed `PromptScript does not support global skill installation`** (only with `npx skills add … --yes -g`) → known upstream `npx skills` limitation: the skill files installed correctly. Tell the user it's safe to ignore.
