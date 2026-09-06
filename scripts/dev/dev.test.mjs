import assert from "node:assert/strict";
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
