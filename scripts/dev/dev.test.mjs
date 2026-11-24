import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';

const source = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(source, '../..');
const marker = '# onchainos-dev generated skill conflict';

function fixture(t, config = '') {
  const root = fs.mkdtempSync(path.join(repoRoot, '.tmpdir-dev-test-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const scripts = path.join(root, 'scripts/dev');
  const home = path.join(root, 'home');
  const tools = path.join(root, 'tools');
  for (const dir of [scripts, home, tools, path.join(root, 'skills'), path.join(root, '.codex')]) fs.mkdirSync(dir, { recursive: true });
  for (const name of fs.readdirSync(source)) {
    if (!name.endsWith('.test.mjs')) fs.copyFileSync(path.join(source, name), path.join(scripts, name));
  }
  fs.writeFileSync(path.join(root, '.codex/config.toml'), config);
  // Only the external package installation is stubbed; run the actual setup.
  fs.writeFileSync(path.join(tools, 'npm'), `#!${process.execPath}
const fs = require('node:fs'); const path = require('node:path');
const prefix = process.argv[process.argv.indexOf('--prefix') + 1];
const cli = path.join(prefix, 'node_modules/@okxweb3/a2a-node/dist/cli.js');
fs.mkdirSync(path.dirname(cli), {recursive:true});
fs.writeFileSync(cli, '#!/usr/bin/env node\\nprocess.exit(0);\\n', {mode:0o700});
const bin = path.join(prefix, 'node_modules/.bin'); fs.mkdirSync(bin, {recursive:true});
const entry = path.join(bin, 'okx-a2a'); if (!fs.existsSync(entry)) fs.symlinkSync(cli, entry);
`, { mode: 0o700 });
  const binary = path.join(root, '.codex/build/cargo-target/debug/onchainos');
  fs.mkdirSync(path.dirname(binary), { recursive: true });
  fs.writeFileSync(binary, '#!/bin/sh\nexit 99\n', { mode: 0o700 });
  const env = { ...process.env, HOME: home, CODEX_HOME: path.join(home, '.codex'),
    PATH: `${tools}${path.delimiter}${process.env.PATH}`, TMPDIR: '/caller/tmp/never-used' };
  return { root, env, config: path.join(root, '.codex/config.toml') };
}

function run(f, command) {
  return spawnSync(process.execPath, [path.join(f.root, 'scripts/dev/dev.mjs'), command], {
    cwd: f.root, env: f.env, encoding: 'utf8',
  });
}
function setup(f) {
  const result = run(f, 'init');
  assert.equal(result.status, 0, result.stderr + result.stdout);
  return result;
}
function assertConfig(f) {
  const text = fs.readFileSync(f.config, 'utf8');
  assert.equal((text.match(/^TMPDIR\s*=/gm) || []).length, 1);
  assert.equal((text.match(/^PATH\s*=/gm) || []).length, 1);
  assert.ok(text.includes(`TMPDIR = ${JSON.stringify(path.join(f.root, '.codex/runtime/tmp'))}`));
  assert.equal(fs.statSync(path.join(f.root, '.codex/runtime/tmp')).mode & 0o777, 0o700);
  assert.equal(fs.statSync(f.config).mode & 0o777, 0o600);
  return text;
}

test('first setup configures project TMPDIR and existing Codex profile; repeat is byte-identical', t => {
  const f = fixture(t);
  const result = setup(f);
  const first = assertConfig(f);
  assert.match(result.stdout, /Restart old Codex sessions and the A2A daemon/);
  assert.match(result.stdout, /project development profile/);
  setup(f);
  assert.equal(assertConfig(f), first);
  const profile = fs.readFileSync(path.join(f.root, '.codex/profile-name'), 'utf8').trim();
  assert.equal(fs.realpathSync(path.join(f.env.CODEX_HOME, `${profile}.config.toml`)), f.config);
});

test('setup replaces existing TMPDIR and PATH on later lines, preserving all unrelated config', t => {
  const f = fixture(t, `model = "keep-model"
[shell_environment_policy]
exclude = ["KEEP"]
inherit = "none"
[shell_environment_policy.set]
KEEP = "preserved"
PATH = "/old/path"
TMPDIR = "/private/var/old"
[other]
PATH = "unrelated"
TMPDIR = "unrelated"
`);
  setup(f);
  const text = fs.readFileSync(f.config, 'utf8');
  assert.match(text, /model = "keep-model"/);
  assert.match(text, /exclude = \["KEEP"\]/);
  assert.match(text, /inherit = "all"/);
  assert.match(text, /KEEP = "preserved"/);
  assert.ok(text.includes('[other]\nPATH = "unrelated"\nTMPDIR = "unrelated"'));
  assert.equal((text.match(/^TMPDIR\s*=/gm) || []).length, 2);
  assert.equal((text.match(/^PATH\s*=/gm) || []).length, 2);
  setup(f);
  assert.equal(fs.readFileSync(f.config, 'utf8'), text);
});

test('empty set section inserts at its own offset and keeps following table', t => {
  const f = fixture(t, '[shell_environment_policy]\ninherit = "all"\n[shell_environment_policy.set]\n[other]\nkeep = true\n');
  setup(f);
  const text = assertConfig(f);
  assert.match(text, /\[shell_environment_policy.set\]\n[\s\S]*PATH =/);
  assert.match(text, /\[other\]\nkeep = true/);
});

for (const conflictRemains of [false, true]) {
  test(`generated conflict immediately after set section cannot swallow TMPDIR (rebuild=${conflictRemains})`, t => {
    const f = fixture(t, `[shell_environment_policy]\ninherit = "all"\n[shell_environment_policy.set]\nPATH = "/old/path"\n${marker}\n[[skills.config]]\npath = "/old/skill/SKILL.md"\nenabled = false\n[other]\nkeep = true\n`);
    if (conflictRemains) {
      for (const dir of [path.join(f.root, 'skills/test-skill'), path.join(f.env.HOME, '.agents/skills/test-skill')]) {
        fs.mkdirSync(dir, { recursive: true });
        fs.writeFileSync(path.join(dir, 'SKILL.md'), '---\nname: test-skill\n---\n');
      }
    }
    setup(f);
    const first = assertConfig(f);
    assert.match(first, /keep = true/);
    assert.equal(first.includes(marker), conflictRemains);
    if (conflictRemains) assert.ok(first.indexOf('TMPDIR =') < first.indexOf(marker));
    setup(f);
    assert.equal(assertConfig(f), first);
  });
}

test('doctor checks profile and actual wrappers without executing consumers', t => {
  const f = fixture(t);
  setup(f);
  const result = run(f, 'doctor');
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /profile and CLI\/A2A wrappers use the same project TMPDIR/);
  assert.match(result.stdout, /Node probe does not guarantee Rust secure staging/);
  assert.deepEqual(fs.readdirSync(path.join(f.root, '.codex/runtime/tmp')), []);
});

test('doctor reports a stale profile TMPDIR even if caller happens to match', t => {
  const f = fixture(t);
  setup(f);
  fs.writeFileSync(f.config, fs.readFileSync(f.config, 'utf8').replace(/^TMPDIR =.*$/m, 'TMPDIR = "/stale/profile"'));
  const result = run(f, 'doctor');
  assert.equal(result.status, 1);
  assert.match(result.stderr, /development profile TMPDIR does not match/);
});

test('doctor reports an actual wrapper runtime mismatch', t => {
  const f = fixture(t);
  setup(f);
  const common = path.join(f.root, 'scripts/dev/common.sh');
  fs.writeFileSync(common, fs.readFileSync(common, 'utf8').replace('export TMPDIR="$dev_repo_root/.codex/runtime/tmp"', 'export TMPDIR="$dev_repo_root/.codex/runtime/wrong-tmp"'));
  const result = run(f, 'doctor');
  assert.equal(result.status, 1);
  assert.match(result.stderr, /TMPDIR resolves differently between development profile and .* wrapper/);
});

test('doctor reports a missing or non-private project temp directory', t => {
  const f = fixture(t);
  setup(f);
  const tmp = path.join(f.root, '.codex/runtime/tmp');
  fs.chmodSync(tmp, 0o755);
  assert.match(run(f, 'doctor').stderr, /must have mode 0700/);
  fs.rmSync(tmp, { recursive: true });
  assert.match(run(f, 'doctor').stderr, /project temporary directory does not exist/);
});

test('setup supports a final empty set section without a trailing newline', t => {
  const f = fixture(t, '[shell_environment_policy]\ninherit = "all"\n[shell_environment_policy.set]');
  setup(f);
  const first = assertConfig(f);
  setup(f);
  assert.equal(assertConfig(f), first);
});
