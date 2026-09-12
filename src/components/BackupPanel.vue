<script setup lang="ts">
import { ref } from "vue";
import { open, save } from "@tauri-apps/plugin-dialog";

import { exportBackup, importBackup } from "@/api/backup";
import { shortDate } from "@/composables/useFormat";
import { useLibraryStore } from "@/stores/library";
import { useSettingsStore } from "@/stores/settings";

const library = useLibraryStore();
const settings = useSettingsStore();

const busy = ref(false);
const status = ref<string | null>(null);
const error = ref<string | null>(null);

const FILTERS = [{ name: "GameTracker backup", extensions: ["json"] }];

function today() {
  return new Date().toISOString().slice(0, 10);
}

async function doExport() {
  error.value = null;
  status.value = null;
  const path = await save({
    defaultPath: `gametracker-${today()}.json`,
    filters: FILTERS,
  });
  if (!path) return;

  busy.value = true;
  try {
    const count = await exportBackup(path);
    status.value = `Saved ${count} ${count === 1 ? "game" : "games"}.`;
  } catch (e) {
    error.value = (e as { message?: string }).message ?? String(e);
  } finally {
    busy.value = false;
  }
}

async function doImport() {
  error.value = null;
  status.value = null;
  const path = await open({ multiple: false, directory: false, filters: FILTERS });
  if (typeof path !== "string") return;

  busy.value = true;
  try {
    const summary = await importBackup(path, false);
    status.value =
      `Added ${summary.added} of ${summary.entriesInFile}` +
      (summary.skipped ? `, left ${summary.skipped} already-tracked alone` : "") +
      ` (backup from ${shortDate(summary.exportedAt)}).`;
    await library.load();
    await settings.load();
  } catch (e) {
    error.value = (e as { message?: string }).message ?? String(e);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <section class="mt-9">
    <h2 class="text-[14px] font-semibold">Backup</h2>
    <p class="mt-1.5 text-[12.5px] leading-relaxed text-ink-faint">
      Your library as a plain JSON file. Importing only adds — a game already tracked is left
      exactly as it is, so restoring into a live library can never overwrite your edits.
      API keys are not included; those stay in your <code class="text-ink-dim">.env</code>.
    </p>

    <div class="mt-4 flex flex-wrap gap-2">
      <button
        type="button"
        :disabled="busy"
        class="rounded-lg border border-line bg-elevated px-3 py-1.5 text-[12.5px] text-ink-dim transition-colors hover:text-ink disabled:opacity-50"
        @click="doExport"
      >
        Export…
      </button>
      <button
        type="button"
        :disabled="busy"
        class="rounded-lg border border-line bg-elevated px-3 py-1.5 text-[12.5px] text-ink-dim transition-colors hover:text-ink disabled:opacity-50"
        @click="doImport"
      >
        Import…
      </button>
    </div>

    <p v-if="status" class="mt-2 text-[12px] text-ink-dim">{{ status }}</p>
    <p v-if="error" class="mt-2 text-[12px] text-danger">{{ error }}</p>
  </section>
</template>
