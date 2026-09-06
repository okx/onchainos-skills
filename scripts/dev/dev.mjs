#!/usr/bin/env node

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "../..");
const codexDir = path.join(repoRoot, ".codex");
const binDir = path.join(codexDir, "bin");
const projectSkillsDir = path.join(repoRoot, ".agents", "skills");
const devConfig = path.join(codexDir, "dev.json");
const codexConfig = path.join(codexDir, "config.toml");
const sourceSkillsDir = path.join(repoRoot, "skills");
const generatedSkillPrefix = "# onchainos-dev generated skill conflict";

function fail(message, code = 1) {
  console.error(`error: ${message}`);
  process.exit(code);
}

function ensureDir(dir, mode = 0o700) {
  fs.mkdirSync(dir, { recursive: true, mode });
  try { fs.chmodSync(dir, mode); } catch {}
}

function chmodPrivate(file) {
  try { fs.chmodSync(file, 0o600); } catch {}
}

function readEnvironment() {
  let value;
  try { value = JSON.parse(fs.readFileSync(devConfig, "utf8")); }
  catch (error) { fail(`invalid ${devConfig}: ${error.message}`); }
  const keys = Object.keys(value);
  if (keys.length !== 1 || keys[0] !== "environment" || typeof value.environment !== "string" || !value.environment.trim()) {
    fail(`${devConfig} must contain exactly one non-empty string field: environment`);
  }
  const selected = value.environment.trim();
  if (selected === "beta") return { selected, label: "beta", baseUrl: "https://beta.okex.org" };
  if (selected === "production") return { selected, label: "production", baseUrl: "https://web3.okx.com" };
  if (/^https?:\/\//.test(selected)) return { selected, label: "custom", baseUrl: selected };
  fail(`unsupported environment '${selected}' (use beta, production, or an http(s) URL)`, 2);
}

function parseSkillName(skillMd) {
  const content = fs.readFileSync(skillMd, "utf8");
  const match = content.match(/^---\s*\n[\s\S]*?^name:\s*["']?([^\n"']+)["']?\s*$[\s\S]*?^---\s*$/m);
  return match?.[1]?.trim() || path.basename(path.dirname(skillMd));
}

function sourceSkills() {
  return fs.readdirSync(sourceSkillsDir, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => path.join(sourceSkillsDir, entry.name, "SKILL.md"))
    .filter((skillMd) => fs.existsSync(skillMd))
    .map((skillMd) => ({ name: parseSkillName(skillMd), dir: path.dirname(skillMd), skillMd }));
}

function walkSkillFiles(root, maxDepth = 3) {
  const result = [];
  function walk(current, depth) {
    if (depth > maxDepth || !fs.existsSync(current)) return;
    let entries;
    try { entries = fs.readdirSync(current, { withFileTypes: true }); } catch { return; }
    for (const entry of entries) {
      const target = path.join(current, entry.name);
      if (entry.name === "SKILL.md" && entry.isFile()) result.push(target);
      else if (entry.isDirectory() || entry.isSymbolicLink()) walk(target, depth + 1);
    }
  }
  walk(root, 0);
  return result;
}

function globalSkillsByName() {
  const roots = [path.join(os.homedir(), ".agents", "skills"), path.join(os.homedir(), ".codex", "skills")];
  const map = new Map();
  for (const root of roots) {
    for (const skillMd of walkSkillFiles(root)) {
      let name;
      try { name = parseSkillName(skillMd); } catch { continue; }
      const items = map.get(name) || [];
      items.push(skillMd);
      map.set(name, items);
    }
  }
  return map;
}

function sameFile(a, b) {
  try { return fs.realpathSync(a) === fs.realpathSync(b); } catch { return false; }
}

function refreshSkills() {
  ensureDir(projectSkillsDir);
  const globals = globalSkillsByName();
  const desired = new Map();
  const conflicts = [];
  const reused = [];

  for (const skill of sourceSkills()) {
    const globalMatches = globals.get(skill.name) || [];
    const sameSource = globalMatches.find((candidate) => sameFile(candidate, skill.skillMd));
    for (const candidate of globalMatches) {
      if (!sameFile(candidate, skill.skillMd)) conflicts.push({ name: skill.name, path: candidate });
    }
    if (sameSource) {
      reused.push({ name: skill.name, path: sameSource });
      continue;
    }
    desired.set(skill.name, skill.dir);
  }

  for (const entry of fs.readdirSync(projectSkillsDir, { withFileTypes: true })) {
    const target = path.join(projectSkillsDir, entry.name);
    if (!entry.isSymbolicLink()) continue;
    const marker = fs.readlinkSync(target);
    const absolute = path.resolve(path.dirname(target), marker);
    if (absolute.startsWith(`${sourceSkillsDir}${path.sep}`) && !desired.has(entry.name)) fs.unlinkSync(target);
  }

  for (const [name, source] of desired) {
    const target = path.join(projectSkillsDir, name);
    if (fs.existsSync(target) || fs.lstatSync(target, { throwIfNoEntry: false })) {
      if (fs.lstatSync(target).isSymbolicLink() && sameFile(target, source)) continue;
      fail(`refusing to replace existing project skill entry: ${target}`);
    }
    fs.symlinkSync(source, target, "dir");
  }

  updateConflictConfig(conflicts);
  return { linked: [...desired.keys()], reused, conflicts };
}

function stripGeneratedConflictBlocks(text) {
  const lines = text.split("\n");
  const output = [];
  for (let i = 0; i < lines.length; i += 1) {
    if (lines[i] !== generatedSkillPrefix) { output.push(lines[i]); continue; }
    i += 3;
  }
  return output.join("\n").replace(/\n{3,}/g, "\n\n");
}

function updateConflictConfig(conflicts) {
  let text = fs.existsSync(codexConfig) ? fs.readFileSync(codexConfig, "utf8") : "";
  text = stripGeneratedConflictBlocks(text).trimEnd();
  for (const conflict of conflicts) {
    const escaped = conflict.path.replaceAll("\\", "\\\\").replaceAll('"', '\\"');
    text += `\n\n${generatedSkillPrefix}\n[[skills.config]]\npath = "${escaped}"\nenabled = false`;
  }
  fs.writeFileSync(codexConfig, `${text.trimStart()}\n`, { mode: 0o600 });
  chmodPrivate(codexConfig);
}

function findExecutableOutsideProject(name) {
  const existingReal = path.join(binDir, `${name}.real`);
  if (fs.existsSync(existingReal)) {
    try {
      const resolved = fs.realpathSync(existingReal);
      if (fs.statSync(resolved).mode & 0o111) return resolved;
    } catch {}
  }
  for (const dir of (process.env.PATH || "").split(path.delimiter)) {
    if (!dir || path.resolve(dir) === path.resolve(binDir)) continue;
    const candidate = path.join(dir, name);
    try { if (fs.statSync(candidate).mode & 0o111) return fs.realpathSync(candidate); } catch {}
  }
  return null;
}

function linkFile(source, target) {
  if (fs.existsSync(target) || fs.lstatSync(target, { throwIfNoEntry: false })) fs.unlinkSync(target);
  fs.symlinkSync(source, target);
}

function dedupePath(entries) {
  return [...new Set(entries.filter(Boolean).map((entry) => path.resolve(entry)))];
}

function updateCodexPath() {
  let text = fs.existsSync(codexConfig) ? fs.readFileSync(codexConfig, "utf8") : "";
  const currentPath = dedupePath((process.env.PATH || "").split(path.delimiter)
    .filter((entry) => path.resolve(entry) !== path.join(repoRoot, "cli", "target", "debug"))
    .filter((entry) => path.resolve(entry) !== binDir));
  const configuredPath = [binDir, ...currentPath].join(path.delimiter).replaceAll("\\", "\\\\").replaceAll('"', '\\"');

  if (!/^\[shell_environment_policy\]$/m.test(text)) {
    text = `${text.trimEnd()}\n\n[shell_environment_policy]\ninherit = "all"\n`;
  } else if (/^inherit\s*=/m.test(section(text, "shell_environment_policy"))) {
    text = replaceInSection(text, "shell_environment_policy", /^inherit\s*=.*$/m, 'inherit = "all"');
  } else {
    text = insertInSection(text, "shell_environment_policy", 'inherit = "all"');
  }

  if (!/^\[shell_environment_policy\.set\]$/m.test(text)) {
    text = `${text.trimEnd()}\n\n[shell_environment_policy.set]\nPATH = "${configuredPath}"\n`;
  } else if (/^PATH\s*=/m.test(section(text, "shell_environment_policy.set"))) {
    text = replaceInSection(text, "shell_environment_policy.set", /^PATH\s*=.*$/m, `PATH = "${configuredPath}"`);
  } else {
    text = insertInSection(text, "shell_environment_policy.set", `PATH = "${configuredPath}"`);
  }
  fs.writeFileSync(codexConfig, `${text.trim()}\n`, { mode: 0o600 });
  chmodPrivate(codexConfig);
}

function section(text, name) {
  const match = text.match(new RegExp(`^\\[${name.replaceAll(".", "\\.")}\\]\\n([\\s\\S]*?)(?=^\\[|$)`, "m"));
  return match?.[1] || "";
}

function replaceInSection(text, name, pattern, replacement) {
  const body = section(text, name);
  return text.replace(body, body.replace(pattern, replacement));
}

function insertInSection(text, name, line) {
  const body = section(text, name);
  return text.replace(body, `${body.trimEnd()}\n${line}\n`);
}

function init() {
  ensureDir(codexDir);
  ensureDir(binDir);
  for (const dir of ["build/cargo-home", "build/cargo-target", "runtime/onchainos", "runtime/a2a", "runtime/a2a-spool", "runtime/tmp"]) ensureDir(path.join(codexDir, dir));
  if (!fs.existsSync(devConfig)) fs.copyFileSync(path.join(repoRoot, "config", "onchainos-dev.example.json"), devConfig);
  chmodPrivate(devConfig);
  readEnvironment();

  const a2a = findExecutableOutsideProject("okx-a2a");
  if (!a2a) fail("global okx-a2a is required but was not found on PATH");
  const codex = findExecutableOutsideProject("codex");
  linkFile(path.join(scriptDir, "onchainos.sh"), path.join(binDir, "onchainos"));
  linkFile(path.join(scriptDir, "okx-a2a.sh"), path.join(binDir, "okx-a2a"));
  linkFile(a2a, path.join(binDir, "okx-a2a.real"));
  if (codex) {
    linkFile(path.join(scriptDir, "codex-a2a.sh"), path.join(binDir, "codex-a2a"));
    linkFile(codex, path.join(binDir, "codex.real"));
  }
  updateCodexPath();
  const skills = refreshSkills();

  console.log("Initialized project-local OnchainOS development.");
  console.log(`  Config: ${devConfig}`);
  console.log(`  CLI:    ${path.join(binDir, "onchainos")}`);
  console.log(`  A2A:    ${path.join(binDir, "okx-a2a")} -> ${a2a}`);
  console.log(codex
    ? `  Codex:  ${path.join(binDir, "codex-a2a")} -> ${codex}`
    : "  Codex:  not installed; Codex-specific A2A session pinning skipped");
  console.log(`  Skills: linked=${skills.linked.length} reused-global-same-source=${skills.reused.length} conflicts-disabled=${skills.conflicts.length}`);
  console.log("Reload Codex and start a new task before validating Skill routing.");
}

function envCommand(value) {
  if (value) {
    const selected = value.trim();
    if (!(selected === "beta" || selected === "production" || /^https?:\/\//.test(selected))) fail("environment must be beta, production, or an http(s) URL", 2);
    ensureDir(codexDir);
    fs.writeFileSync(devConfig, `${JSON.stringify({ environment: selected }, null, 2)}\n`, { mode: 0o600 });
    chmodPrivate(devConfig);
  }
  const env = readEnvironment();
  console.log(`Environment: ${env.label}`);
  console.log(`Base URL:    ${env.baseUrl}`);
}

function doctor() {
  const errors = [];
  const env = readEnvironment();
  const expected = {
    onchainos: path.join(binDir, "onchainos"),
    "okx-a2a": path.join(binDir, "okx-a2a"),
  };
  for (const [name, target] of Object.entries(expected)) {
    try { if (!(fs.statSync(target).mode & 0o111)) errors.push(`${name} wrapper is not executable`); }
    catch { errors.push(`${name} wrapper is missing`); }
  }
  try { if (!(fs.statSync(path.join(binDir, "okx-a2a.real")).mode & 0o111)) errors.push("okx-a2a.real is not executable"); }
  catch { errors.push("okx-a2a.real is missing or broken"); }
  const codexAdapter = path.join(binDir, "codex-a2a");
  const codexReal = path.join(binDir, "codex.real");
  if (fs.existsSync(codexAdapter) || fs.existsSync(codexReal)) {
    try { if (!(fs.statSync(codexAdapter).mode & 0o111)) errors.push("codex-a2a wrapper is not executable"); }
    catch { errors.push("codex-a2a wrapper is missing or broken"); }
    try { if (!(fs.statSync(codexReal).mode & 0o111)) errors.push("codex.real is not executable"); }
    catch { errors.push("codex.real is missing or broken"); }
  }
  const configText = fs.existsSync(codexConfig) ? fs.readFileSync(codexConfig, "utf8") : "";
  if (!configText.includes(binDir)) errors.push(".codex/config.toml does not put .codex/bin on PATH");

  const skills = sourceSkills();
  const globals = globalSkillsByName();
  for (const skill of skills) {
    const sameGlobal = (globals.get(skill.name) || []).some((candidate) => sameFile(candidate, skill.skillMd));
    const local = path.join(projectSkillsDir, skill.name);
    if (!sameGlobal && !sameFile(local, skill.dir)) errors.push(`skill '${skill.name}' has no active project or same-source global link`);
  }

  console.log(`Environment: ${env.label}`);
  console.log(`Base URL:    ${env.baseUrl}`);
  console.log(`CLI state:   ${path.join(codexDir, "runtime", "onchainos")}`);
  console.log(`A2A state:   ${path.join(codexDir, "runtime", "a2a")}`);
  if (errors.length) {
    for (const error of errors) console.error(`FAIL: ${error}`);
    process.exit(1);
  }
  console.log("OK: project-local development structure is valid.");
  console.log("Note: reload Codex/new-task routing must be verified separately after Skill changes.");
}

function runA2a(args, quiet = false) {
  const wrapper = path.join(binDir, "okx-a2a");
  if (!fs.existsSync(wrapper)) { if (!quiet) fail("A2A wrapper is missing; run npm run dev:init"); return; }
  return spawnSync(wrapper, args, { cwd: repoRoot, stdio: quiet ? "ignore" : "inherit" });
}

function stop() {
  const result = runA2a(["daemon", "stop"], true);
  if (result && result.status !== 0) console.log("A2A daemon was not running or could not be stopped cleanly.");
  else console.log("Stopped project-local A2A daemon.");
}

function removeOwnedProjectSkills() {
  if (!fs.existsSync(projectSkillsDir)) return;
  for (const entry of fs.readdirSync(projectSkillsDir, { withFileTypes: true })) {
    const target = path.join(projectSkillsDir, entry.name);
    if (!entry.isSymbolicLink()) continue;
    const absolute = path.resolve(path.dirname(target), fs.readlinkSync(target));
    if (absolute.startsWith(`${sourceSkillsDir}${path.sep}`)) fs.unlinkSync(target);
  }
}

function clean(all, removeConfig) {
  stop();
  removeOwnedProjectSkills();
  for (const relative of ["bin", "build", "runtime/a2a-spool", "runtime/tmp"]) fs.rmSync(path.join(codexDir, relative), { recursive: true, force: true });
  if (all) {
    fs.rmSync(path.join(codexDir, "runtime", "onchainos"), { recursive: true, force: true });
    fs.rmSync(path.join(codexDir, "runtime", "a2a"), { recursive: true, force: true });
  }
  if (removeConfig) {
    fs.rmSync(devConfig, { force: true });
    fs.rmSync(codexConfig, { force: true });
  }
  console.log(`Cleaned project-local development${all ? " including credentials and A2A identity" : "; credentials and A2A identity were preserved"}.`);
}

const [command = "help", ...args] = process.argv.slice(2);
switch (command) {
  case "init": init(); break;
  case "skills": {
    const result = refreshSkills();
    console.log(`Skills refreshed: linked=${result.linked.length} reused-global-same-source=${result.reused.length} conflicts-disabled=${result.conflicts.length}`);
    console.log("Reload Codex/start a new task after changing Skill metadata or membership.");
    break;
  }
  case "env": envCommand(args[0]); break;
  case "doctor": doctor(); break;
  case "stop": stop(); break;
  case "clean": clean(args.includes("--all"), args.includes("--config")); break;
  default:
    console.log("Usage: dev.mjs <init|skills|env [name-or-url]|doctor|stop|clean [--all] [--config]>");
    process.exit(command === "help" ? 0 : 2);
}
