# CLI Input/Output Rules

Rules for CLI inputs (flags) and outputs (stdout). Read this file before you
change CLI flags, input validation, or command output, and before you document
them in a Skill. It is deliberately short: it covers only the points below and
adds no other CLI requirements. More rules may be added later.

Where the code and this file disagree on current behavior, the code is the source
of truth: see `cli/src/validators.rs` for validation and `cli/src/output.rs` for
output. Existing commands may predate these rules; apply them to new and changed
code and do not rewrite unrelated commands.

## Inputs

1. **IO-IN-02: Flag names are easy to read.** Name each flag so that a reader can
   tell what the value means from the name alone.
2. **IO-IN-04: Use shared helpers for parsing and validation.** Handle each kind of
   value with one common helper, and call it from every command that takes that
   value. Do not write a second copy. If one command needs a stricter check, add a
   parameter to the shared helper instead. Examples of existing helpers:
   `validate_amount`, `readable_to_minimal_str`, and `validate_slippage` in
   `cli/src/validators.rs`, and `resolve_chain` with `ensure_supported_chain` in
   `cli/src/chains.rs`.

## Outputs

1. **Output is JSON.** A command writes its result to stdout as JSON so that an
   agent can parse it. This file does not fix the exact JSON fields; follow the
   existing code in `cli/src/output.rs` and stay consistent with neighboring
   commands.
