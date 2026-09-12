import { call } from "./ipc";
import type { Settings, SettingsPatch } from "@/types/models";

export const getSettings = () => call<Settings>("get_settings");

export const updateSettings = (patch: SettingsPatch) =>
  call<Settings>("update_settings", { patch });
