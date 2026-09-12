<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { openUrl } from "@tauri-apps/plugin-opener";

import PageHeader from "@/components/PageHeader.vue";
import EmptyState from "@/components/EmptyState.vue";
import AppIcon from "@/components/AppIcon.vue";
import GameCover from "@/components/GameCover.vue";
import SaleCountdown from "@/components/SaleCountdown.vue";
import { listActiveDeals, refreshWishlistPrices } from "@/api/prices";
import { money, shopLabel } from "@/composables/useFormat";
import { useLibraryStore } from "@/stores/library";
import type { DealRow } from "@/types/models";

const library = useLibraryStore();
const deals = ref<DealRow[]>([]);
const loading = ref(false);
const refreshing = ref(false);
const error = ref<string | null>(null);
const status = ref<string | null>(null);

async function load() {
  loading.value = true;
  error.value = null;
  try {
    deals.value = await listActiveDeals();
  } catch (e) {
    error.value = (e as { message?: string }).message ?? String(e);
  } finally {
    loading.value = false;
  }
}

async function refresh() {
  refreshing.value = true;
  status.value = null;
  error.value = null;
  try {
    const report = await refreshWishlistPrices();
    status.value =
      report.checked === 0
        ? "Nothing on your wishlist to check."
        : `Checked ${report.checked}` +
          (report.failed ? `, ${report.failed} failed` : "") +
          (report.alerts ? ` · ${report.alerts} hit your target price` : "");
    if (report.message) status.value += ` — ${report.message}`;
    await load();
  } catch (e) {
    error.value = (e as { message?: string }).message ?? String(e);
  } finally {
    refreshing.value = false;
  }
}

onMounted(async () => {
  if (!library.entries.length) await library.load();
  await load();
});

/**
 * Rank by how good the deal actually is, not just the headline percentage:
 * a 60% cut that is still triple its all-time low is worse than a 40% cut
 * sitting at the record low.
 */
const ranked = computed(() =>
  deals.value.slice().sort((a, b) => {
    if (a.meetsTarget !== b.meetsTarget) return a.meetsTarget ? -1 : 1;
    const av = a.vsAllTimeLow ?? 99;
    const bv = b.vsAllTimeLow ?? 99;
    if (Math.abs(av - bv) > 0.05) return av - bv;
    return b.cut - a.cut;
  }),
);

const wishlistCount = computed(
  () => library.entries.filter((e) => !e.owned && e.status === "want").length,
);
</script>

<template>
  <PageHeader
    title="Deals"
    :subtitle="
      deals.length
        ? `${deals.length} of your ${wishlistCount} wishlist games are discounted`
        : 'Wishlist games that are discounted right now'
    "
  >
    <template #actions>
      <button
        type="button"
        :disabled="refreshing"
        class="flex items-center gap-1.5 rounded-lg bg-accent px-3 py-1.5 text-[13px] font-medium text-white transition-opacity hover:opacity-90 disabled:opacity-50"
        @click="refresh"
      >
        <AppIcon name="refresh" :size="15" />
        {{ refreshing ? "Checking…" : "Check prices" }}
      </button>
    </template>
  </PageHeader>

  <div class="pane-scroll min-h-0 flex-1">
    <p v-if="error" class="px-8 pt-4 text-[12.5px] text-danger">{{ error }}</p>
    <p v-if="status" class="px-8 pt-4 text-[12px] text-ink-faint">{{ status }}</p>

    <p v-if="loading && !deals.length" class="px-8 pt-6 text-[12.5px] text-ink-faint">Loading…</p>

    <EmptyState
      v-else-if="!deals.length"
      icon="tag"
      :title="wishlistCount ? 'Nothing on sale right now' : 'No wishlist games yet'"
      :body="
        wishlistCount
          ? 'None of your wishlist games are discounted. Check prices to look again.'
          : 'Add games you want but do not own — those are the ones price tracking watches.'
      "
    >
      <button
        v-if="wishlistCount"
        type="button"
        :disabled="refreshing"
        class="rounded-lg bg-accent px-3 py-1.5 text-[13px] font-medium text-white disabled:opacity-50"
        @click="refresh"
      >
        {{ refreshing ? "Checking…" : "Check prices" }}
      </button>
    </EmptyState>

    <ul v-else class="px-8 py-5">
      <li
        v-for="deal in ranked"
        :key="deal.entryId"
        class="mb-2.5 flex items-center gap-3.5 rounded-xl border border-line bg-surface p-3 transition-colors hover:border-line-solid"
      >
        <div class="h-20 w-[60px] shrink-0 overflow-hidden rounded-md bg-elevated">
          <GameCover :image-id="deal.coverImageId" :name="deal.name" size="cover_small" />
        </div>

        <div class="min-w-0 flex-1">
          <p class="truncate text-[13.5px] font-medium">{{ deal.name }}</p>
          <p class="mt-1 flex flex-wrap items-center gap-2 text-[11.5px] text-ink-faint">
            <span>{{ shopLabel(deal.shop) }}</span>
            <SaleCountdown v-if="deal.saleExpiry" :expiry="deal.saleExpiry" />
            <span
              v-if="deal.meetsTarget"
              class="rounded-md bg-deal-soft px-1.5 py-0.5 font-medium text-deal"
            >
              below your {{ money(deal.targetPrice!, deal.currency) }} target
            </span>
          </p>
          <p class="mt-1 text-[11.5px] text-ink-dim">
            <template v-if="deal.vsAllTimeLow !== null && deal.vsAllTimeLow <= 1.001">
              <span class="text-deal">Lowest price ever recorded.</span>
            </template>
            <template v-else-if="deal.allTimeLow !== null">
              All-time low {{ money(deal.allTimeLow, deal.currency) }} ·
              now {{ deal.vsAllTimeLow?.toFixed(1) }}× that
            </template>
          </p>
        </div>

        <div class="shrink-0 text-right">
          <p class="text-[16px] font-semibold tracking-tight">
            {{ money(deal.price, deal.currency) }}
          </p>
          <p class="mt-0.5 text-[11.5px]">
            <s v-if="deal.regular" class="text-ink-faint">
              {{ money(deal.regular, deal.currency) }}
            </s>
            <span class="ml-1.5 text-deal">−{{ deal.cut }}%</span>
          </p>
          <button
            v-if="deal.url"
            type="button"
            class="mt-1.5 rounded-lg border border-line bg-elevated px-2.5 py-1 text-[12px] text-ink-dim transition-colors hover:text-ink"
            @click="openUrl(deal.url!)"
          >
            Open store
          </button>
        </div>
      </li>

      <p class="mt-4 text-[10.5px] text-ink-faint">
        Prices by IsThereAnyDeal &amp; CheapShark
      </p>
    </ul>
  </div>
</template>
