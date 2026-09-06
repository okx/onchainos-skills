#!/usr/bin/env node

import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const repoRoot = process.argv[2];
if (!repoRoot) {
  console.error("error: repository root is required");
  process.exit(2);
}

const entries = fs.readdirSync(path.join(repoRoot, "skills"), { withFileTypes: true })
  .filter((entry) => entry.isDirectory())
  .map((entry) => path.join(repoRoot, "skills", entry.name, "SKILL.md"))
  .filter((skillMd) => fs.existsSync(skillMd))
  .map((skillMd) => ({ path: skillMd, enabled: true }));

const entryByPath = new Map(entries.map((entry) => [entry.path, entry]));
const disable = (skillMd) => entryByPath.set(skillMd, { path: skillMd, enabled: false });

const configPath = path.join(repoRoot, ".codex", "config.toml");
const config = fs.existsSync(configPath) ? fs.readFileSync(configPath, "utf8") : "";
for (const match of config.matchAll(/\[\[skills\.config\]\]\s*\npath = "([^"]+)"\s*\nenabled = false/g)) {
  disable(match[1]);
}

// A2A daemon sessions are dedicated to the V2 marketplace lifecycle. Disable
// legacy task Skills that otherwise compete for the same structured job
// envelopes before okx-ai-v2 can apply its event router.
const legacyTaskSkill = /^(?:okx-ai(?:-v2)?|okx-agent-task(?:-.+)?|okx-agent-chat|okx-task-watch)$/;
const configuredRoots = process.env.CODEX_A2A_GLOBAL_SKILL_ROOTS;
const globalRoots = configuredRoots
  ? configuredRoots.split(path.delimiter).filter(Boolean)
  : [path.join(os.homedir(), ".codex", "skills"), path.join(os.homedir(), ".agents", "skills")];
const projectSkillsRoot = fs.realpathSync(path.join(repoRoot, "skills"));

for (const root of globalRoots) {
  if (!fs.existsSync(root)) continue;
  for (const candidate of fs.readdirSync(root, { withFileTypes: true })) {
    if (!candidate.isDirectory() || !legacyTaskSkill.test(candidate.name)) continue;
    const skillMd = path.join(root, candidate.name, "SKILL.md");
    if (!fs.existsSync(skillMd)) continue;
    const realSkill = fs.realpathSync(skillMd);
    if (realSkill.startsWith(projectSkillsRoot + path.sep)) continue;
    disable(skillMd);
  }
}

const tomlString = (value) => JSON.stringify(value);
process.stdout.write(`[${[...entryByPath.values()].map((entry) => `{path=${tomlString(entry.path)},enabled=${entry.enabled}}`).join(",")}]`);
