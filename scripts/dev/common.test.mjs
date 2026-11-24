import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';

const source = path.dirname(fileURLToPath(import.meta.url));
const consumer = "const keys = ['TMPDIR', 'ONCHAINOS_HOME', 'OKX_AGENT_TASK_HOME', 'ONCHAINOS_A2A_SPOOL_DIR', 'CARGO_HOME', 'CARGO_TARGET_DIR']; const output = Object.fromEntries(keys.map(key => [key, Object.hasOwn(process.env, key) ? process.env[key] : null])); process.stdout.write(JSON.stringify(output));";

function fixture(t) {
  const root = fs.mkdtempSync(path.join(process.cwd(), '.tmp-common-test-'));
  const scriptDir = path.join(root, 'scripts/dev');
  fs.mkdirSync(scriptDir, { recursive: true });
  for (const name of ['common.sh', 'onchainos.sh', 'okx-a2a.sh']) {
    fs.copyFileSync(path.join(source, name), path.join(scriptDir, name));
  }
  fs.mkdirSync(path.join(root, '.codex/build/cargo-target/debug'), { recursive: true });
  fs.mkdirSync(path.join(root, '.codex/bin'), { recursive: true });
  fs.writeFileSync(path.join(root, '.codex/build/cargo-target/debug/onchainos'), `#!/usr/bin/env node\n${consumer}\n`, { mode: 0o700 });
  fs.writeFileSync(path.join(root, '.codex/bin/okx-a2a.real'), `#!/usr/bin/env node\n${consumer}\n`, { mode: 0o700 });
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  return { root, scriptDir };
}

function run(command, args, env) {
  const result = spawnSync(command, args, { env, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  return JSON.parse(result.stdout);
}

function shellEnv(overrides = {}) {
  const env = { ...process.env };
  if (Object.hasOwn(overrides, 'TMPDIR')) env.TMPDIR = overrides.TMPDIR;
  else delete env.TMPDIR;
  return env;
}

function sourceCommon(f, temporaryDirectory) {
  const bashArgs = ['-c', 'source "$1"; load_dev_runtime; node -e "$2"', '--', path.join(f.scriptDir, 'common.sh'), consumer];
  return run('bash', bashArgs, shellEnv(temporaryDirectory));
}

test('uses the project temporary directory and leaves a caller TMPDIR unchanged', t => {
  const f = fixture(t);
  const temporaryDirectory = path.join(f.root, 'caller temporary directory');
  fs.mkdirSync(temporaryDirectory, { mode: 0o770 });
  fs.chmodSync(temporaryDirectory, 0o770);

  const actual = sourceCommon(f, { TMPDIR: temporaryDirectory });

  assert.equal(actual.TMPDIR, path.join(f.root, '.codex/runtime/tmp'));
  assert.equal(fs.statSync(temporaryDirectory).mode & 0o777, 0o770);
  assert.equal(actual.ONCHAINOS_HOME, path.join(f.root, '.codex/runtime/onchainos'));
  assert.equal(actual.OKX_AGENT_TASK_HOME, path.join(f.root, '.codex/runtime/a2a'));
  assert.equal(actual.ONCHAINOS_A2A_SPOOL_DIR, path.join(f.root, '.codex/runtime/a2a-spool'));
  assert.equal(actual.CARGO_HOME, path.join(f.root, '.codex/build/cargo-home'));
  assert.equal(actual.CARGO_TARGET_DIR, path.join(f.root, '.codex/build/cargo-target'));
  for (const relative of ['runtime/onchainos', 'runtime/a2a', 'runtime/a2a-spool', 'build/cargo-home']) {
    assert.equal(fs.statSync(path.join(f.root, '.codex', relative)).mode & 0o777, 0o700);
  }
});

test('sets and protects the project temporary directory when the caller has no TMPDIR', t => {
  const f = fixture(t);
  const actual = sourceCommon(f);

  assert.equal(actual.TMPDIR, path.join(f.root, '.codex/runtime/tmp'));
  assert.equal(fs.statSync(actual.TMPDIR).mode & 0o777, 0o700);
});

test('overrides an explicitly empty TMPDIR for project development processes', t => {
  const f = fixture(t);
  const actual = sourceCommon(f, { TMPDIR: '' });

  assert.equal(actual.TMPDIR, path.join(f.root, '.codex/runtime/tmp'));
  assert.equal(fs.statSync(actual.TMPDIR).mode & 0o777, 0o700);
});

test('uses project TMPDIR without modifying a caller symlink target', t => {
  const f = fixture(t);
  const target = path.join(f.root, 'caller temporary target');
  const alias = path.join(f.root, 'caller temporary alias');
  fs.mkdirSync(target, { mode: 0o750 });
  fs.chmodSync(target, 0o750);
  fs.symlinkSync(target, alias);

  const actual = sourceCommon(f, { TMPDIR: alias });

  assert.equal(actual.TMPDIR, path.join(f.root, '.codex/runtime/tmp'));
  assert.equal(fs.statSync(target).mode & 0o777, 0o750);
});

test('uses project TMPDIR without creating a missing caller directory', t => {
  const f = fixture(t);
  const missingDirectory = path.join(f.root, 'caller directory that does not exist');

  const actual = sourceCommon(f, { TMPDIR: missingDirectory });

  assert.equal(actual.TMPDIR, path.join(f.root, '.codex/runtime/tmp'));
  assert.equal(fs.existsSync(missingDirectory), false);
});

test('CLI and A2A wrappers pass the same project temporary directory to their consumers', t => {
  const f = fixture(t);
  const temporaryDirectory = path.join(f.root, 'caller temporary directory');
  fs.mkdirSync(temporaryDirectory);
  const env = shellEnv({ TMPDIR: temporaryDirectory });

  const cli = run('bash', [path.join(f.scriptDir, 'onchainos.sh')], env);
  const a2a = run('bash', [path.join(f.scriptDir, 'okx-a2a.sh')], env);

  assert.equal(cli.TMPDIR, path.join(f.root, '.codex/runtime/tmp'));
  assert.equal(a2a.TMPDIR, path.join(f.root, '.codex/runtime/tmp'));
  assert.equal(fs.statSync(cli.TMPDIR).mode & 0o777, 0o700);
  assert.equal(cli.ONCHAINOS_HOME, path.join(f.root, '.codex/runtime/onchainos'));
  assert.equal(a2a.ONCHAINOS_HOME, path.join(f.root, '.codex/runtime/onchainos'));
});

test('project runtime umask creates envelope files with mode 0600', t => {
  const f = fixture(t);
  const result = spawnSync('bash', ['-c', 'source "$1"; load_dev_runtime; node -e \'require("node:fs").writeFileSync(process.env.TMPDIR + "/envelope.json", "{}", {flag:"wx"})\'', '--', path.join(f.scriptDir, 'common.sh')], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(fs.statSync(path.join(f.root, '.codex/runtime/tmp/envelope.json')).mode & 0o777, 0o600);
});

test('Codex auto-profile process inherits project runtime without leaking it to caller shell', t => {
  const f = fixture(t);
  const gitInit = spawnSync('git', ['init', '--quiet', f.root], { encoding: 'utf8' });
  assert.equal(gitInit.status, 0, gitInit.stderr);
  const codexBin = path.join(f.root, 'fake-bin');
  fs.mkdirSync(codexBin);
  const capture = path.join(f.root, 'codex.json');
  fs.writeFileSync(path.join(codexBin, 'codex'), `#!/usr/bin/env node\nrequire('node:fs').writeFileSync(${JSON.stringify(capture)}, JSON.stringify({argv:process.argv.slice(2),TMPDIR:process.env.TMPDIR,umask:process.umask()})); require('node:fs').writeFileSync(process.env.TMPDIR + '/agent-envelope.json', '{}', {flag:'wx'});\n`, { mode: 0o700 });
  fs.writeFileSync(path.join(f.root, '.codex/profile-name'), 'onchainos-test\n');
  const env = { ...process.env, PATH: `${codexBin}${path.delimiter}${process.env.PATH}`, TMPDIR: path.join(f.root, 'caller-tmp') };
  fs.mkdirSync(env.TMPDIR);
  const script = 'source "$1"; codex --help; printf \'%s\' "$TMPDIR" > "$2"';
  const result = spawnSync('bash', ['-c', script, '--', path.join(source, 'codex-shell.sh'), path.join(f.root, 'parent-tmpdir')], { cwd: f.root, env, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  const invoked = JSON.parse(fs.readFileSync(capture, 'utf8'));
  assert.equal(invoked.TMPDIR, path.join(f.root, '.codex/runtime/tmp'));
  assert.equal(invoked.umask, 0o77);
  assert.equal(fs.statSync(path.join(invoked.TMPDIR, 'agent-envelope.json')).mode & 0o777, 0o600);
  assert.ok(invoked.argv.includes('onchainos-test'));
  assert.equal(fs.readFileSync(path.join(f.root, 'parent-tmpdir'), 'utf8'), env.TMPDIR);
});

test('explicit Codex workspace/profile keeps the selected invocation and environment', t => {
  const f = fixture(t);
  const gitInit = spawnSync('git', ['init', '--quiet', f.root], { encoding: 'utf8' });
  assert.equal(gitInit.status, 0, gitInit.stderr);
  const codexBin = path.join(f.root, 'fake-bin');
  fs.mkdirSync(codexBin);
  const capture = path.join(f.root, 'codex.json');
  fs.writeFileSync(path.join(codexBin, 'codex'), `#!/usr/bin/env node\nrequire('node:fs').writeFileSync(${JSON.stringify(capture)}, JSON.stringify({argv:process.argv.slice(2),TMPDIR:process.env.TMPDIR}));\n`, { mode: 0o700 });
  fs.writeFileSync(path.join(f.root, '.codex/profile-name'), 'onchainos-test\n');
  const selectedTmp = path.join(f.root, 'other-profile-tmp');
  fs.mkdirSync(selectedTmp);
  const env = { ...process.env, PATH: `${codexBin}${path.delimiter}${process.env.PATH}`, TMPDIR: selectedTmp };
  const result = spawnSync('bash', ['-c', 'source "$1"; codex --profile other-profile --help', '--', path.join(source, 'codex-shell.sh')], { cwd: f.root, env, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  const invoked = JSON.parse(fs.readFileSync(capture, 'utf8'));
  assert.deepEqual(invoked.argv, ['--profile', 'other-profile', '--help']);
  assert.equal(invoked.TMPDIR, selectedTmp);
});

test('explicit own profile loads project runtime and preserves separated/combined arguments', t => {
  const f = fixture(t);
  const gitInit = spawnSync('git', ['init', '--quiet', f.root], { encoding: 'utf8' });
  assert.equal(gitInit.status, 0, gitInit.stderr);
  const codexBin = path.join(f.root, 'fake-bin');
  fs.mkdirSync(codexBin);
  const capture = path.join(f.root, 'codex.json');
  fs.writeFileSync(path.join(codexBin, 'codex'), `#!/usr/bin/env node\nrequire('node:fs').writeFileSync(${JSON.stringify(capture)}, JSON.stringify({argv:process.argv.slice(2),TMPDIR:process.env.TMPDIR,umask:process.umask()})); require('node:fs').writeFileSync(process.env.TMPDIR + '/explicit-agent-envelope.json', '{}', {flag:'wx'});\n`, { mode: 0o700 });
  fs.writeFileSync(path.join(f.root, '.codex/profile-name'), 'onchainos-test\n');
  const env = { ...process.env, PATH: `${codexBin}${path.delimiter}${process.env.PATH}`, TMPDIR: path.join(f.root, 'caller-tmp') };
  fs.mkdirSync(env.TMPDIR);
  const result = spawnSync('bash', ['-c', 'source "$1"; codex -ponchainos-test --cd . --help', '--', path.join(source, 'codex-shell.sh')], { cwd: f.root, env, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  const invoked = JSON.parse(fs.readFileSync(capture, 'utf8'));
  assert.deepEqual(invoked.argv, ['-ponchainos-test', '--cd', '.', '--help']);
  assert.equal(invoked.TMPDIR, path.join(f.root, '.codex/runtime/tmp'));
  assert.equal(invoked.umask, 0o77);
  assert.equal(fs.statSync(path.join(invoked.TMPDIR, 'explicit-agent-envelope.json')).mode & 0o777, 0o600);
});

test('explicit own profile with another workspace leaves runtime context unchanged', t => {
  const f = fixture(t);
  const gitInit = spawnSync('git', ['init', '--quiet', f.root], { encoding: 'utf8' });
  assert.equal(gitInit.status, 0, gitInit.stderr);
  const codexBin = path.join(f.root, 'fake-bin');
  fs.mkdirSync(codexBin);
  const capture = path.join(f.root, 'codex.json');
  fs.writeFileSync(path.join(codexBin, 'codex'), `#!/usr/bin/env node\nrequire('node:fs').writeFileSync(${JSON.stringify(capture)}, JSON.stringify({argv:process.argv.slice(2),TMPDIR:process.env.TMPDIR,umask:process.umask()}));\n`, { mode: 0o700 });
  fs.writeFileSync(path.join(f.root, '.codex/profile-name'), 'onchainos-test\n');
  const otherWorkspace = path.join(f.root, 'other-workspace');
  fs.mkdirSync(otherWorkspace);
  const selectedTmp = path.join(f.root, 'other-workspace-tmp');
  fs.mkdirSync(selectedTmp);
  const env = { ...process.env, PATH: `${codexBin}${path.delimiter}${process.env.PATH}`, TMPDIR: selectedTmp };
  const result = spawnSync('bash', ['-c', 'source "$1"; codex --profile=onchainos-test -C' + otherWorkspace, '--', path.join(source, 'codex-shell.sh')], { cwd: f.root, env, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  const invoked = JSON.parse(fs.readFileSync(capture, 'utf8'));
  assert.deepEqual(invoked.argv, [`--profile=onchainos-test`, `-C${otherWorkspace}`]);
  assert.equal(invoked.TMPDIR, selectedTmp);
  assert.notEqual(invoked.umask, 0o77);
});

test('zsh Codex wrapper loads project runtime through Bash when zsh is available', t => {
  const zsh = spawnSync('zsh', ['--version'], { encoding: 'utf8' });
  if (zsh.error?.code === 'ENOENT') return t.skip('zsh is not installed');
  assert.equal(zsh.status, 0, zsh.stderr);
  const f = fixture(t);
  const gitInit = spawnSync('git', ['init', '--quiet', f.root], { encoding: 'utf8' });
  assert.equal(gitInit.status, 0, gitInit.stderr);
  const codexBin = path.join(f.root, 'fake-bin');
  fs.mkdirSync(codexBin);
  const capture = path.join(f.root, 'codex.json');
  fs.writeFileSync(path.join(codexBin, 'codex'), `#!/usr/bin/env node\nrequire('node:fs').writeFileSync(${JSON.stringify(capture)}, JSON.stringify({argv:process.argv.slice(2),TMPDIR:process.env.TMPDIR,umask:process.umask()}));\n`, { mode: 0o700 });
  fs.writeFileSync(path.join(f.root, '.codex/profile-name'), 'onchainos-test\n');
  const env = { ...process.env, PATH: `${codexBin}${path.delimiter}${process.env.PATH}`, TMPDIR: path.join(f.root, 'caller-tmp') };
  fs.mkdirSync(env.TMPDIR);
  const result = spawnSync('zsh', ['-c', 'source "$1"; codex --help', '--', path.join(source, 'codex-shell.sh')], { cwd: f.root, env, encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  const invoked = JSON.parse(fs.readFileSync(capture, 'utf8'));
  assert.equal(invoked.TMPDIR, path.join(f.root, '.codex/runtime/tmp'));
  assert.equal(invoked.umask, 0o77);
  assert.ok(invoked.argv.includes('onchainos-test'));
});
