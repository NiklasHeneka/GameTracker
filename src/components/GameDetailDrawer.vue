<script setup lang="ts">
import { computed, ref, watch } from "vue";

import GameCover from "./GameCover.vue";
import DealsPanel from "./DealsPanel.vue";
import { getGame } from "@/api/library";
import { igdbImage, releaseYear } from "@/composables/useIgdbImage";
import { BUCKETS, bucketOf } from "@/composables/buckets";
import { roundTo } from "@/composables/useFormat";
import { useLibraryStore } from "@/stores/library";
import type { GameDetail, LibraryEntry } from "@/types/models";

const props = defineProps<{ entry: LibraryEntry }>();
const emit = defineEmits<{ close: [] }>();

const library = useLibraryStore();
const detail = ref<GameDetail | null>(null);
const confirmingDelete = ref(false);

// The list only carries a summary; the drawer needs the description and
// credits, which are cached locally after the first fetch.
watch(
  () => props.entry.game.igdbId,
  async (id) => {
    detail.value = null;
    confirmingDelete.value = false;
    try {
      detail.value = await getGame(id);
    } catch {
      detail.value = null;
    }
  },
  { immediate: true },
);

const year = computed(() => releaseYear(props.entry.game.firstRelease));
const artwork = computed(() => igdbImage(detail.value?.artworkImageId, "720p", false));
const currentBucket = computed(() => bucketOf(props.entry));

const ownablePlatforms = computed(() =>
  props.entry.game.platforms.filter((p) => p.family !== "other"),
);

function move(key: string) {
  const bucket = BUCKETS.find((b) => b.key === key);
  if (bucket) library.patch(props.entry.id, bucket.patch);
}

function setNumber(field: "hoursPlayed" | "targetPrice" | "purchasePrice", raw: string) {
  const value = raw.trim() === "" ? null : Number(raw);
  if (value !== null && Number.isNaN(value)) return;
  library.patch(props.entry.id, { [field]: value });
}

async function remove() {
  await library.remove(props.entry.id);
  emit("close");
}
</script>

<template>
  <aside
    class="flex w-[420px] shrink-0 flex-col border-l border-line bg-surface"
    aria-label="Game details"
  >
    <!-- Hero -->
    <div class="relative shrink-0">
      <div class="h-32 w-full overflow-hidden bg-elevated">
        <img
          v-if="artwork"
          :src="artwork"
          alt=""
          class="h-full w-full object-cover opacity-45"
        />
      </div>
      <div class="pointer-events-none absolute inset-0 bg-gradient-to-t from-surface via-surface/70 to-transparent"></div>

      <button
        type="button"
        class="absolute right-3 top-3 z-20 grid h-7 w-7 place-items-center rounded-lg bg-black/50 text-white/80 backdrop-blur-sm transition-colors hover:bg-black/70 hover:text-white"
        aria-label="Close details"
        @click="emit('close')"
      >
        ✕
      </button>

      <div class="pointer-events-none absolute inset-x-0 bottom-0 z-10 flex items-end gap-3 px-5">
        <div class="h-28 w-20 shrink-0 overflow-hidden rounded-lg ring-1 ring-line-solid">
          <GameCover :image-id="entry.game.coverImageId" :name="entry.game.name" />
        </div>
        <div class="min-w-0 flex-1 pb-1">
          <h2 class="text-[16px] font-semibold leading-tight">{{ entry.game.name }}</h2>
          <p class="mt-1 truncate text-[12px] text-ink-faint">
            <span v-if="year">{{ year }}</span>
            <span v-if="year && detail?.developer"> · </span>
            <span v-if="detail?.developer">{{ detail.developer }}</span>
          </p>
        </div>
      </div>
    </div>

    <div class="pane-scroll min-h-0 flex-1 px-5 py-4">
      <!-- Status -->
      <div class="flex flex-wrap gap-1.5">
        <button
          v-for="b in BUCKETS"
          :key="b.key"
          type="button"
          class="rounded-lg px-2.5 py-1.5 text-[12px] transition-colors"
          :class="
            currentBucket.key === b.key
              ? 'bg-accent text-white'
              : 'bg-elevated text-ink-dim hover:text-ink'
          "
          :title="b.hint"
          @click="move(b.key)"
        >
          {{ b.label }}
        </button>
      </div>

      <!-- Chips -->
      <div v-if="entry.game.genres.length" class="mt-4 flex flex-wrap gap-1.5">
        <span
          v-for="g in entry.game.genres"
          :key="g"
          class="rounded-md bg-elevated px-2 py-1 text-[11px] text-ink-dim"
        >
          {{ g }}
        </span>
      </div>

      <div v-if="entry.game.platforms.length" class="mt-2 flex flex-wrap gap-1.5">
        <span
          v-for="p in entry.game.platforms"
          :key="p.id"
          class="rounded-md border border-line px-2 py-1 text-[11px] text-ink-faint"
          :title="p.name"
        >
          {{ p.abbreviation ?? p.name }}
        </span>
      </div>

      <p v-if="detail?.summaryText" class="mt-4 text-[12.5px] leading-relaxed text-ink-dim">
        {{ detail.summaryText }}
      </p>

      <!-- Your copy -->
      <h3 class="mt-6 text-[13px] font-semibold">Your copy</h3>
      <div class="mt-3 space-y-3">
        <label v-if="ownablePlatforms.length" class="block">
          <span class="mb-1.5 block text-[11.5px] text-ink-faint">Platform</span>
          <select
            class="w-full rounded-lg border border-line bg-elevated px-2.5 py-2 text-[12.5px]"
            :value="entry.ownPlatform ?? ''"
            @change="
              library.patch(entry.id, {
                ownPlatform: ($event.target as HTMLSelectElement).value || null,
              })
            "
          >
            <option value="">Not set</option>
            <option v-for="p in ownablePlatforms" :key="p.id" :value="p.abbreviation ?? p.name">
              {{ p.name }}
            </option>
          </select>
        </label>

        <div class="grid grid-cols-2 gap-3">
          <label class="block">
            <span class="mb-1.5 block text-[11.5px] text-ink-faint">Hours played</span>
            <input
              type="number"
              min="0"
              step="any"
              class="w-full rounded-lg border border-line bg-elevated px-2.5 py-2 text-[12.5px]"
              :value="entry.hoursPlayed == null ? '' : roundTo(entry.hoursPlayed, 2)"
              @change="setNumber('hoursPlayed', ($event.target as HTMLInputElement).value)"
            />
          </label>

          <label class="block">
            <span class="mb-1.5 block text-[11.5px] text-ink-faint">
              {{ entry.owned ? "Paid" : "Alert below" }}
            </span>
            <input
              type="number"
              min="0"
              step="0.01"
              class="w-full rounded-lg border border-line bg-elevated px-2.5 py-2 text-[12.5px]"
              :value="(entry.owned ? entry.purchasePrice : entry.targetPrice) ?? ''"
              @change="
                setNumber(
                  entry.owned ? 'purchasePrice' : 'targetPrice',
                  ($event.target as HTMLInputElement).value,
                )
              "
            />
          </label>
        </div>

        <div>
          <span class="mb-1.5 block text-[11.5px] text-ink-faint">Your rating</span>
          <div class="flex flex-wrap gap-1">
            <button
              v-for="n in 10"
              :key="n"
              type="button"
              class="h-7 w-7 rounded-md text-[11.5px] transition-colors"
              :class="
                entry.myRating === n
                  ? 'bg-accent text-white'
                  : 'bg-elevated text-ink-faint hover:text-ink'
              "
              @click="library.patch(entry.id, { myRating: entry.myRating === n ? null : n })"
            >
              {{ n }}
            </button>
          </div>
        </div>

        <label class="block">
          <span class="mb-1.5 block text-[11.5px] text-ink-faint">Notes</span>
          <textarea
            rows="3"
            class="w-full resize-none rounded-lg border border-line bg-elevated px-2.5 py-2 text-[12.5px] leading-relaxed"
            :value="entry.notes ?? ''"
            placeholder="Why you want it, where you left off…"
            @change="
              library.patch(entry.id, { notes: ($event.target as HTMLTextAreaElement).value })
            "
          ></textarea>
        </label>
      </div>

      <DealsPanel :igdb-id="entry.game.igdbId" />

      <div class="mt-6 border-t border-line pt-4">
        <button
          v-if="!confirmingDelete"
          type="button"
          class="text-[12px] text-ink-faint transition-colors hover:text-danger"
          @click="confirmingDelete = true"
        >
          Remove from library
        </button>
        <div v-else class="flex items-center gap-2">
          <span class="text-[12px] text-ink-dim">Remove “{{ entry.game.name }}”?</span>
          <button
            type="button"
            class="rounded-lg bg-danger/15 px-2.5 py-1 text-[12px] text-danger"
            @click="remove"
          >
            Remove
          </button>
          <button
            type="button"
            class="rounded-lg px-2 py-1 text-[12px] text-ink-faint hover:text-ink"
            @click="confirmingDelete = false"
          >
            Cancel
          </button>
        </div>
      </div>
    </div>
  </aside>
</template>
