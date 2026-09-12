import { defineStore } from "pinia";
import { ref } from "vue";

import { getSettings, updateSettings } from "@/api/settings";
import { configStatus, reloadConfig, revealEnvFile } from "@/api/config";
import type { ConfigStatus, Settings, SettingsPatch } from "@/types/models";

export const useSettingsStore = defineStore("settings", () => {
  const settings = ref<Settings | null>(null);
  const config = ref<ConfigStatus | null>(null);
  const loading = ref(false);
  const error = ref<string | null>(null);

  async function load() {
    loading.value = true;
    error.value = null;
    try {
      const [s, c] = await Promise.all([getSettings(), configStatus()]);
      settings.value = s;
      config.value = c;
    } catch (e) {
      error.value = (e as { message?: string }).message ?? String(e);
    } finally {
      loading.value = false;
    }
  }

  async function patch(p: SettingsPatch) {
    // The backend returns the merged result, so it stays the source of truth
    // even when it clamps or normalises a value.
    settings.value = await updateSettings(p);
  }

  async function reload() {
    config.value = await reloadConfig();
  }

  const reveal = () => revealEnvFile();

  return { settings, config, loading, error, load, patch, reload, reveal };
});
