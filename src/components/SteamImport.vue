<script setup lang="ts">
import { computed, ref } from "vue";

import { importSteamLibrary, previewSteamImport } from "@/api/import";
import { useLibraryStore } from "@/stores/library";
import { useSettingsStore } from "@/stores/settings";
import type { ImportReport, SteamPreview } from "@/types/models";

const library = useLibraryStore();
const settings = useSettingsStore();

const input = ref(settings.settings?.steamId ?? "");
const preview = ref<SteamPreview | null>(null);
const report = ref<ImportReport | null>(null);
const busy = ref(false);
const error = ref<string | null>(null);

const includePlayed = ref(true);
const includeUnplayed = ref(false);

const willImport = computed(() => {
  if (!preview.value) return 0;
  return (
    (includePlayed.value ? preview.value.played : 0) +
    (includeUnplayed.value ? preview.value.unplayed : 0)
  );
});

function fail(e: unknown) {
  error.value = (e as { message?: string }).message ?? String(e);
}

async function look() {
  busy.value = true;
  error.value = null;
  report.value = null;
  try {
    preview.value = await previewSteamImport(input.value);
    input.value = preview.value.steamId;
  } catch (e) {
    preview.value = null;
    fail(e);
  } finally {
    busy.value = false;
  }
}

async function run() {
  busy.value = true;
  error.value = null;
  try {
    report.value = await importSteamLibrary(input.value, {
      includePlayed: includePlayed.value,
      includeUnplayed: includeUnplayed.value,
      minMinutes: 0,
    });
    await library.load();
  } catch (e) {
    fail(e);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <section class="mt-9">
    <h2 class="text-[14px] font-semibold">Steam library</h2>
    <p class="mt-1.5 text-[12.5px] leading-relaxed text-ink-faint">
      Import what you own, with playtime. Your Steam profile and game details must be set to
      Public. Importing again later is safe — it only fills gaps and never re-categorises a
      game you have already sorted.
    </p>

    <div class="mt-4 flex gap-2">
      <input
        v-model="input"
        type="text"
        placeholder="SteamID64 or your profile name"
        class="min-w-0 flex-1 rounded-lg border border-line bg-surface px-2.5 py-2 text-[12.5px] outline-none focus:border-line-solid"
        @keydown.enter="look"
      />
      <button
        type="button"
        :disabled="busy || !input.trim()"
        class="shrink-0 rounded-lg border border-line bg-elevated px-3 py-2 text-[12.5px] text-ink-dim transition-colors hover:text-ink disabled:opacity-50"
        @click="look"
      >
        {{ busy && !preview ? "Checking…" : "Check" }}
      </button>
    </div>

    <p v-if="error" class="mt-2 text-[12px] text-danger">{{ error }}</p>

    <template v-if="preview">
      <div class="mt-3 rounded-xl border border-line bg-surface px-4 py-3">
        <p class="text-[12.5px]">
          <strong>{{ preview.total }}</strong> games on this account ·
          {{ preview.played }} played · {{ preview.unplayed }} never launched
          <template v-if="preview.alreadyTracked">
            · {{ preview.alreadyTracked }} already here
          </template>
        </p>

        <div class="mt-3 space-y-2">
          <label class="flex items-center gap-2 text-[12.5px]">
            <input v-model="includePlayed" type="checkbox" class="h-3.5 w-3.5 accent-accent" />
            Games you have played ({{ preview.played }})
          </label>
          <label class="flex items-center gap-2 text-[12.5px]">
            <input v-model="includeUnplayed" type="checkbox" class="h-3.5 w-3.5 accent-accent" />
            Never launched ({{ preview.unplayed }}) — your real backlog
          </label>
        </div>

        <button
          type="button"
          :disabled="busy || willImport === 0"
          class="mt-3 rounded-lg bg-accent px-3 py-1.5 text-[12.5px] font-medium text-white transition-opacity hover:opacity-90 disabled:opacity-40"
          @click="run"
        >
          {{ busy ? "Importing…" : `Import ${willImport} games` }}
        </button>
      </div>
    </template>

    <div
      v-if="report"
      class="mt-3 rounded-xl border border-line bg-surface px-4 py-3 text-[12.5px] leading-relaxed"
    >
      <p>
        Added <strong class="text-deal">{{ report.added }}</strong>
        <template v-if="report.updated">
          · filled in playtime for {{ report.updated }}
        </template>
        <template v-if="report.considered - report.added - report.updated > 0">
          · {{ report.considered - report.added - report.updated }} left alone
        </template>
      </p>
      <p v-if="report.message" class="mt-1 text-[11.5px] text-ink-faint">{{ report.message }}</p>
    </div>
  </section>
</template>
