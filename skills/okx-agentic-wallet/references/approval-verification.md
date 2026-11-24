# Approval Verification

## Module Description

Load when a command returns `confirming` with `scene=approval_code_sent`, or an `errorCode` that starts with `approval_`. The backend paused a token approval until the user enters a code from the bound email. The CLI keeps the original request.

## Flow

### Collect Parameters

1. Show the CLI `message` in the user's language, followed by `{policy_settings_line}` from [wallet.md](wallet.md) → Template: Security Policy.
2. Ask the user for the 6-digit code. Use only the code the user typed. Never guess, reuse, or repeat a code in the conversation.

### Execute

- The user gives the code → run `onchainos wallet approval-code verify --verification-code <code>`.
- The user asks for a new code → run `onchainos wallet approval-code resend`.
- The user cancels → run `onchainos wallet approval-code cancel` and say the approval was not executed.

### Handle Result

- **Success**: report the result as for the original command. A verified code is not a confirmed transaction: check the order with `wallet history` before you say it is on chain.
- **Paused step of a multi-step command** (for example, the approval step of a swap): only the paused request ran.
  - Tell the user which step completed.
  - Do not rerun the original command on your own.
  - If the user wants the rest, start it as a new request with its own preview and confirmation.
- **Error**: show the CLI `message`. Offer only the commands in `nextSteps`.

## Security

### Risk and Stop Conditions

- **NEVER** resend a code or rerun `verify` on your own. The backend limits sends and attempts, and an automatic retry hides the failure from the user.
- **NEVER** rebuild, edit, or hand-write the transaction or typed data. The verification is bound to the exact original request, and the CLI replays it once, unchanged.

## CLI Reference

- [wallet-cli-reference.md](wallet-cli-reference.md) → `wallet approval-code`
