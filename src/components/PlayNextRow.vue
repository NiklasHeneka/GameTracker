<script setup lang="ts">
import { computed } from "vue";

import AppIcon from "./AppIcon.vue";
import GameCover from "./GameCover.vue";
import StorePicker from "./StorePicker.vue";
import { releaseYear } from "@/composables/useIgdbImage";
import { bucketOf } from "@/composables/buckets";
import { money } from "@/composables/useFormat";
import { useLibraryStore } from "@/stores/library";
import type { QueueRow } from "@/types/models";

const props = defineProps<{
  row: QueueRow;
  rank: number;
  selected: boolean;
  pickerOpen: boolean;
}>();
const emit = defineEmits<{
  open: [];
  remove: [];
  togglePicker: [];
  pickShop: [shop: string];
}>();

const library = useLibraryStore();

const entry = computed(() => props.row.entry);
const game = computed(() => entry.value.game);
const year = computed(() => releaseYear(game.value.firstRelease));
const bucket = computed(() => bucketOf(entry.value));

/** Platforms the game exists on, deduplicated by label. */
const platforms = computed(() => {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const p of game.value.platforms) {
    const label = p.abbreviation ?? p.name;
    if (seen.has(label)) continue;
    seen.add(label);
    out.push(label);
  }
  return out;
});

/**
 * The platforms it makes sense to *own* a copy on — the same list the detail
 * drawer offers, so the two pickers cannot disagree about what to store.
 */
const ownable = computed(() =>
  game.value.platforms.filter((p) => p.family !== "other"),
);

const hours = computed(() => {
  const h = entry.value.hoursPlayed;
  if (h === null || h < 1) return null;
  return `${Math.round(h)} h played`;
});

// ── Prices, for games not owned yet ──────────────────────────────────────

/** An unset preference means "whichever is cheapest", and that is the order. */
const store = computed(
  () =>
    props.row.stores.find((s) => s.shop === props.row.preferredShop) ??
    props.row.stores[0] ??
    null,
);

/** "on now", "in 11 days" — how far off this store's next storewide sale is. */
const saleTiming = computed(() => {
  const o = store.value?.outlook;
  if (!o) return null;
  if (o.liveNow) return "on now";
  if (o.daysAway === 0) return "starts today";
  if (o.daysAway === 1) return "tomorrow";
  return `in ${o.daysAway} days`;
});

/** What it cost during the previous run of that same sale. */
const lastTime = computed(() => {
  const last = store.value?.outlook?.lastEvent;
  if (!last) return null;
  if (!last.hadData) return "no data last time";
  if (!last.bestCut) return "not discounted last time";
  return `last time −${last.bestCut}% (${money(last.bestPrice!, last.currency)})`;
});
</script>

<template>
  <article
    class="group relative flex items-stretch gap-3 rounded-[var(--radius-card)] bg-surface ring-1 transition-colors"
    :class="selected ? 'ring-accent' : 'ring-line hover:ring-line-solid'"
  >
    <!-- Dragging is handle-only. The row carries buttons and a picker, and a
         drag started on those would swallow the click. -->
    <button
      type="button"
      class="gt-grip shrink-0 cursor-grab rounded-l-[var(--radius-card)] px-1.5 text-ink-faint transition-colors hover:bg-elevated/60 hover:text-ink-dim active:cursor-grabbing"
      :aria-label="`Reorder ${game.name}`"
      tabindex="-1"
    >
      <AppIcon name="grip" :size="15" />
    </button>

    <span
      class="w-5 shrink-0 self-center text-right text-[12.5px] tabular-nums text-ink-faint"
      aria-hidden="true"
    >
      {{ rank }}
    </span>

    <button
      type="button"
      class="flex min-w-0 flex-1 items-center gap-3.5 py-2.5 pr-2 text-left"
      @click="emit('open')"
    >
      <!-- The row is landscape but a cover is 3:4, and IGDB's "artworks" are
           as often a logo on white as a screenshot, which crops to unreadable
           lettering. So: the cover's own colours blurred to fill the width,
           the cover itself sharp and whole on top. Every game has a cover, so
           this degrades predictably where artwork does not. -->
      <div
        class="relative aspect-video w-[168px] shrink-0 overflow-hidden rounded-lg bg-elevated @max-[46rem]:w-[104px]"
      >
        <!-- Scaled well past the box so the blur's soft edge never shows. -->
        <div class="absolute inset-0 scale-150 opacity-70 blur-lg saturate-150" aria-hidden="true">
          <GameCover :image-id="game.coverImageId" :name="game.name" size="cover_small" />
        </div>
        <div class="absolute inset-0 bg-black/25" aria-hidden="true"></div>
        <div class="absolute inset-0">
          <GameCover
            :image-id="game.coverImageId"
            :name="game.name"
            size="cover_small"
            fit="contain"
          />
        </div>
      </div>

      <div class="min-w-0 flex-1">
        <p class="truncate text-[14px] font-medium leading-tight">{{ game.name }}</p>

        <p class="mt-1 truncate text-[11.5px] text-ink-faint">
          <span v-if="year">{{ year }}</span>
          <span v-if="year && platforms.length"> · </span>
          <span>{{ platforms.slice(0, 5).join(" · ") }}</span>
        </p>

        <!-- Never wrap: in a narrow pane a second row of chips pushes the
             row taller than its own artwork. The mask fades whatever does not
             fit, so a clipped chip reads as deliberate rather than broken. -->
        <div
          v-if="game.genres.length"
          class="mt-2 flex gap-1.5 overflow-hidden [mask-image:linear-gradient(to_right,black_85%,transparent)]"
        >
          <span
            v-for="genre in game.genres.slice(0, 3)"
            :key="genre"
            class="shrink-0 rounded-md bg-elevated px-1.5 py-0.5 text-[10.5px] text-ink-dim"
          >
            {{ genre }}
          </span>
        </div>
      </div>
    </button>

    <!-- Owned games show ownership instead of a price: once it is bought, what
         it costs is a question already answered. Phase 5b puts the store
         picker and the price under the "not owned" branch. -->
    <div
      class="flex w-[210px] shrink-0 flex-col items-end justify-center gap-1 py-2.5 pr-3 @max-[46rem]:w-[148px]"
    >
      <template v-if="entry.owned">
        <span class="flex items-center gap-1.5 text-[12.5px] font-medium text-deal">
          <AppIcon name="check" :size="14" />
          Owned
        </span>

        <span v-if="entry.ownPlatform" class="text-[11.5px] text-ink-dim">
          {{ entry.ownPlatform }}
        </span>
        <!-- Games added by hand have no platform stored; `add_entry` defaults
             it to null. Offer to fill it in rather than showing a dangling
             "Owned on". -->
        <select
          v-else-if="ownable.length"
          class="max-w-full rounded-md border border-line bg-elevated px-1.5 py-0.5 text-[11px] text-ink-dim"
          :value="''"
          :aria-label="`Which platform do you own ${game.name} on?`"
          @change="
            library.patch(entry.id, {
              ownPlatform: ($event.target as HTMLSelectElement).value || null,
            })
          "
          @click.stop
        >
          <option value="">Which platform?</option>
          <option v-for="p in ownable" :key="p.id" :value="p.abbreviation ?? p.name">
            {{ p.name }}
          </option>
        </select>

        <span v-if="hours" class="text-[11px] text-ink-faint">{{ hours }}</span>
      </template>

      <template v-else>
        <!-- One store at a time. Five stores' prices and five sale dates on
             every row would be a wall of numbers nobody reads. -->
        <StorePicker
          v-if="store"
          :stores="row.stores"
          :selected="store.shop"
          :open="pickerOpen"
          @toggle="emit('togglePicker')"
          @select="emit('pickShop', $event)"
        />
        <span v-else class="text-[12.5px] text-ink-dim">{{ bucket.label }}</span>

        <template v-if="store?.offer">
          <!-- Wraps rather than overflowing: "−80% 9,99 € 1,99 €" is wider
               than the column once the drawer narrows it. -->
          <span class="flex max-w-full flex-wrap items-baseline justify-end gap-1.5">
            <span
              v-if="store.offer.cut > 0"
              class="rounded-md bg-deal-soft px-1 py-0.5 text-[10.5px] font-medium text-deal"
            >
              −{{ store.offer.cut }}%
            </span>
            <span
              v-if="store.offer.regular && store.offer.cut > 0"
              class="text-[10.5px] text-ink-faint line-through"
            >
              {{ money(store.offer.regular, store.offer.currency) }}
            </span>
            <span class="text-[13px] font-medium tabular-nums">
              {{ money(store.offer.price, store.offer.currency) }}
            </span>
          </span>
          <span v-if="store.offer.isAllTimeLow" class="text-[10.5px] text-deal">
            all-time low
          </span>
        </template>
        <span v-else-if="store" class="text-[11.5px] text-ink-faint">Not sold here</span>
        <span v-else class="text-[11px] text-ink-faint">No prices yet</span>

        <span v-if="saleTiming" class="max-w-full truncate text-[11px] text-ink-dim">
          {{ store!.outlook!.event }} {{ saleTiming }}
        </span>
        <span v-if="lastTime" class="max-w-full truncate text-[10.5px] text-ink-faint">
          {{ lastTime }}
        </span>
      </template>
    </div>

    <button
      type="button"
      class="absolute right-1.5 top-1.5 rounded-md p-1 text-ink-faint opacity-0 transition-opacity hover:bg-elevated hover:text-ink focus-visible:opacity-100 group-hover:opacity-100"
      :aria-label="`Remove ${game.name} from Play Next`"
      title="Remove from Play Next"
      @click.stop="emit('remove')"
    >
      <AppIcon name="close" :size="14" />
    </button>
  </article>
</template>
