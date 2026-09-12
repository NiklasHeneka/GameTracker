import { invoke } from "@tauri-apps/api/core";
import type { IpcError } from "@/types/models";

/**
 * Every backend error arrives as `{ kind, message }`. Anything else (a panic,
 * a missing command) comes through as a bare string, so normalise it here and
 * let callers branch on `kind` alone.
 */
export function toIpcError(e: unknown): IpcError {
  if (typeof e === "object" && e !== null && "kind" in e && "message" in e) {
    return e as IpcError;
  }
  return { kind: "unknown", message: String(e) };
}

export async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (e) {
    const err = toIpcError(e);
    console.error(`invoke(${command}) failed:`, err);
    throw err;
  }
}
