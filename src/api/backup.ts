import { call } from "./ipc";
import type { ImportSummary } from "@/types/models";

/** Returns how many entries were written. */
export const exportBackup = (path: string) => call<number>("export_backup", { path });

export const importBackup = (path: string, applySettings: boolean) =>
  call<ImportSummary>("import_backup", { path, applySettings });
