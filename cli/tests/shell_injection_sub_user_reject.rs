//! Real Shell-interpreter regression for the A2A stored-title RCE, scoped to the
//! ASP `sub_user_reject` decision card.
//!
//! ─── Why this test exists ────────────────────────────────────────────────────
//! The in-crate unit tests (`pending_v2::shared_encoding_shell_safety_tests`,
//! `asp::flow::sub_user_reject_*`) prove two halves separately: the real
//! `sub_user_reject` renderer emits the reserved `{{__OKX_TASK_TITLE__}}` /
//! `{{__OKX_TASK_LABEL_TITLE__}}` placeholders plus a shell-safe Base64
//! `--template-vars-b64` payload (raw title provably OFF the emitted line), and
//! the decode+render round-trip reconstructs the title in-process. What they do
//! NOT do is execute the vulnerable boundary — a real shell parsing the emitted
//! command against the real `onchainos` binary. This integration test closes that
//! gap end-to-end:
//!
//!   real SubUserReject-shaped command (placeholders + Base64)
//!     → zsh -f / bash parses it
//!       → the real test-built `onchainos` request-prompt runs
//!         → in-process template substitution (after clap parse, before any push)
//!           → fake `okx-a2a` records the final decision-card argv
//!
//! Hermetic and offline (proc-hermetic-cli-test-onchainos-home,
//! proc-cli-tests-no-hardcoded-tmp): isolated HOME / ONCHAINOS_HOME / PATH under
//! `target/test_tmp`, a fake no-op `okx-a2a` first in PATH, and
//! `ONCHAINOS_SKIP_A2A_PREFLIGHT=1`. No real backend / A2A daemon / MCP / wallet /
//! signing / transaction / network call.
//!
//! NOTE: this crate is a bin-only package (no `lib`), so an integration test can
//! only invoke the binary — it cannot call `generate_next_action` directly. The
//! command it runs is therefore assembled in the exact shape the `sub_user_reject`
//! renderer emits (verified against that renderer by the in-crate asp/flow tests):
//! `--user-content` carrying `{{__OKX_TASK_TITLE__}}`, `--list-label` carrying
//! `{{__OKX_TASK_LABEL_TITLE__}}`, `--source-event sub_user_reject`, and
//! `--template-vars-b64 "<Base64(JSON)>"`.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::fresh_home;

/// A hostile title payload. In zsh, `${(e)}` forces
/// eval and `${(#):-96}` yields the character with code 96 (a backtick), so this
/// reconstructs and runs `id>&2` IF any byte of it ever reaches a zsh command
/// line. The whole point of the fix is that it never does.
const HOSTILE_ZSH_TITLE: &str = "x${(e):-${(#):-96}id>&2${(#):-96}}";

/// Standard-alphabet Base64 encoder (with padding). Self-contained so the test
/// needs no new dev-dependency (`base64` is a normal dep, not a dev-dep, and
/// adding it to `[dev-dependencies]` would desync Cargo.toml/Cargo.lock and trip
/// the onchainos_check gate). Produces the same wire bytes as the CLI's
/// `encode_title_vars` for the same JSON.
fn base64_standard(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((n >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Build the `--template-vars-b64` payload exactly as the renderer's
/// `encode_title_vars(copy, label)` does: Base64(JSON object) with the two
/// whitelisted keys. Uses `serde_json` (a dev-dep) for correct JSON escaping.
fn title_vars_b64(copy_title: &str, label_title: &str) -> String {
    let obj = serde_json::json!({
        "__OKX_TASK_TITLE__": copy_title,
        "__OKX_TASK_LABEL_TITLE__": label_title,
    });
    base64_standard(&serde_json::to_vec(&obj).expect("json serializes"))
}

/// Absolute path to the test-built `onchainos` binary (cargo sets this env var
/// for integration tests and builds the bin first).
fn onchainos_bin() -> &'static str {
    env!("CARGO_BIN_EXE_onchainos")
}

/// Write a fake, no-op `okx-a2a` first in PATH. It records every argv element
/// (NUL-delimited, so titles containing newlines/quotes/semicolons round-trip
/// intact) to `argv_file`, appends one line to `call_file` per invocation, and
/// exits 0. It performs NO side effect of its own.
fn write_fake_okx_a2a(bin_dir: &Path) {
    std::fs::create_dir_all(bin_dir).expect("create fake bin dir");
    let script = "#!/bin/sh\n\
                  # Fake okx-a2a: record argv, no side effects, exit 0.\n\
                  for a in \"$@\"; do printf '%s\\0' \"$a\" >> \"$OKX_A2A_ARGV_FILE\"; done\n\
                  printf 'CALLED\\n' >> \"$OKX_A2A_CALL_FILE\"\n\
                  exit 0\n";
    let path = bin_dir.join("okx-a2a");
    std::fs::write(&path, script).expect("write fake okx-a2a");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake okx-a2a");
    }
}

/// One invocation of the emitted decision-card command through a real shell.
struct ShellRun {
    status_code: Option<i32>,
    stdout: String,
    stderr: String,
    /// Recorded argv elements of the fake `okx-a2a` (empty if it was never called).
    okx_a2a_argv: Vec<String>,
    /// Number of times the fake `okx-a2a` was invoked.
    okx_a2a_calls: usize,
    /// The exact shell program source that was executed.
    program: String,
}

/// Assemble the `sub_user_reject`-shaped `request-prompt` command, write it as a
/// shell program, and run it under `shell_bin` with the given `shell_flags`
/// (e.g. `zsh -f`). Hermetic: fresh HOME/ONCHAINOS_HOME/PATH, fake okx-a2a first.
#[allow(clippy::too_many_arguments)]
fn run_emitted_command(
    home: &Path,
    shell_bin: &str,
    shell_flags: &[&str],
    user_content: &str,
    list_label: &str,
    template_vars_b64: Option<&str>,
    // `true` forces CLI mode (CLAUDECODE=1) so the push goes straight to okx-a2a
    // once with no queue file. `false` runs in queue mode, which persists the
    // PendingEntry (with the in-process-substituted `list_label`) to
    // `$ONCHAINOS_HOME/task/pending-decisions-new.json` — the only place the
    // resolved list-label is observable end-to-end (CLI mode discards it).
    cli_mode: bool,
    tag: &str,
) -> ShellRun {
    let bin_dir = home.join("fakebin");
    write_fake_okx_a2a(&bin_dir);
    let argv_file = home.join(format!("okx_a2a_argv_{tag}.bin"));
    let call_file = home.join(format!("okx_a2a_calls_{tag}.log"));
    let _ = std::fs::remove_file(&argv_file);
    let _ = std::fs::remove_file(&call_file);

    // Escape for a double-quoted shell word: only `\` and `"` are special inside
    // "..." for both zsh and bash (this mirrors the renderer's own escaping).
    let dq = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let flag_line = match template_vars_b64 {
        Some(b) => format!(" \\\n  --template-vars-b64 \"{}\"", dq(b)),
        None => String::new(),
    };
    let program = format!(
        "\"{bin}\" agent pending-decisions-v2 request-prompt \\\n\
         \x20\x20--job-id 0xsub01 --role asp --agent-id 864 \\\n\
         \x20\x20--user-content \"{content}\" \\\n\
         \x20\x20--list-label \"{label}\" \\\n\
         \x20\x20--source-event sub_user_reject{flag_line}\n",
        bin = onchainos_bin(),
        content = dq(user_content),
        label = dq(list_label),
        flag_line = flag_line,
    );

    let prog_file = home.join(format!("emitted_{tag}.sh"));
    std::fs::write(&prog_file, &program).expect("write shell program");

    let output = Command::new(shell_bin)
        .args(shell_flags)
        .arg(&prog_file)
        // Isolate the environment completely.
        .env_clear()
        .env("HOME", home)
        .env("ONCHAINOS_HOME", home)
        .env("PATH", format!("{}:/usr/bin:/bin", bin_dir.display()))
        .env("ONCHAINOS_SKIP_A2A_PREFLIGHT", "1")
        .envs(
            // CLI mode → straight-to-okx-a2a once; queue mode → persist the entry.
            cli_mode.then_some(("CLAUDECODE", "1")),
        )
        .env("OKX_A2A_ARGV_FILE", &argv_file)
        .env("OKX_A2A_CALL_FILE", &call_file)
        .output()
        .unwrap_or_else(|e| panic!("failed to run {shell_bin}: {e}"));

    let okx_a2a_argv = std::fs::read(&argv_file)
        .map(|bytes| {
            bytes
                .split(|&b| b == 0)
                .filter(|s| !s.is_empty())
                .map(|s| String::from_utf8_lossy(s).into_owned())
                .collect()
        })
        .unwrap_or_default();
    let okx_a2a_calls = std::fs::read_to_string(&call_file)
        .map(|s| s.lines().filter(|l| !l.is_empty()).count())
        .unwrap_or(0);

    ShellRun {
        status_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        okx_a2a_argv,
        okx_a2a_calls,
        program,
    }
}

/// Shells to exercise. zsh MUST be run with `-f` (no rc files). Bash is run when
/// available; both are present in CI/dev, but the loop skips a missing shell
/// rather than failing so the suite stays portable.
fn available_shells() -> Vec<(&'static str, Vec<&'static str>)> {
    let mut shells: Vec<(&'static str, Vec<&'static str>)> = Vec::new();
    for (bin, flags) in [("zsh", vec!["-f"]), ("bash", vec![])] {
        if Command::new(bin).arg("--version").output().is_ok() {
            shells.push((bin, flags));
        }
    }
    assert!(
        shells.iter().any(|(b, _)| *b == "zsh"),
        "zsh is required for this regression because the hostile title uses zsh expansion"
    );
    shells
}

/// The renderer puts these two placeholders in the two visible fields; the raw
/// titles travel only inside the Base64 payload.
const COPY_TEMPLATE: &str =
    "[Action Needed: User Rejection] The user has rejected {{__OKX_TASK_TITLE__}}'s current period. A. refund  B. dispute";
const LABEL_TEMPLATE: &str = "[Decision 0xsub01] {{__OKX_TASK_LABEL_TITLE__}} — refund or dispute";

// ════════════════════════════════════════════════════════════════════════════
//  Core regression — the hostile payload stays literal end-to-end.
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn hostile_payload_never_reaches_the_shell_and_argv_is_byte_exact() {
    let (guard, home) = fresh_home("shell_injection_sub_user_reject");
    let _ = &guard;

    // A corpus of hostile titles carrying zsh expansion or other shell
    // metacharacters. The `$(touch ...)` probe writes a sentinel IF command
    // substitution ever fires; it must not.
    let sentinel = home.join("ATTACK_SENTINEL");
    let touch_probe = format!("$(touch {})", sentinel.display());
    // Redirection probe: build the target from the isolated `home` so that IF the
    // security control ever regresses, the redirection can only create a file
    // INSIDE the test's temp dir — never a fixed external `/tmp` path. This keeps
    // the "all sentinels under test temp" claim honest (proc-cli-tests-no-hardcoded-tmp)
    // and lets us assert the redirection sentinel is absent.
    let redir_sentinel = home.join("REDIR_ATTACK_SENTINEL");
    let redir_probe = format!("oops > {}", redir_sentinel.display());
    let corpus: Vec<(&str, String)> = vec![
        ("zsh_eval", HOSTILE_ZSH_TITLE.to_string()),
        ("touch_probe", touch_probe.clone()),
        ("backtick", "`id`".to_string()),
        ("dollar_paren", "$(id)".to_string()),
        ("quote_semi_hash", "\"; id; #".to_string()),
        ("redir", redir_probe.clone()),
        // CJK + emoji as \u escapes (no raw CJK bytes in source).
        ("unicode", "\u{4e2d}\u{6587}\u{1f680}".to_string()),
        ("apostrophe", "Oli's task".to_string()),
    ];

    for (shell_bin, shell_flags) in available_shells() {
        for (name, title) in &corpus {
            let b64 = title_vars_b64(title, title);
            let tag = format!("{shell_bin}_{name}");
            let run = run_emitted_command(
                &home,
                shell_bin,
                &shell_flags,
                COPY_TEMPLATE,
                LABEL_TEMPLATE,
                Some(&b64),
                true, // CLI mode: push once to okx-a2a, assert user-content byte-exact
                &tag,
            );

            // (1) The raw title is absent from the emitted shell program source —
            //     only its Base64 form and the reserved placeholders appear.
            assert!(
                !run.program.contains(title.as_str()),
                "[{tag}] raw title leaked into the emitted shell program:\n{}",
                run.program
            );
            assert!(
                run.program.contains(&b64) && run.program.contains("{{__OKX_TASK_TITLE__}}"),
                "[{tag}] program must carry Base64 payload + placeholder"
            );

            // (2) The command parsed & ran cleanly (valid command exits 0).
            assert_eq!(
                run.status_code,
                Some(0),
                "[{tag}] expected exit 0; stdout={} stderr={}",
                run.stdout,
                run.stderr
            );

            // (3) The fake okx-a2a was invoked EXACTLY once.
            assert_eq!(
                run.okx_a2a_calls, 1,
                "[{tag}] fake okx-a2a must be called exactly once"
            );

            // (4) The final decision-card argv carries the original title
            //     byte-for-byte (substituted in-process, never shell-parsed).
            let uc = argv_value(&run.okx_a2a_argv, "--user-content")
                .unwrap_or_else(|| panic!("[{tag}] okx-a2a argv missing --user-content"));
            assert_eq!(
                uc,
                COPY_TEMPLATE.replace("{{__OKX_TASK_TITLE__}}", title),
                "[{tag}] user-content must equal the copy with the exact title substituted"
            );
            assert!(
                uc.contains(title.as_str()),
                "[{tag}] the original title must survive byte-for-byte in argv"
            );

            // (4b) The emitted command carries each vulnerable / relevant flag
            //      EXACTLY once, so neither the decision-copy field, the
            //      originally-vulnerable label field, nor the source event is
            //      duplicated or dropped in the shell program the LLM runs.
            for flag in ["--user-content", "--list-label", "--source-event"] {
                assert_eq!(
                    run.program.matches(flag).count(),
                    1,
                    "[{tag}] emitted command must carry {flag} exactly once"
                );
            }

            // (4c) Lock the ORIGINALLY vulnerable sink — the title embedded in
            //      `--list-label "[Decision ...] <title> - refund or dispute"`. In
            //      CLI mode the resolved list-label is NOT forwarded to okx-a2a
            //      (only --user-content / --llm-content are), so re-run the exact
            //      same emitted command in queue mode and read the persisted
            //      PendingEntry: its `list_label` is the in-process-substituted
            //      value, proving the literal title survives byte-for-byte at the
            //      real boundary (never shell-parsed).
            let qtag = format!("{tag}_queue");
            let qrun = run_emitted_command(
                &home,
                shell_bin,
                &shell_flags,
                COPY_TEMPLATE,
                LABEL_TEMPLATE,
                Some(&b64),
                false, // queue mode → persist the entry with the resolved list_label
                &qtag,
            );
            assert_eq!(
                qrun.status_code,
                Some(0),
                "[{qtag}] queue-mode command must exit 0; stdout={} stderr={}",
                qrun.stdout,
                qrun.stderr
            );
            let persisted_label = persisted_list_label(&home)
                .unwrap_or_else(|| panic!("[{qtag}] no persisted list_label found in queue"));
            assert_eq!(
                persisted_label,
                LABEL_TEMPLATE.replace("{{__OKX_TASK_LABEL_TITLE__}}", title),
                "[{qtag}] persisted list-label must equal the label with the exact title"
            );
            assert!(
                persisted_label.contains(title.as_str()),
                "[{qtag}] original title must survive byte-for-byte in the persisted list-label"
            );

            // (5) No injection side effect fired: no sentinel file (command-sub or
            //     redirection), and `id` never executed (its output has `uid=`).
            assert!(
                !sentinel.exists(),
                "[{tag}] injection sentinel was created — command substitution fired!"
            );
            assert!(
                !redir_sentinel.exists(),
                "[{tag}] redirection sentinel was created — shell redirection fired!"
            );
            assert!(
                !run.stdout.contains("uid=") && !run.stderr.contains("uid="),
                "[{tag}] `id` output leaked — the title was executed by the shell"
            );
        }
    }
    assert!(
        !sentinel.exists(),
        "attack sentinel must never be created across the whole corpus"
    );
    assert!(
        !redir_sentinel.exists(),
        "redirection sentinel must never be created across the whole corpus"
    );
}

// ════════════════════════════════════════════════════════════════════════════
//  Fail-closed at the real boundary — a surviving placeholder with no flag
//  aborts before any push when the template-variable flag is missing.
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn missing_flag_with_placeholder_fails_closed_exit1_no_push_no_leak() {
    let (guard, home) = fresh_home("shell_injection_sub_user_reject_failclosed");
    let _ = &guard;
    let title = HOSTILE_ZSH_TITLE; // never supplied, so it can never leak anywhere

    for (shell_bin, shell_flags) in available_shells() {
        let tag = format!("{shell_bin}_noflag");
        let run = run_emitted_command(
            &home,
            shell_bin,
            &shell_flags,
            COPY_TEMPLATE,
            LABEL_TEMPLATE,
            None, // <-- missing --template-vars-b64 while placeholders remain
            true, // CLI mode: assert okx-a2a called ZERO times on fail-closed
            &tag,
        );

        // Exit 1 with the stable coded error, and NO card pushed.
        assert_eq!(
            run.status_code,
            Some(1),
            "[{tag}] must exit 1 (fail closed)"
        );
        assert!(
            run.stdout.contains("TEMPLATE_VALUE_MISSING"),
            "[{tag}] expected TEMPLATE_VALUE_MISSING; stdout={}",
            run.stdout
        );
        assert_eq!(
            run.okx_a2a_calls, 0,
            "[{tag}] fake okx-a2a must be called ZERO times on the fail-closed path"
        );
        // No error/audit output contains raw or decoded title data.
        assert!(
            !run.stdout.contains(title) && !run.stderr.contains(title),
            "[{tag}] fail-closed output must not contain title data"
        );
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Legacy path — no reserved placeholder + no flag behaves exactly as before
//  (exact legacy behavior and output).
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn no_placeholder_no_flag_is_exact_legacy_passthrough() {
    let (guard, home) = fresh_home("shell_injection_sub_user_reject_legacy");
    let _ = &guard;
    const PLAIN_CONTENT: &str = "[Decision 0xsub01] plain decision copy — A. refund  B. dispute";
    const PLAIN_LABEL: &str = "[Decision 0xsub01] plain label — refund or dispute";

    for (shell_bin, shell_flags) in available_shells() {
        let tag = format!("{shell_bin}_legacy");
        let run = run_emitted_command(
            &home,
            shell_bin,
            &shell_flags,
            PLAIN_CONTENT,
            PLAIN_LABEL,
            None,
            true, // CLI mode: legacy path pushes exactly one card
            &tag,
        );
        assert_eq!(run.status_code, Some(0), "[{tag}] legacy path must exit 0");
        assert_eq!(
            run.okx_a2a_calls, 1,
            "[{tag}] legacy path pushes exactly one card"
        );
        let uc = argv_value(&run.okx_a2a_argv, "--user-content")
            .unwrap_or_else(|| panic!("[{tag}] okx-a2a argv missing --user-content"));
        assert_eq!(
            uc, PLAIN_CONTENT,
            "[{tag}] legacy user-content must be passed through verbatim"
        );
    }
}

/// Return the value that follows `flag` in a recorded argv vector.
fn argv_value(argv: &[String], flag: &str) -> Option<String> {
    argv.iter()
        .position(|a| a == flag)
        .and_then(|i| argv.get(i + 1))
        .cloned()
}

/// Read the resolved `list_label` of the most-recent persisted PendingEntry from
/// the isolated `$ONCHAINOS_HOME/task/pending-decisions-new.json` queue file.
/// This is where a queue-mode `request-prompt` writes the in-process-substituted
/// list-label (the originally vulnerable sink). Parsed structurally so the assert
/// is byte-exact rather than a substring match.
fn persisted_list_label(home: &Path) -> Option<String> {
    let queue_file = home.join("task").join("pending-decisions-new.json");
    let bytes = std::fs::read(&queue_file).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value
        .get("entries")?
        .as_array()?
        .last()?
        .get("list_label")?
        .as_str()
        .map(str::to_owned)
}

// Silence unused-path warnings on non-unix (the suite is unix-only in practice).
#[allow(dead_code)]
fn _unused_pathbuf() -> PathBuf {
    PathBuf::new()
}
