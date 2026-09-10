#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const skillDir = path.resolve(scriptDir, "../..");
const repoDir = path.resolve(skillDir, "../..");
const referencesDir = path.join(skillDir, "references");
const errors = [];

function walk(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const target = path.join(dir, entry.name);
    return entry.isDirectory() ? walk(target) : [target];
  });
}

function walkDirectories(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    if (!entry.isDirectory()) return [];
    const target = path.join(dir, entry.name);
    return [target, ...walkDirectories(target)];
  });
}

function relative(file) {
  return path.relative(repoDir, file).split(path.sep).join("/");
}

const markdownFiles = walk(skillDir).filter((file) => file.endsWith(".md"));

for (const dir of walkDirectories(referencesDir)) {
  const parts = path.relative(referencesDir, dir).split(path.sep);
  const isRoleDirectory = parts.length === 2
    && parts[0] === "a2a"
    && ["user", "provider", "evaluator"].includes(parts[1]);
  if (parts.length > 1 && !isRoleDirectory) {
    errors.push(`${relative(dir)}: only a2a/user, a2a/provider, and a2a/evaluator may extend the domain directory`);
  }
}

for (const file of markdownFiles) {
  const source = fs.readFileSync(file, "utf8");
  const lineCount = source === "" ? 0 : source.split("\n").length;
  if (lineCount > 500) {
    errors.push(`${relative(file)}: ${lineCount} lines (maximum 500)`);
  }

  if (file.startsWith(`${referencesDir}${path.sep}`)) {
    const parts = path.relative(referencesDir, file).split(path.sep);
    const isRoleLeaf = parts.length === 3
      && parts[0] === "a2a"
      && ["user", "provider", "evaluator"].includes(parts[1]);
    if (parts.length !== 2 && !isRoleLeaf) {
      errors.push(`${relative(file)}: references must be domain/file.md or a2a/<role>/file.md`);
    }
  }

  for (const match of source.matchAll(/\[[^\]]*\]\(([^)]+)\)/g)) {
    const rawTarget = match[1].split("#", 1)[0];
    if (!rawTarget || /^(?:https?:|mailto:)/.test(rawTarget)) continue;
    const resolved = path.resolve(path.dirname(file), rawTarget);
    if (!fs.existsSync(resolved)) {
      errors.push(`${relative(file)}: broken link ${match[1]}`);
    }
  }
}

const runtimeFiles = [
  ...walk(path.join(repoDir, "skills")).filter((file) => /\.(?:md|json|mjs)$/.test(file)),
  ...walk(path.join(repoDir, "cli", "src")).filter((file) => file.endsWith(".rs")),
].filter((file) => file !== fileURLToPath(import.meta.url));

for (const file of runtimeFiles) {
  const source = fs
    .readFileSync(file, "utf8")
    .split("\n")
    .filter((line) => !line.includes("assert!(!"))
    .join("\n");
  const retiredSkillPath = ["skills/okx-ai-v", "2/references/"].join("");
  if (source.includes(retiredSkillPath)) {
    errors.push(`${relative(file)}: retired OKX.AI runtime reference`);
  }
  if (source.includes("[SKILL_PREFETCH]") && source.includes("via references/a2a/router.md")) {
    errors.push(`${relative(file)}: SKILL_PREFETCH must re-enter through Top-level routing, not force a2a/router.md`);
  }
}

const forbiddenDispatchers = [
  "references/shared/task-action-routing.md",
  "references/shared/task-output-templates.md",
  "references/shared/task-cli-reference.md",
];
for (const target of forbiddenDispatchers) {
  if (fs.existsSync(path.join(skillDir, target))) {
    errors.push(`${target}: retired dispatcher/encyclopedia still exists`);
  }
}

const routingContracts = [
  ["references/a2mcp/handoff.md", [/\]\(invoke\.md(?:#[^)]+)?\)/], [/\]\(router\.md(?:#[^)]+)?\)/]],
  ["references/a2a/user/create-prepare.md", [/\]\(\.\.\/\.\.\/a2mcp\/handoff\.md(?:#[^)]+)?\)/], [/a2mcp\/router\.md/]],
  ["references/a2a/evaluator/evidence.md", [], [/\]\(router\.md(?:#[^)]+)?\)/]],
  ["references/a2a/provider/arbitration-decision.md", [], [/\]\(\.\.\/router\.md(?:#[^)]+)?\)/]],
  ["SKILL.md", [/\]\(references\/a2a\/peer\.md\)/, /\[SKILL_PREFETCH\]/], [/^## Envelope precedence$/m]],
  ["references/a2a/router.md", [], [/msgType:\s*"a2a-agent-chat"/, /\[SKILL_PREFETCH\]/]],
  ["references/a2a/peer.md", [/\]\(user\/intake\.md\)/, /\]\(params\.md\)/, /\]\(user\/subscription-signal\.md\)/, /\]\(provider\/assignment\.md\)/], []],
];
for (const [target, required, forbidden] of routingContracts) {
  const file = path.join(skillDir, target);
  if (!fs.existsSync(file)) {
    errors.push(`${target}: routing contract owner is missing`);
    continue;
  }
  const source = fs.readFileSync(file, "utf8");
  for (const pattern of required) {
    if (!pattern.test(source)) errors.push(`${target}: missing required routing contract ${pattern}`);
  }
  for (const pattern of forbidden) {
    if (pattern.test(source)) errors.push(`${target}: contains forbidden routing contract ${pattern}`);
  }
}

const serviceContractPath = path.join(skillDir, "references/identity/service-contract.md");
if (!fs.existsSync(serviceContractPath)) {
  errors.push("references/identity/service-contract.md: service contract owner is missing");
} else {
  const source = fs.readFileSync(serviceContractPath, "utf8");
  const serviceTypeSection = source.match(/### serviceType\n([\s\S]*?)(?=\n### )/)?.[1] || "";
  const developerDocsLink = "[Developer Documentation](https://web3.okx.com/onchainos/dev-docs/okxai/asp)";
  if (!serviceTypeSection.includes(developerDocsLink)) {
    errors.push("references/identity/service-contract.md: service type prompt is missing the ASP developer documentation link");
  }
}

const requiredOwners = [
  "references/a2a/router.md",
  "references/a2a/user/router.md",
  "references/a2a/provider/router.md",
  "references/a2a/evaluator/router.md",
  "references/a2a/user/create-prepare.md",
  "references/a2a/user/create.md",
  "references/a2a/provider/assignment.md",
  "references/a2a/provider/delivery.md",
  "references/a2a/user/review.md",
  "references/a2a/user/refund-prepare.md",
  "references/a2a/user/refund-execute.md",
  "references/a2a/refund-reconcile.md",
  "references/a2a/completion.md",
  "references/runtime/decision-relay.md",
  "references/runtime/cleanup.md",
  "references/shared/refund-contract.md",
];
for (const target of requiredOwners) {
  if (!fs.existsSync(path.join(skillDir, target))) {
    errors.push(`${target}: required V2 owner is missing`);
  }
}

const contractsPath = path.join(skillDir, "evals/fixtures/task-contracts.json");
const tracesPath = path.join(skillDir, "evals/fixtures/task-traces.json");
for (const fixturePath of [contractsPath, tracesPath]) {
  if (!fs.existsSync(fixturePath)) {
    errors.push(`${relative(fixturePath)}: required task fixture is missing`);
  }
}

if (fs.existsSync(contractsPath)) {
  const fixture = JSON.parse(fs.readFileSync(contractsPath, "utf8"));
  const ids = new Set();
  for (const contract of fixture.contracts || []) {
    if (!contract.id || ids.has(contract.id)) {
      errors.push(`${relative(contractsPath)}: missing or duplicate contract id ${contract.id || "<empty>"}`);
    }
    ids.add(contract.id);
    if (!contract.role || !contract.stateGate || !contract.sideEffectOwner) {
      errors.push(`${relative(contractsPath)}: ${contract.id} lacks role/stateGate/sideEffectOwner`);
    }
    if (!contract.targetLeaf || !fs.existsSync(path.join(skillDir, contract.targetLeaf))) {
      errors.push(`${relative(contractsPath)}: ${contract.id} has missing target leaf ${contract.targetLeaf}`);
    }
  }
}

if (fs.existsSync(tracesPath)) {
  const fixture = JSON.parse(fs.readFileSync(tracesPath, "utf8"));
  for (const trace of fixture.traces || []) {
    if (!trace.id) errors.push(`${relative(tracesPath)}: trace is missing id`);
    for (const step of trace.steps || []) {
      if (/task-action-routing|task-output-templates/.test(step.route || "")) {
        errors.push(`${relative(tracesPath)}: ${trace.id} references retired routing`);
      }
    }
  }
}

if (errors.length > 0) {
  console.error(`okx-ai audit failed (${errors.length} issues):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}

console.log(`okx-ai audit passed: ${markdownFiles.length} Markdown files checked`);
