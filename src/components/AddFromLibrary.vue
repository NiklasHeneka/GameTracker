<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref } from "vue";

import AppIcon from "./AppIcon.vue";
import GameCover from "./GameCover.vue";
import { bucketOf } from "@/composables/buckets";
import { releaseYear } from "@/composables/useIgdbImage";
import { useLibraryStore } from "@/stores/library";
import { useQueueStore } from "@/stores/queue";

const emit = defineEmits<{ close: [] }>();

const library = useLibraryStore();
const queue = useQueueStore();

const term = ref("");
const error = ref<string | null>(null);
const busy = ref<number | null>(null);
const input = ref<HTMLInputElement | null>(null);

// Bound to the window rather than the input: clicking Add moves focus to a
// button, and the footer promises Esc closes the dialog regardless.
function onKey(e: KeyboardEvent) {
  if (e.key === "Escape") emit("close");
}

onMounted(() => {
  nextTick(() => input.value?.focus());
  window.addEventListener("keydown", onKey);
});
onUnmounted(() => window.removeEventListener("keydown", onKey));

/**
 * The library minus what is already queued. Unlike the Add-game modal this
 * searches locally and never touches IGDB: Play Next is drawn from games you
 * already track, so there is nothing to look up.
 */
const candidates = computed(() => {
  const q = term.value.trim().toLowerCase();
  return library.entries
    .filter((e) => !queue.queuedIds.has(e.id))
    .filter((e) => !q || e.game.name.toLowerCase().includes(q))
    .slice(0, 60);
});

async function add(entryId: number) {
  busy.value = entryId;
  error.value = null;
  try {
    await queue.add(entryId);
  } catch (e) {
    error.value = (e as { message?: string }).message ?? String(e);
  } finally {
    busy.value = null;
  }
}
</script>

<template>
  <div
    class="fixed inset-0 z-50 flex items-start justify-center bg-black/60 px-6 pt-[12vh] backdrop-blur-sm"
    @click.self="emit('close')"
  >
    <div
      class="flex max-h-[70vh] w-full max-w-xl flex-col overflow-hidden rounded-2xl border border-line-solid bg-surface shadow-2xl shadow-black/60"
      role="dialog"
      aria-label="Add to Play Next"
    >
      <div class="flex items-center gap-2.5 border-b border-line px-4">
        <AppIcon name="search" :size="17" class="shrink-0 text-ink-faint" />
        <input
          ref="input"
          v-model="term"
          type="text"
          placeholder="Search your library…"
          class="w-full bg-transparent py-3.5 text-[14px] outline-none placeholder:text-ink-faint"
        />
      </div>

      <div class="pane-scroll min-h-0 flex-1">
        <p v-if="error" class="px-4 py-4 text-[12.5px] text-danger">{{ error }}</p>

        <p v-if="!candidates.length" class="px-4 py-8 text-center text-[12.5px] text-ink-faint">
          <template v-if="term.trim()">Nothing in your library matches “{{ term }}”.</template>
          <template v-else-if="library.entries.length">
            Everything in your library is already on the list.
          </template>
          <template v-else>Add some games to your library first.</template>
        </p>

        <ul v-else class="divide-y divide-line">
          <li
            v-for="entry in candidates"
            :key="entry.id"
            class="flex items-center gap-3 px-4 py-2.5 transition-colors hover:bg-elevated/50"
          >
            <div class="h-14 w-[42px] shrink-0 overflow-hidden rounded-md bg-elevated">
              <GameCover
                :image-id="entry.game.coverImageId"
                :name="entry.game.name"
                size="cover_small"
              />
            </div>

            <div class="min-w-0 flex-1">
              <p class="truncate text-[13px]">{{ entry.game.name }}</p>
              <p class="mt-0.5 truncate text-[11.5px] text-ink-faint">
                {{ bucketOf(entry).label }}
                <template v-if="releaseYear(entry.game.firstRelease)">
                  · {{ releaseYear(entry.game.firstRelease) }}
                </template>
              </p>
            </div>

            <button
              type="button"
              :disabled="busy === entry.id"
              class="shrink-0 rounded-lg bg-accent px-2.5 py-1.5 text-[12px] font-medium text-white transition-opacity hover:opacity-90 disabled:opacity-50"
              @click="add(entry.id)"
            >
              Add
            </button>
          </li>
        </ul>
      </div>

      <p class="border-t border-line px-4 py-2 text-[11px] text-ink-faint">
        New games join the end of the list · Esc to close
      </p>
    </div>
  </div>
</template>
