<script setup lang="ts">
import { computed, onMounted, ref } from "vue";

import PageHeader from "@/components/PageHeader.vue";
import EmptyState from "@/components/EmptyState.vue";
import { getStats } from "@/api/import";
import { money } from "@/composables/useFormat";
import type { Stats } from "@/types/models";

const stats = ref<Stats | null>(null);
const loading = ref(false);
const error = ref<string | null>(null);

onMounted(async () => {
  loading.value = true;
  try {
    stats.value = await getStats();
  } catch (e) {
    error.value = (e as { message?: string }).message ?? String(e);
  } finally {
    loading.value = false;
  }
});

const genreMax = computed(() =>
  Math.max(1, ...(stats.value?.topGenres ?? []).map((g) => g.count)),
);

function hours(value: number): string {
  if (value < 1) return "—";
  return value >= 100 ? `${Math.round(value)}` : value.toFixed(1);
}
</script>

<template>
  <PageHeader title="Stats" subtitle="What your library adds up to" />

  <div class="pane-scroll min-h-0 flex-1">
    <p v-if="error" class="px-8 pt-4 text-[12.5px] text-danger">{{ error }}</p>
    <p v-else-if="loading" class="px-8 pt-6 text-[12.5px] text-ink-faint">Loading…</p>

    <EmptyState
      v-else-if="!stats || stats.total === 0"
      icon="chart"
      title="Nothing to measure yet"
      body="Add some games — or import your Steam library from Settings — and this fills in."
    />

    <div v-else class="mx-auto max-w-3xl px-8 py-6">
      <!-- Headline counts -->
      <div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <div
          v-for="tile in [
            { label: 'Tracked', value: String(stats.total) },
            { label: 'Backlog', value: String(stats.backlog) },
            { label: 'Playing', value: String(stats.playing) },
            { label: 'Finished', value: String(stats.finished) },
          ]"
          :key="tile.label"
          class="rounded-xl border border-line bg-surface px-4 py-3"
        >
          <p class="text-[22px] font-semibold tracking-tight">{{ tile.value }}</p>
          <p class="mt-0.5 text-[11.5px] text-ink-faint">{{ tile.label }}</p>
        </div>
      </div>

      <!-- Time and money -->
      <div class="mt-3 grid grid-cols-1 gap-3 sm:grid-cols-3">
        <div class="rounded-xl border border-line bg-surface px-4 py-3">
          <p class="text-[22px] font-semibold tracking-tight">{{ hours(stats.hoursPlayed) }}</p>
          <p class="mt-0.5 text-[11.5px] text-ink-faint">Hours played</p>
        </div>
        <div class="rounded-xl border border-line bg-surface px-4 py-3">
          <p class="text-[22px] font-semibold tracking-tight">
            {{ money(stats.moneySpent, stats.currency) }}
          </p>
          <p class="mt-0.5 text-[11.5px] text-ink-faint">Spent on recorded purchases</p>
        </div>
        <div class="rounded-xl border border-line bg-surface px-4 py-3">
          <p class="text-[22px] font-semibold tracking-tight">
            {{ money(stats.wishlistValue, stats.currency) }}
          </p>
          <p class="mt-0.5 text-[11.5px] text-ink-faint">
            Wishlist at today's best prices ({{ stats.wishlist }} games)
          </p>
        </div>
      </div>

      <!-- Saved by waiting: only meaningful once there is something to compare -->
      <div
        v-if="stats.savedFrom > 0"
        class="mt-3 rounded-xl border border-line bg-surface px-4 py-3.5"
      >
        <p class="text-[13px]">
          Waiting has saved you
          <strong :class="stats.savedByWaiting >= 0 ? 'text-deal' : 'text-danger'">
            {{ money(Math.abs(stats.savedByWaiting), stats.currency) }}
          </strong>
          <template v-if="stats.savedByWaiting < 0"> less than nothing</template>
          across {{ stats.savedFrom }}
          {{ stats.savedFrom === 1 ? "purchase" : "purchases" }}.
        </p>
        <p class="mt-1 text-[11.5px] leading-relaxed text-ink-faint">
          Comparing the cheapest price when you added each game with what you actually paid.
        </p>
      </div>
      <p v-else class="mt-3 text-[11.5px] leading-relaxed text-ink-faint">
        Once you buy something from your wishlist and record the price, this will show what
        waiting was worth.
      </p>

      <!-- Genres -->
      <template v-if="stats.topGenres.length">
        <h2 class="mt-8 text-[14px] font-semibold">What you collect</h2>
        <ul class="mt-3 space-y-1.5">
          <li v-for="g in stats.topGenres" :key="g.name" class="flex items-center gap-3">
            <span class="w-40 shrink-0 truncate text-[12.5px] text-ink-dim">{{ g.name }}</span>
            <span class="h-2 flex-1 overflow-hidden rounded-full bg-elevated">
              <span
                class="block h-full rounded-full bg-accent/70"
                :style="{ width: `${(g.count / genreMax) * 100}%` }"
              ></span>
            </span>
            <span class="w-6 shrink-0 text-right text-[11.5px] text-ink-faint">{{ g.count }}</span>
          </li>
        </ul>
      </template>

      <div class="mt-8 space-y-1.5 text-[12.5px] text-ink-dim">
        <p v-if="stats.averageRating !== null">
          Your average rating is
          <strong class="text-ink">{{ stats.averageRating.toFixed(1) }}</strong> / 10.
        </p>
        <p v-if="stats.longestWait">
          Longest on the wishlist:
          <strong class="text-ink">{{ stats.longestWait.name }}</strong>,
          {{ stats.longestWait.days }} days.
        </p>
      </div>
    </div>
  </div>
</template>
