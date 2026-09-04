# A2A User Receipt

Controls subscription-message delivery to devices. It does not authorize execution.

## Enable this device

1. Read current state:

   ```bash
   onchainos agent subscribe-detail <jobId> --format json
   ```

2. Branch on `deviceList` and `thisDeviceReceives`:

   | State | Action |
   |---|---|
   | `deviceList: null` | Default-all; report enabled and do not write. |
   | Explicit list with `thisDeviceReceives: true` | Report enabled and do not write. |
   | Explicit list with `thisDeviceReceives: false` | Union the fresh list with the current device, then write. |
   | `deviceList: null` with `thisDeviceReceives: false` | Stop; state is inconsistent. |

3. Write only when required:

   ```bash
   onchainos agent subscribe-device-update \
     --job-id <jobId> --device-list <fresh-union>
   ```

4. Reread `subscribe-detail --format json`; report enabled only when
   `thisDeviceReceives` is true.

`null` means default-all, `[]` means none, and a non-empty array is an
allowlist. Build writes only from the fresh complete list. Do not start watch,
read history, sign, pay, or change execution policy.
