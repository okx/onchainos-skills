# CLI Conventions

- Use only documented options; inspect `--help` before using unknown syntax.
- Use the command's JSON mode when structured fields are required.
- Render amount strings verbatim; do not convert them to floats.
- Read current state before retrying an uncertain write.
- Take every `jobId`, device ID, and address from current structured CLI output.
