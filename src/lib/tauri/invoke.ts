import { invoke } from "@tauri-apps/api/core";

export type InvokeCommand = <T>(
  command: string,
  args?: Record<string, unknown>,
) => Promise<T>;

export const tauriInvoke: InvokeCommand = <T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> =>
  args === undefined ? invoke<T>(command) : invoke<T>(command, args);
