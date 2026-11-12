# Service CLI Reference

## Commands

### `onchainos service`

#### Syntax

```bash
onchainos service call --service newsliquid --operation status
onchainos service call --service newsliquid --operation news-search --params '{"coins":["BTC"],"limit":3}'
```

#### Parameters

| Flag | Applies to | Meaning |
| --- | --- | --- |
| `--service` | call | NewsLiquid/6551 service name; no implicit default |
| `--operation` | call | NewsLiquid operation name documented in its references |
| `--id` | call | Original resource ID when the operation requires one |
| `--params` | call | JSON object; default `{}`; NewsLiquid defines its fields |
| `--confirmed` | call | Records consent already obtained for operations requiring confirmation |

#### Return Fields

`call` returns `{ok:true,data:{service,result}}`; provider fields remain inside `result`. Failures use the existing error/errorCode/data contract.

#### Examples

```bash
onchainos service call --service newsliquid --operation login
onchainos service call --service newsliquid --operation pay --id "$NEWSLIQUID_ORDER_ID" --params "$NEWSLIQUID_PAY_PARAMS" --confirmed
```

#### Error Handling

Unsupported service names/operations are rejected before execution. `service_operation_requires_confirmation` requires the owning Flow's user confirmation, not automatic retry.

## Command Constraints

The CLI calls NewsLiquid directly. Its implementation owns endpoint/authentication and operation policy. Wallet and payment signing remain in their existing modules. This interface creates no hosted service, scheduler or notification platform.
