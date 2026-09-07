#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { tmpdir } from "node:os";

const scriptDir = dirname(new URL(import.meta.url).pathname);
const evalDir = resolve(scriptDir, "..");
const skillDir = resolve(evalDir, "..");
const repoRoot = resolve(skillDir, "..", "..");
const evals = JSON.parse(readFileSync(join(evalDir, "evals.json"), "utf8"));
if (process.argv.includes("--help")) {
  console.log("Usage: node skills/okx-ai/evals/scripts/run-codex-evals.mjs [--case <id>] [--baseline]");
  process.exit(0);
}
const selectedId = readOption("--case");
const includeBaseline = process.argv.includes("--baseline");
const selectedEvals = selectedId
  ? evals.evals.filter((scenario) => String(scenario.id) === selectedId)
  : evals.evals;

if (selectedId && selectedEvals.length === 0) {
  throw new Error(`Unknown case: ${selectedId}`);
}

const codexHome = process.env.CODEX_HOME || join(process.env.HOME || "", ".codex");
if (!existsSync(codexHome)) {
  throw new Error("CODEX_HOME is required so codex exec can use its existing authentication.");
}

const runId = new Date().toISOString().replaceAll(":", "-").replace(/\..+/, "");
const outputRoot = resolve(repoRoot, "artifacts", "skill-evals", runId);
mkdirSync(outputRoot, { recursive: true });

const results = [];
for (const scenario of selectedEvals) {
  const withSkill = runScenario(scenario, "with-skill");
  const baseline = includeBaseline ? runScenario(scenario, "baseline") : null;
  results.push({
    id: scenario.id,
    prompt: scenario.prompt,
    with_skill: withSkill,
    baseline,
  });
}

const summary = {
  skill: evals.skill_name,
  output_root: outputRoot,
  passed: results.every((result) => result.with_skill.passed),
  results,
};
writeFileSync(join(outputRoot, "summary.json"), `${JSON.stringify(summary, null, 2)}\n`);
for (const result of results) {
  console.log(`case ${result.id}: ${result.with_skill.passed ? "PASS" : "FAIL"}`);
  for (const failure of result.with_skill.failures) console.log(`  - ${failure}`);
}
console.log(`artifacts: ${outputRoot}`);
process.exitCode = summary.passed ? 0 : 1;

function runScenario(scenario, mode) {
  const scenarioRoot = join(outputRoot, `case-${scenario.id}`, mode);
  const workspace = mkdtempSync(join(tmpdir(), "okx-ai-eval-"));
  const resultDir = join(scenarioRoot, "result");
  mkdirSync(resultDir, { recursive: true });
  prepareWorkspace(workspace, mode === "with-skill");

  const tracePath = join(resultDir, "trace.jsonl");
  const finalPath = join(resultDir, "final.md");
  const command = [
    "--ask-for-approval",
    "never",
    "exec",
    "--json",
    "--ephemeral",
    "--sandbox",
    "read-only",
    "--ignore-user-config",
    "--ignore-rules",
    "--skip-git-repo-check",
    "-C",
    workspace,
    "-o",
    finalPath,
    scenario.prompt,
  ];
  const env = {
    ...process.env,
    HOME: join(scenarioRoot, "home"),
    CODEX_HOME: codexHome,
    PATH: `${join(workspace, "bin")}:${process.env.PATH}`,
    ONCHAINOS_EVAL_FIXTURES: join(evalDir, "fixtures"),
  };
  mkdirSync(env.HOME, { recursive: true, mode: 0o700 });

  try {
    const execution = spawnSync("codex", command, {
      cwd: workspace,
      encoding: "utf8",
      env,
      maxBuffer: 10 * 1024 * 1024,
    });
    writeFileSync(tracePath, execution.stdout || "");
    writeFileSync(join(resultDir, "stderr.log"), execution.stderr || "");

    const commands = readCommands(execution.stdout || "");
    const final = existsSync(finalPath) ? readFileSync(finalPath, "utf8") : "";
    const failures = mode === "with-skill"
      ? evaluate(scenario.assertions, commands, final, execution)
      : [];
    return {
      passed: failures.length === 0,
      exit_code: execution.status,
      commands,
      final,
      failures,
    };
  } finally {
    rmSync(workspace, { recursive: true, force: true });
  }
}

function prepareWorkspace(workspace, loadSkill) {
  mkdirSync(join(workspace, "bin"), { recursive: true });
  link(join(evalDir, "scripts", "onchainos"), join(workspace, "bin", "onchainos"));
  if (!loadSkill) return;

  mkdirSync(join(workspace, ".agents", "skills"), { recursive: true });
  mkdirSync(join(workspace, "skills"), { recursive: true });
  link(skillDir, join(workspace, "skills", "okx-ai"));
  link(join(repoRoot, "skills", "okx-agentic-wallet"), join(workspace, "skills", "okx-agentic-wallet"));
  link(skillDir, join(workspace, ".agents", "skills", "okx-ai"));
  link("../../skills/okx-agentic-wallet", join(workspace, ".agents", "skills", "okx-agentic-wallet"));
}

function link(target, path) {
  if (!existsSync(path)) symlinkSync(target, path);
}

function readCommands(trace) {
  return trace
    .split("\n")
    .flatMap((line) => {
      try {
        const event = JSON.parse(line);
        return event.type === "item.completed" && event.item?.type === "command_execution"
          ? [event.item.command]
          : [];
      } catch {
        return [];
      }
    });
}

function evaluate(assertions = {}, commands, final, execution) {
  const failures = [];
  if (execution.error) failures.push(`codex could not start: ${execution.error.message}`);
  if (execution.status !== 0) failures.push(`codex exited with ${execution.status}`);
  for (const required of assertions.required_commands || []) {
    const matches = commands.filter((command) => command.includes(required));
    if (matches.length !== 1) failures.push(`expected exactly one command: ${required}`);
  }
  for (const forbidden of assertions.forbidden_commands || []) {
    if (commands.some((command) => command.includes(forbidden))) {
      failures.push(`forbidden command observed: ${forbidden}`);
    }
  }
  for (const text of assertions.response_includes || []) {
    if (!final.includes(text)) failures.push(`response missing: ${text}`);
  }
  for (const text of assertions.response_excludes || []) {
    if (final.includes(text)) failures.push(`response must not include: ${text}`);
  }
  return failures;
}

function readOption(name) {
  const index = process.argv.indexOf(name);
  return index === -1 ? null : process.argv[index + 1];
}
