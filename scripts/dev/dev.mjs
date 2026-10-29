#!/usr/bin/env node

import fs from "node:fs";
import crypto from "node:crypto";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { assertLocalA2a, installLocalA2a, prepareLocalA2a } from "./a2a.mjs";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "../..");
const codexDir = path.join(repoRoot, ".codex");
const binDir = path.join(codexDir, "bin");
const projectSkillsDir = path.join(repoRoot, ".agents", "skills");
const codexConfig = path.join(codexDir, "config.toml");
const codexProfileMarker = path.join(codexDir, "profile-name");
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

  for (const skill of sourceSkills()) {
    const globalMatches = globals.get(skill.name) || [];
    desired.set(skill.name, skill.dir);
    for (const candidate of globalMatches) conflicts.push({ name: skill.name, path: candidate });
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
      const stat = fs.lstatSync(target);
      if (stat.isSymbolicLink()) {
        if (sameFile(target, source)) continue;
        const marker = fs.readlinkSync(target);
        const absolute = path.resolve(path.dirname(target), marker);
        // Project-owned links can become stale when a skill directory is renamed.
        // Refresh those links, but never replace a link managed outside this repo.
        if (absolute.startsWith(`${sourceSkillsDir}${path.sep}`)) {
          fs.unlinkSync(target);
          fs.symlinkSync(source, target, "dir");
          continue;
        }
      }
      fail(`refusing to replace existing project skill entry: ${target}`);
    }
    fs.symlinkSync(source, target, "dir");
  }

  updateConflictConfig(conflicts);
  return { linked: [...desired.keys()], conflicts };
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
    // The Linux development dispatcher may itself be on PATH. Never treat it
    // as the globally installed executable, otherwise .codex/bin/<name>.real
    // points back into this project and commands recurse forever.
    if (process.platform === "linux" && name === "okx-a2a"
      && [linuxA2aDispatcherPath(), linuxOnchainosDispatcherPath(), legacyLinuxCliDispatcherPath()]
        .some((dispatcher) => sameFile(candidate, dispatcher))) continue;
    try { if (fs.statSync(candidate).mode & 0o111) return fs.realpathSync(candidate); } catch {}
  }
  return null;
}

function linkFile(source, target) {
  if (fs.existsSync(target) || fs.lstatSync(target, { throwIfNoEntry: false })) fs.unlinkSync(target);
  fs.symlinkSync(source, target);
}

function linuxA2aLink() {
  return path.join(os.homedir(), ".local", "bin", "okx-a2a");
}

function linuxOnchainosLink() {
  return path.join(os.homedir(), ".local", "bin", "onchainos");
}

function linuxOnchainosRelease() {
  return path.join(os.homedir(), ".local", "bin", "onchainos.release");
}

function linuxOnchainosDispatcherPath() {
  return path.join(codexHome(), "onchainos-linux-dev-dispatch.sh");
}

function linuxA2aDispatcherPath() {
  return path.join(codexHome(), "okx-a2a-linux-dev-dispatch.sh");
}

function legacyLinuxCliDispatcherPath() {
  return path.join(codexHome(), "onchainos-dev-dispatch.sh");
}

function removeLegacyLinuxCliLink() {
  if (process.platform !== "linux") return false;
  const userLink = linuxOnchainosLink();
  const projectWrapper = path.join(binDir, "onchainos");
  try {
    if (!fs.lstatSync(userLink).isSymbolicLink()) return false;
    const target = path.resolve(path.dirname(userLink), fs.readlinkSync(userLink));
    if (target !== projectWrapper) return false;
    fs.unlinkSync(userLink);
    return true;
  } catch (error) {
    if (error.code === "ENOENT") return false;
    throw error;
  }
}

function isManagedCliDispatcher(link) {
  try {
    return fs.lstatSync(link).isSymbolicLink()
      && (sameFile(link, linuxA2aDispatcherPath()) || sameFile(link, linuxOnchainosDispatcherPath())
        || sameFile(link, legacyLinuxCliDispatcherPath()));
  } catch {
    return false;
  }
}

function ensureLinuxCliDispatcher() {
  if (process.platform !== "linux") return;
  const userLink = linuxOnchainosLink();
  const release = linuxOnchainosRelease();
  const existing = fs.lstatSync(userLink, { throwIfNoEntry: false });
  if (existing && !existing.isSymbolicLink()) {
    if (fs.existsSync(release)) fs.rmSync(release);
    fs.renameSync(userLink, release);
  }
  ensureLinuxDispatcher(userLink, linuxOnchainosDispatcherPath(), path.join(scriptDir, "onchainos-linux-dispatch.sh"));
  ensureLinuxDispatcher(linuxA2aLink(), linuxA2aDispatcherPath(), path.join(scriptDir, "okx-a2a-linux-dispatch.sh"));
}

function ensureLinuxDispatcher(userLink, dispatcher, source) {
  fs.mkdirSync(codexHome(), { recursive: true, mode: 0o700 });
  if (fs.existsSync(dispatcher) && !fs.readFileSync(dispatcher, "utf8").includes("# onchainos-dev generated")) {
    fail(`refusing to replace existing Codex CLI dispatcher: ${dispatcher}`);
  }
  fs.copyFileSync(source, dispatcher);
  fs.chmodSync(dispatcher, 0o755);
  const existing = fs.lstatSync(userLink, { throwIfNoEntry: false });
  if (existing && !isManagedCliDispatcher(userLink)) {
    fail(`refusing to replace existing ~/.local/bin/onchainos; remove or rename it before enabling the development dispatcher`);
  }
  const alreadyLinked = Boolean(existing) && sameFile(userLink, dispatcher);
  if (existing && !alreadyLinked) fs.unlinkSync(userLink);
  if (!alreadyLinked) {
    fs.mkdirSync(path.dirname(userLink), { recursive: true, mode: 0o755 });
    fs.symlinkSync(dispatcher, userLink);
  }
  console.log(`Linux dispatcher: ${userLink} -> ${dispatcher}`);
}

function removeLinuxCliDispatcherIfUnused() {
  if (process.platform !== "linux") return;
  const profilesDir = codexHome();
  let hasOnchainosProfile = false;
  try {
    hasOnchainosProfile = fs.readdirSync(profilesDir).some((entry) => /^onchainos-[A-Za-z0-9_-]+\.config\.toml$/.test(entry));
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
  const links = [[linuxA2aLink(), linuxA2aDispatcherPath()]];
  if (!hasOnchainosProfile) {
    for (const [userLink, dispatcher] of links) {
      if (!isManagedCliDispatcher(userLink)) continue;
      fs.unlinkSync(userLink);
      fs.rmSync(dispatcher, { force: true });
      console.log(`Removed unused Linux dispatcher: ${userLink}`);
    }
    fs.rmSync(legacyLinuxCliDispatcherPath(), { force: true });
  }
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

function codexHome() {
  return process.env.CODEX_HOME || path.join(os.homedir(), ".codex");
}

function profileNameForRepo() {
  const base = path.basename(repoRoot).replaceAll(/[^A-Za-z0-9_-]/g, "-") || "workspace";
  const suffix = crypto.createHash("sha256").update(fs.realpathSync(repoRoot)).digest("hex").slice(0, 12);
  return `onchainos-${base}-${suffix}`;
}

function readProfileMarker() {
  try {
    const profile = fs.readFileSync(codexProfileMarker, "utf8").trim();
    return /^[A-Za-z0-9_-]+$/.test(profile) ? profile : null;
  } catch (error) {
    if (error.code === "ENOENT") return null;
    throw error;
  }
}

function ensureCodexProfile() {
  const profile = readProfileMarker() || profileNameForRepo();
  const profileFile = path.join(codexHome(), `${profile}.config.toml`);
  fs.mkdirSync(codexHome(), { recursive: true, mode: 0o700 });

  const existing = fs.lstatSync(profileFile, { throwIfNoEntry: false });
  if (existing) {
    if (!existing.isSymbolicLink() || !sameFile(profileFile, codexConfig)) {
      fail(`refusing to replace existing Codex profile: ${profileFile}`);
    }
  } else {
    fs.symlinkSync(codexConfig, profileFile);
  }
  fs.writeFileSync(codexProfileMarker, `${profile}\n`, { mode: 0o600 });
  chmodPrivate(codexProfileMarker);
  return { profile, profileFile };
}

function removeCodexProfile() {
  const profile = readProfileMarker();
  if (!profile) return false;
  const profileFile = path.join(codexHome(), `${profile}.config.toml`);
  try {
    if (fs.lstatSync(profileFile).isSymbolicLink() && sameFile(profileFile, codexConfig)) {
      fs.unlinkSync(profileFile);
      console.log(`Removed Codex development profile: ${profileFile}`);
    }
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
  fs.unlinkSync(codexProfileMarker);
  return true;
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

  const a2a = installLocalA2a(repoRoot);
  linkFile(path.join(scriptDir, "onchainos.sh"), path.join(binDir, "onchainos"));
  linkFile(path.join(scriptDir, "okx-a2a.sh"), path.join(binDir, "okx-a2a"));
  linkFile(a2a, path.join(binDir, "okx-a2a.real"));
  const removedLegacyLink = removeLegacyLinuxCliLink();
  ensureLinuxCliDispatcher();
  updateCodexPath();
  const profile = ensureCodexProfile();
  const skills = refreshSkills();

  console.log("Initialized project-local OnchainOS development.");
  console.log("Run npm run build to compile the CLI and start/verify project-local A2A.");
  console.log(`  CLI:    ${path.join(binDir, "onchainos")}`);
  console.log(`  A2A:    ${path.join(binDir, "okx-a2a")} -> ${a2a}`);
  console.log(`  Codex:  -C ${repoRoot} -p ${profile.profile} --disable shell_snapshot`);
  console.log(`  Profile: ${profile.profileFile} -> ${codexConfig}`);
  console.log(`  Skills: linked=${skills.linked.length} global-conflicts-disabled=${skills.conflicts.length}`);
  if (removedLegacyLink) console.log("Replaced the old worktree-specific Linux CLI link with the dispatcher.");
  console.log("Source scripts/dev/codex-shell.sh once in your interactive shell to make bare `codex` add these options from this worktree.");
}

function build() {
  assertLocalA2a(repoRoot);
  const result = spawnSync("bash", [path.join(scriptDir, "build-onchainos.sh")], { cwd: repoRoot, stdio: "inherit" });
  if (result.error) fail(`failed to start CLI build: ${result.error.message}`);
  if (result.status !== 0) process.exit(result.status ?? 1);
  prepareLocalA2a(repoRoot);
}

function doctor() {
  const errors = [];
  const expected = { onchainos: path.join(binDir, "onchainos"), "okx-a2a": path.join(binDir, "okx-a2a") };
  for (const [name, target] of Object.entries(expected)) {
    try { if (!(fs.statSync(target).mode & 0o111)) errors.push(`${name} wrapper is not executable`); }
    catch { errors.push(`${name} wrapper is missing`); }
  }
  try { if (!(fs.statSync(path.join(binDir, "okx-a2a.real")).mode & 0o111)) errors.push("okx-a2a.real is not executable"); }
  catch { errors.push("okx-a2a.real is missing or broken"); }
  const configText = fs.existsSync(codexConfig) ? fs.readFileSync(codexConfig, "utf8") : "";
  if (!configText.includes(binDir)) errors.push(".codex/config.toml does not put .codex/bin on PATH");
  const profile = readProfileMarker();
  if (!profile) errors.push(".codex/profile-name is missing or invalid");
  else {
    const profileFile = path.join(codexHome(), `${profile}.config.toml`);
    if (!sameFile(profileFile, codexConfig)) errors.push(`Codex profile does not point to this worktree: ${profileFile}`);
  }

  const skills = sourceSkills();
  for (const skill of skills) {
    const local = path.join(projectSkillsDir, skill.name);
    if (!sameFile(local, skill.dir)) errors.push(`skill '${skill.name}' is not linked from the current project`);
  }

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
  if (!fs.existsSync(wrapper)) { if (!quiet) fail("A2A wrapper is missing; run npm run setup"); return; }
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

function clean() {
  stop();
  removeOwnedProjectSkills();
  removeLegacyLinuxCliLink();
  removeCodexProfile();
  removeLinuxCliDispatcherIfUnused();
  for (const relative of ["bin", "a2a", "build", "runtime/a2a-spool", "runtime/tmp"]) fs.rmSync(path.join(codexDir, relative), { recursive: true, force: true });
  fs.rmSync(path.join(codexDir, "runtime", "onchainos"), { recursive: true, force: true });
  fs.rmSync(path.join(codexDir, "runtime", "a2a"), { recursive: true, force: true });
  fs.rmSync(codexConfig, { force: true });
  console.log("Cleaned project-local development including credentials, A2A identity, and Codex configuration.");
}

const [command = "help", ...args] = process.argv.slice(2);
switch (command) {
  case "init": init(); break;
  case "build": build(); break;
  case "doctor": doctor(); break;
  case "stop": stop(); break;
  case "clean": clean(); break;
  default:
    console.log("Usage: dev.mjs <init|build|doctor|stop|clean>");
    process.exit(command === "help" ? 0 : 2);
}
