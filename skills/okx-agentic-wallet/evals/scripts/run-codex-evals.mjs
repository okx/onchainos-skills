#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { tmpdir } from "node:os";

const scriptDir = dirname(new URL(import.meta.url).pathname);
const evalDir = resolve(scriptDir, "..");
const skillDir = resolve(evalDir, "..");
const repoRoot = resolve(skillDir, "..", "..");
const suite = JSON.parse(readFileSync(join(evalDir, "evals.json"), "utf8"));
const selectedId = readOption("--case");
const scenarios = selectedId
  ? suite.evals.filter((scenario) => scenario.id === selectedId)
  : suite.evals;

if (selectedId && scenarios.length === 0) {
  throw new Error(`Unknown case: ${selectedId}`);
}

const codexHome = process.env.CODEX_HOME || join(process.env.HOME || "", ".codex");
if (!existsSync(codexHome)) {
  throw new Error("CODEX_HOME is required so codex exec can use existing authentication.");
}

const runId = new Date().toISOString().replaceAll(":", "-").replace(/\..+/, "");
const outputRoot = resolve(repoRoot, "artifacts", "skill-evals", `wallet-${runId}`);
mkdirSync(outputRoot, { recursive: true });

const results = scenarios.map(runScenario);
const summary = {
  skill: suite.skill_name,
  output_root: outputRoot,
  passed: results.every((result) => result.passed),
  results,
};
writeFileSync(join(outputRoot, "summary.json"), `${JSON.stringify(summary, null, 2)}\n`);

for (const result of results) {
  console.log(`case ${result.id}: ${result.passed ? "PASS" : "FAIL"}`);
  console.log(`  actual commands: ${JSON.stringify(result.actual_commands)}`);
  for (const failure of result.failures) console.log(`  - ${failure}`);
}
console.log(`artifacts: ${outputRoot}`);
process.exitCode = summary.passed ? 0 : 1;

function runScenario(scenario) {
  const workspace = mkdtempSync(join(tmpdir(), "okx-wallet-eval-"));
  const resultDir = join(outputRoot, scenario.id);
  const homeDir = join(resultDir, "home");
  const finalPath = join(resultDir, "final.md");
  const tracePath = join(resultDir, "trace.jsonl");
  mkdirSync(resultDir, { recursive: true });
  mkdirSync(homeDir, { recursive: true, mode: 0o700 });
  prepareWorkspace(workspace);

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
    HOME: homeDir,
    CODEX_HOME: codexHome,
    PATH: `${join(workspace, "bin")}:${process.env.PATH}`,
  };

  try {
    const execution = spawnSync("codex", command, {
      cwd: workspace,
      encoding: "utf8",
      env,
      maxBuffer: 10 * 1024 * 1024,
    });
    const trace = execution.stdout || "";
    writeFileSync(tracePath, trace);
    writeFileSync(join(resultDir, "stderr.log"), execution.stderr || "");

    const actualCommands = readOnchainosCommands(trace);
    const failures = evaluate(scenario, actualCommands, execution);
    return {
      id: scenario.id,
      passed: failures.length === 0,
      exit_code: execution.status,
      actual_commands: actualCommands,
      failures,
    };
  } finally {
    rmSync(workspace, { recursive: true, force: true });
  }
}

function prepareWorkspace(workspace) {
  const binDir = join(workspace, "bin");
  const skillsDir = join(workspace, ".agents", "skills");
  mkdirSync(binDir, { recursive: true });
  mkdirSync(skillsDir, { recursive: true });
  symlinkSync(join(evalDir, "scripts", "onchainos"), join(binDir, "onchainos"));
  symlinkSync(skillDir, join(skillsDir, "okx-agentic-wallet"));
  chmodSync(join(evalDir, "scripts", "onchainos"), 0o755);
}

function readOnchainosCommands(trace) {
  return trace
    .split("\n")
    .flatMap((line) => {
      try {
        const event = JSON.parse(line);
        const command = event.type === "item.completed" && event.item?.type === "command_execution"
          ? event.item.command
          : "";
        return /(^|[\s'"=])onchainos\s/.test(command) ? [command] : [];
      } catch {
        return [];
      }
    });
}

function evaluate(scenario, actualCommands, execution) {
  const failures = [];
  if (execution.error) failures.push(`codex could not start: ${execution.error.message}`);
  if (execution.status !== 0) failures.push(`codex exited with ${execution.status}`);

  for (const required of scenario.required_commands) {
    const count = actualCommands.filter((actual) => actual.includes(required)).length;
    if (count !== 1) failures.push(`expected exactly one command containing '${required}', observed ${count}`);
  }
  if (actualCommands.length !== scenario.required_commands.length) {
    failures.push(
      `expected ${scenario.required_commands.length} total onchainos command(s), observed ${actualCommands.length}`,
    );
  }
  for (const forbidden of scenario.forbidden_commands) {
    if (actualCommands.some((actual) => actual.includes(forbidden))) {
      failures.push(`forbidden command fragment observed: ${forbidden}`);
    }
  }
  return failures;
}

function readOption(name) {
  const index = process.argv.indexOf(name);
  return index === -1 ? null : process.argv[index + 1];
}
