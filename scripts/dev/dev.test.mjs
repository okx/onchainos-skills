import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

const repoRoot = path.resolve(path.dirname(new URL(import.meta.url).pathname), "../..");

test("development JSON template has one environment field", () => {
  const value = JSON.parse(fs.readFileSync(path.join(repoRoot, "config/onchainos-dev.example.json"), "utf8"));
  assert.deepEqual(Object.keys(value), ["environment"]);
  assert.equal(value.environment, "beta");
});

test("production skills keep standard CLI commands", () => {
  const skillFiles = [];
  for (const entry of fs.readdirSync(path.join(repoRoot, "skills"), { withFileTypes: true })) {
    const skill = path.join(repoRoot, "skills", entry.name, "SKILL.md");
    if (entry.isDirectory() && fs.existsSync(skill)) skillFiles.push(skill);
  }
  assert.ok(skillFiles.length > 0);
  for (const file of skillFiles) {
    const content = fs.readFileSync(file, "utf8");
    assert.equal(content.includes("npm run dev:"), false, file);
  }
});

test("generated development paths remain repository-local", () => {
  const expected = [".codex/bin", ".codex/build", ".codex/runtime", ".agents/skills"];
  for (const relative of expected) assert.ok(path.resolve(repoRoot, relative).startsWith(`${repoRoot}${path.sep}`));
  assert.notEqual(repoRoot, os.homedir());
});

test("A2A uses the prebuilt checkout-local OnchainOS binary", () => {
  const wrapper = fs.readFileSync(path.join(repoRoot, "scripts/dev/okx-a2a.sh"), "utf8");
  assert.match(wrapper, /local_onchainos="\$CARGO_TARGET_DIR\/debug\/onchainos"/);
  assert.match(wrapper, /export ONCHAINOS_BIN="\$local_onchainos"/);
  assert.match(wrapper, /run npm run dev:cli/);
  assert.match(wrapper, /if \[\[ -x "\$local_codex" \]\]; then/);
  assert.match(wrapper, /OKX_A2A_AI_CODEX_COMMAND="\$local_codex"/);
});

test("same-source skills still disable stale global duplicates", () => {
  const source = fs.readFileSync(path.join(repoRoot, "scripts/dev/dev.mjs"), "utf8");
  const conflictScan = source.indexOf("if (!sameFile(candidate, skill.skillMd)) conflicts.push");
  const sameSourceReuse = source.indexOf("if (sameSource) {", conflictScan);
  assert.ok(conflictScan >= 0, "stale global matches must be classified as conflicts");
  assert.ok(sameSourceReuse > conflictScan, "conflicts must be collected before same-source reuse continues");
});

test("daemon Codex adapter pins project skills and stale-global disables", () => {
  const adapter = fs.readFileSync(path.join(repoRoot, "scripts/dev/codex-a2a.sh"), "utf8");
  const generator = fs.readFileSync(path.join(repoRoot, "scripts/dev/codex-skill-config.mjs"), "utf8");
  assert.match(adapter, /skills\.config=\$skills_config/);
  assert.match(adapter, /codex\.real/);
  assert.match(generator, /enabled: true/);
  assert.match(generator, /enabled = false/);
  assert.match(generator, /legacyTaskSkill/);
});

test("Codex integration remains optional for non-Codex developers", () => {
  const source = fs.readFileSync(path.join(repoRoot, "scripts/dev/dev.mjs"), "utf8");
  assert.doesNotMatch(source, /if \(!codex\) fail\("global Codex CLI/);
  assert.match(source, /if \(codex\) \{/);
  assert.match(source, /Codex-specific A2A session pinning skipped/);
});

test("daemon Codex adapter disables legacy task skills that compete for envelopes", () => {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "codex-a2a-skills-"));
  const skillRoot = path.join(tempRoot, "skills");
  const legacy = path.join(skillRoot, "okx-agent-task-confirmation");
  const unrelated = path.join(skillRoot, "unrelated-skill");
  fs.mkdirSync(legacy, { recursive: true });
  fs.mkdirSync(unrelated, { recursive: true });
  fs.writeFileSync(path.join(legacy, "SKILL.md"), "---\nname: legacy\n---\n");
  fs.writeFileSync(path.join(unrelated, "SKILL.md"), "---\nname: unrelated\n---\n");

  try {
    const output = execFileSync(
      process.execPath,
      [path.join(repoRoot, "scripts/dev/codex-skill-config.mjs"), repoRoot],
      {
        encoding: "utf8",
        env: { ...process.env, CODEX_A2A_GLOBAL_SKILL_ROOTS: skillRoot },
      },
    );
    assert.ok(output.includes("{path=" + JSON.stringify(path.join(legacy, "SKILL.md")) + ",enabled=false}"));
    assert.doesNotMatch(output, /unrelated-skill/);
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
});
