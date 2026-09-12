import { call } from "./ipc";
import type { ImportOptions, ImportReport, Stats, SteamPreview } from "@/types/models";

export const previewSteamImport = (steamId: string) =>
  call<SteamPreview>("preview_steam_import", { steamId });

export const importSteamLibrary = (steamId: string, options: ImportOptions) =>
  call<ImportReport>("import_steam_library", { steamId, options });

export const getStats = () => call<Stats>("get_stats");
