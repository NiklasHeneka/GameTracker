<script setup lang="ts">
import { computed } from "vue";

import GameCover from "./GameCover.vue";
import { releaseYear } from "@/composables/useIgdbImage";
import type { LibraryEntry } from "@/types/models";

const props = defineProps<{ entry: LibraryEntry; selected?: boolean }>();
defineEmits<{ open: [] }>();

const year = computed(() => releaseYear(props.entry.game.firstRelease));

// One chip is enough on a card; the drawer lists them all.
const platform = computed(() => {
  const owned = props.entry.ownPlatform;
  if (owned) return owned;
  const first = props.entry.game.platforms[0];
  return first?.abbreviation ?? first?.name ?? null;
});
</script>

<template>
  <button
    type="button"
    class="group relative block w-full overflow-hidden rounded-[var(--radius-card)] bg-elevated text-left ring-1 transition-all duration-200 hover:-translate-y-0.5 hover:shadow-lg hover:shadow-black/40"
    :class="selected ? 'ring-accent' : 'ring-line hover:ring-line-solid'"
    @click="$emit('open')"
  >
    <div class="aspect-[3/4] w-full overflow-hidden">
      <GameCover
        :image-id="entry.game.coverImageId"
        :name="entry.game.name"
        class="transition-transform duration-300 group-hover:scale-[1.03]"
      />
    </div>

    <!-- Scrim keeps the title legible over any cover. -->
    <div class="pointer-events-none absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/95 via-black/70 to-transparent px-2.5 pb-2 pt-8">
      <p class="truncate text-[12px] font-medium leading-tight text-white">
        {{ entry.game.name }}
      </p>
      <p class="mt-0.5 truncate text-[10.5px] text-white/55">
        <span v-if="year">{{ year }}</span>
        <span v-if="year && platform"> · </span>
        <span v-if="platform">{{ platform }}</span>
      </p>
    </div>

    <span
      v-if="entry.myRating"
      class="absolute right-1.5 top-1.5 rounded-md bg-black/70 px-1.5 py-0.5 text-[10.5px] font-semibold text-white backdrop-blur-sm"
    >
      {{ entry.myRating }}
    </span>
  </button>
</template>
