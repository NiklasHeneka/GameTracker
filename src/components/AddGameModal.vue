<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from "vue";

import AppIcon from "./AppIcon.vue";
import GameCover from "./GameCover.vue";
import { searchGames } from "@/api/library";
import { useLibraryStore } from "@/stores/library";
import type { SearchResult } from "@/types/models";

const emit = defineEmits<{ close: [] }>();

const library = useLibraryStore();
const term = ref("");
const results = ref<SearchResult[]>([]);
const searching = ref(false);
const error = ref<string | null>(null);
const adding = ref<number | null>(null);
const input = ref<HTMLInputElement | null>(null);

onMounted(() => nextTick(() => input.value?.focus()));

let timer: ReturnType<typeof setTimeout> | undefined;
// Each keystroke is an IGDB request against a 4 req/s budget, so debounce
// rather than firing per character.
let latest = 0;

watch(term, (value) => {
  clearTimeout(timer);
  error.value = null;

  if (value.trim().length < 2) {
    results.value = [];
    searching.value = false;
    return;
  }

  searching.value = true;
  timer = setTimeout(async () => {
    const ticket = ++latest;
    try {
      const found = await searchGames(value);
      // A slower earlier request must not overwrite newer results.
      if (ticket !== latest) return;
      results.value = found;
    } catch (e) {
      if (ticket !== latest) return;
      error.value = (e as { message?: string }).message ?? String(e);
      results.value = [];
    } finally {
      if (ticket === latest) searching.value = false;
    }
  }, 300);
});

async function add(result: SearchResult, owned: boolean) {
  adding.value = result.igdbId;
  try {
    await library.add(result.igdbId, owned);
    result.inLibrary = true;
  } catch (e) {
    error.value = (e as { message?: string }).message ?? String(e);
  } finally {
    adding.value = null;
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
      aria-label="Add a game"
    >
      <div class="flex items-center gap-2.5 border-b border-line px-4">
        <AppIcon name="search" :size="17" class="shrink-0 text-ink-faint" />
        <input
          ref="input"
          v-model="term"
          type="text"
          placeholder="Search games…"
          class="w-full bg-transparent py-3.5 text-[14px] outline-none placeholder:text-ink-faint"
          @keydown.esc="emit('close')"
        />
        <span v-if="searching" class="shrink-0 text-[11.5px] text-ink-faint">searching…</span>
      </div>

      <div class="pane-scroll min-h-0 flex-1">
        <p v-if="error" class="px-4 py-4 text-[12.5px] text-danger">{{ error }}</p>

        <p
          v-else-if="term.trim().length < 2"
          class="px-4 py-8 text-center text-[12.5px] text-ink-faint"
        >
          Type at least two characters.
        </p>

        <p
          v-else-if="!searching && results.length === 0"
          class="px-4 py-8 text-center text-[12.5px] text-ink-faint"
        >
          Nothing found for “{{ term }}”.
        </p>

        <ul v-else class="divide-y divide-line">
          <li
            v-for="r in results"
            :key="r.igdbId"
            class="flex items-center gap-3 px-4 py-2.5 transition-colors hover:bg-elevated/50"
          >
            <div class="h-14 w-[42px] shrink-0 overflow-hidden rounded-md bg-elevated">
              <GameCover :image-id="r.coverImageId" :name="r.name" size="cover_small" />
            </div>

            <div class="min-w-0 flex-1">
              <p class="truncate text-[13px]">{{ r.name }}</p>
              <p class="mt-0.5 truncate text-[11.5px] text-ink-faint">
                <span v-if="r.releaseYear">{{ r.releaseYear }}</span>
                <span v-if="r.releaseYear && r.platforms.length"> · </span>
                <span>{{ r.platforms.slice(0, 4).join(", ") }}</span>
              </p>
            </div>

            <span v-if="r.inLibrary" class="shrink-0 text-[11.5px] text-deal">In library</span>
            <div v-else class="flex shrink-0 gap-1.5">
              <button
                type="button"
                :disabled="adding === r.igdbId"
                class="rounded-lg border border-line bg-elevated px-2.5 py-1.5 text-[12px] text-ink-dim transition-colors hover:text-ink disabled:opacity-50"
                title="Add to your wishlist"
                @click="add(r, false)"
              >
                Want
              </button>
              <button
                type="button"
                :disabled="adding === r.igdbId"
                class="rounded-lg bg-accent px-2.5 py-1.5 text-[12px] font-medium text-white transition-opacity hover:opacity-90 disabled:opacity-50"
                title="Add to your backlog"
                @click="add(r, true)"
              >
                Own
              </button>
            </div>
          </li>
        </ul>
      </div>

      <p class="border-t border-line px-4 py-2 text-[11px] text-ink-faint">
        Game data by IGDB · Esc to close
      </p>
    </div>
  </div>
</template>
