import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { assertLocalA2a, installLocalA2a, prepareLocalA2a } from './a2a.mjs';

const source = path.dirname(fileURLToPath(import.meta.url));
function fixture(t, { ready = true, exit = 0, restartExit = 0, invalid = false, warming = false, stuck = false } = {}) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'a2a-dev-test-')));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const cli = path.join(root, '.codex/a2a/node_modules/@okxweb3/a2a-node/dist/cli.js');
  const bin = path.join(root, '.codex/bin');
  fs.mkdirSync(path.dirname(cli), { recursive: true });
  fs.mkdirSync(bin, { recursive: true });
  fs.mkdirSync(path.join(root, 'scripts/dev'), { recursive: true });
  for (const name of ['common.sh', 'okx-a2a.sh', 'dev.mjs', 'a2a.mjs']) {
    fs.copyFileSync(path.join(source, name), path.join(root, 'scripts/dev', name));
  }
  fs.writeFileSync(cli, `#!/usr/bin/env node
const fs = require('node:fs');
fs.appendFileSync(${JSON.stringify(path.join(root, 'calls.jsonl'))}, JSON.stringify({
  args: process.argv.slice(2), cli: process.env.OKX_AGENT_TASK_CLI_PATH,
  home: process.env.OKX_AGENT_TASK_HOME, wallet: process.env.ONCHAINOS_HOME,
  path: process.env.PATH
}) + '\\n');
if (process.argv[2] === 'daemon') process.exit(${restartExit});
const reportText = ${JSON.stringify(invalid ? 'broken JSON' : JSON.stringify({ ready, cliVersion: 'test', checks: [{ id: 'daemon_running', status: 'pass' }] }))};
if (${warming} || ${stuck}) {
  const report = JSON.parse(reportText);
  const count = fs.readFileSync(${JSON.stringify(path.join(root, 'calls.jsonl'))}, 'utf8').trim().split('\\n').length;
  if (${stuck} || count === 2) report.checks[0].status = 'warn';
  console.log(JSON.stringify(report));
} else console.log(reportText);
process.exit(${exit});
`, { mode: 0o755 });
  const localBin = path.join(root, '.codex/a2a/node_modules/.bin');
  fs.mkdirSync(localBin, { recursive: true });
  fs.symlinkSync(cli, path.join(localBin, 'okx-a2a'));
  fs.symlinkSync(cli, path.join(bin, 'okx-a2a.real'));
  fs.symlinkSync(path.join(root, 'scripts/dev/okx-a2a.sh'), path.join(bin, 'okx-a2a'));
  return { root, cli, calls: () => fs.readFileSync(path.join(root, 'calls.jsonl'), 'utf8').trim().split('\n').map(JSON.parse) };
}

test('refreshes an existing local package without a global install', t => {
  const f = fixture(t);
  const tools = path.join(f.root, 'tools');
  fs.mkdirSync(tools);
  fs.writeFileSync(path.join(tools, 'npm'), `#!/usr/bin/env node
require('node:fs').writeFileSync(${JSON.stringify(path.join(f.root, 'npm.json'))}, JSON.stringify(process.argv.slice(2)));
require('node:fs').writeFileSync(${JSON.stringify(f.cli)}, '#!/usr/bin/env node\\n// upgraded');
`, { mode: 0o755 });
  const oldPath = process.env.PATH;
  process.env.PATH = `${tools}${path.delimiter}${oldPath}`;
  try { installLocalA2a(f.root); } finally { process.env.PATH = oldPath; }
  const args = JSON.parse(fs.readFileSync(path.join(f.root, 'npm.json')));
  assert.ok(args.includes('--global=false'));
  assert.equal(args[args.indexOf('--prefix') + 1], path.join(f.root, '.codex/a2a'));
  assert.ok(args.includes('@okxweb3/a2a-node@latest'));
  assert.match(fs.readFileSync(f.cli, 'utf8'), /upgraded/);
});

test('rejects a local entry pointing to an external package', t => {
  const f = fixture(t);
  const outside = path.join(f.root, 'global-cli.js');
  fs.writeFileSync(outside, '', { mode: 0o755 });
  const entry = path.join(f.root, '.codex/a2a/node_modules/.bin/okx-a2a');
  fs.unlinkSync(entry);
  fs.symlinkSync(outside, entry);
  assert.throws(() => assertLocalA2a(f.root), /outside this project/);
});

test('restarts with project paths and checks readiness without global repair', t => {
  const f = fixture(t);
  assert.equal(prepareLocalA2a(f.root).ready, true);
  const calls = f.calls();
  assert.deepEqual(calls.map(c => c.args), [
    ['daemon', 'restart', '--provider', 'codex', '--no-autostart'],
    ['doctor', '--non-interactive', '--json'],
  ]);
  for (const call of calls) {
    assert.equal(call.cli, f.cli);
    assert.equal(call.home, path.join(f.root, '.codex/runtime/a2a'));
    assert.equal(call.wallet, path.join(f.root, '.codex/runtime/onchainos'));
    assert.equal(call.path.split(path.delimiter)[0], path.join(f.root, '.codex/bin'));
  }
});

for (const options of [{ ready: false }, { exit: 1 }, { invalid: true }, { restartExit: 1 }]) {
  test(`fails the build readiness stage for ${JSON.stringify(options)}`, t => {
    const f = fixture(t, options);
    assert.throws(() => prepareLocalA2a(f.root), /not ready|invalid JSON|restart failed/);
  });
}

test('does not restart A2A when CLI compilation fails', t => {
  const f = fixture(t);
  fs.writeFileSync(path.join(f.root, 'scripts/dev/build-onchainos.sh'), 'exit 23\n');
  const result = spawnSync(process.execPath, [path.join(f.root, 'scripts/dev/dev.mjs'), 'build']);
  assert.equal(result.status, 23);
  assert.equal(fs.existsSync(path.join(f.root, 'calls.jsonl')), false);
});

test('rejects a wrapper pointing to a global package before starting A2A', t => {
  const f = fixture(t);
  const link = path.join(f.root, '.codex/bin/okx-a2a.real');
  fs.unlinkSync(link);
  fs.symlinkSync(process.execPath, link);
  assert.throws(() => prepareLocalA2a(f.root), /does not point to the project package/);
  assert.equal(fs.existsSync(path.join(f.root, 'calls.jsonl')), false);
});


test('rechecks daemon readiness after identity refresh warms up XMTP', t => {
  const f = fixture(t, { warming: true });
  assert.equal(prepareLocalA2a(f.root).checks[0].status, 'pass');
  assert.equal(f.calls().length, 3);
});

test('does not report success while the daemon stays unready', t => {
  const f = fixture(t, { stuck: true });
  assert.throws(() => prepareLocalA2a(f.root), /has not finished starting/);
});

test('rejects a global wrapper even when the package link is local', t => {
  const f = fixture(t);
  const wrapper = path.join(f.root, '.codex/bin/okx-a2a');
  fs.unlinkSync(wrapper);
  fs.symlinkSync(process.execPath, wrapper);
  assert.throws(() => prepareLocalA2a(f.root), /does not point to the project package/);
});

test('build compiles before starting and validating A2A', t => {
  const f = fixture(t);
  const marker = path.join(f.root, 'compiled');
  fs.writeFileSync(path.join(f.root, 'scripts/dev/build-onchainos.sh'), `node -e 'require("node:fs").writeFileSync("compiled", "built")'\n`);
  const script = fs.readFileSync(f.cli, 'utf8');
  fs.writeFileSync(f.cli, script.replace("const fs = require('node:fs');", `const fs = require('node:fs');
if (!fs.existsSync(${JSON.stringify(marker)})) process.exit(42);`));
  const result = spawnSync(process.execPath, [path.join(f.root, 'scripts/dev/dev.mjs'), 'build'], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(f.calls().length, 2);
});
