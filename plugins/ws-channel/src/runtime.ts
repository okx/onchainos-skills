import type { PluginRuntime } from "openclaw/plugin-sdk";

let runtime: PluginRuntime | null = null;

export function setRuntime(r: PluginRuntime): void {
  runtime = r;
}

export function getRuntime(): PluginRuntime {
  if (!runtime) throw new Error("[ws-channel] runtime not initialized");
  return runtime;
}
