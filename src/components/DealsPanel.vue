<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { openUrl } from "@tauri-apps/plugin-opener";

import AppIcon from "./AppIcon.vue";
import SaleCountdown from "./SaleCountdown.vue";
import PriceSparkline from "./PriceSparkline.vue";
import SaleOutlook from "./SaleOutlook.vue";
import ManualPriceForm from "./ManualPriceForm.vue";
import { getPrices } from "@/api/prices";
import { daysAgo, money, shopLabel, shortDate } from "@/composables/useFormat";
import type { PriceOverview } from "@/types/models";

const props = defineProps<{ igdbId: number }>();

const data = ref<PriceOverview | null>(null);
const loading = ref(false);
const error = ref<string | null>(null);

const FAMILY_LABEL: Record<string, string> = {
  pc: "PC",
  playstation: "PlayStation",
  nintendo: "Nintendo",
  xbox: "Xbox",
};

async function load(force = false) {
  loading.value = true;
  error.value = null;
  try {
    data.value = await getPrices(props.igdbId, force);
  } catch (e) {
    error.value = (e as { message?: string }).message ?? String(e);
  } finally {
    loading.value = false;
  }
}

watch(() => props.igdbId, () => { data.value = null; load(); }, { immediate: true });

const allTimeLow = computed(() => data.value?.lows.find((l) => l.scope === "all") ?? null);
const best = computed(() => {
  const rows = data.value?.groups.flatMap((g) => g.rows) ?? [];
  return rows.length ? rows.reduce((a, b) => (b.price < a.price ? b : a)) : null;
});

/**
 * The genuinely useful buy/wait signal: how the best price now compares with
 * the best price ever seen.
 */
const onSaleNow = computed(() => (best.value?.cut ?? 0) > 0);

// Price a manual entry in whatever the automatic sources are already quoting,
// so the panel does not end up mixing currencies.
const currency = computed(() => best.value?.currency ?? data.value?.lows[0]?.currency ?? "EUR");

const versusLow = computed(() => {
  const low = allTimeLow.value?.price;
  const now = best.value?.price;
  if (!low || !now || low <= 0) return null;
  return now / low;
});

async function open(url: string | null) {
  if (url) await openUrl(url);
}
</script>

<template>
  <section class="mt-6">
    <div class="flex items-baseline justify-between gap-3">
      <h3 class="text-[13px] font-semibold">Prices</h3>
      <button
        type="button"
        class="flex items-center gap-1 text-[11.5px] text-ink-faint transition-colors hover:text-ink"
        :disabled="loading"
        @click="load(true)"
      >
        <AppIcon name="refresh" :size="12" />
        {{ loading ? "checking…" : "refresh" }}
      </button>
    </div>

    <p v-if="loading && !data" class="mt-3 text-[12px] text-ink-faint">Checking stores…</p>
    <p v-else-if="error" class="mt-3 text-[12px] text-danger">{{ error }}</p>

    <template v-else-if="data">
      <!-- Buy/wait signal -->
      <div v-if="best && allTimeLow" class="mt-3 rounded-xl border border-line bg-elevated px-3.5 py-3">
        <div class="flex items-baseline gap-2">
          <span class="text-[19px] font-semibold tracking-tight">
            {{ money(best.price, best.currency) }}
          </span>
          <span v-if="best.cut > 0" class="rounded-md bg-deal-soft px-1.5 py-0.5 text-[11px] font-medium text-deal">
            −{{ best.cut }}%
          </span>
          <span class="ml-auto text-[11px] text-ink-faint">{{ shopLabel(best.shop) }}</span>
        </div>
        <p class="mt-1.5 text-[11.5px] leading-relaxed text-ink-dim">
          <template v-if="versusLow !== null && versusLow <= 1.001">
            <span class="text-deal">At its lowest price ever.</span>
          </template>
          <template v-else-if="versusLow !== null">
            All-time low {{ money(allTimeLow.price, allTimeLow.currency) }}<template
              v-if="allTimeLow.occurredAt"> ({{ shortDate(allTimeLow.occurredAt) }})</template>
            — currently <strong class="text-ink">{{ versusLow.toFixed(1) }}×</strong> that.
          </template>
        </p>
      </div>

      <!-- Per-store rows, grouped by platform -->
      <div v-for="group in data.groups" :key="group.family" class="mt-3.5">
        <p v-if="data.groups.length > 1" class="mb-1.5 text-[11px] font-medium text-ink-faint">
          {{ FAMILY_LABEL[group.family] ?? group.family }}
        </p>
        <ul class="overflow-hidden rounded-xl border border-line">
          <li
            v-for="(row, i) in group.rows"
            :key="row.shop"
            class="flex items-center gap-2.5 bg-surface px-3 py-2 transition-colors"
            :class="[i > 0 && 'border-t border-line', row.url && 'cursor-pointer hover:bg-elevated']"
            @click="open(row.url)"
          >
            <span class="min-w-0 flex-1 truncate text-[12.5px]" :title="row.shop">{{ shopLabel(row.shop) }}</span>

            <SaleCountdown v-if="row.saleExpiry" :expiry="row.saleExpiry" />

            <span
              v-if="row.isAllTimeLow"
              class="rounded-md bg-deal-soft px-1.5 py-0.5 text-[10.5px] font-medium text-deal"
              title="Matches the lowest price ever recorded"
            >
              low
            </span>
            <span v-if="row.source === 'manual'" class="text-[10.5px] text-ink-faint">
              entered by you
            </span>

            <span v-if="row.cut > 0" class="text-[11px] text-deal">−{{ row.cut }}%</span>
            <s v-if="row.regular && row.cut > 0" class="text-[11px] text-ink-faint">
              {{ money(row.regular, row.currency) }}
            </s>
            <span class="w-[68px] text-right text-[12.5px] font-medium">
              {{ money(row.price, row.currency) }}
            </span>
          </li>
        </ul>
      </div>

      <p v-if="!data.groups.length" class="mt-3 text-[12px] text-ink-faint">
        No store listings found for this game.
      </p>

      <!-- History -->
      <template v-if="data.history.length > 1">
        <p class="mt-4 mb-1 text-[11px] font-medium text-ink-faint">Last 12 months</p>
        <PriceSparkline :history="data.history" />
      </template>

      <!-- What the history says -->
      <div class="mt-3 space-y-1.5 text-[11.5px] leading-relaxed text-ink-dim">
        <!-- While a discount is running it *is* the last sale, so reporting it
             again as "0 days ago" only confuses. -->
        <p v-if="data.lastSale && !onSaleNow">
          Last sale: <strong class="text-ink">−{{ data.lastSale.cut }}%</strong>
          ({{ money(data.lastSale.price, data.lastSale.currency) }}),
          {{ daysAgo(data.lastSale.occurredAt) }} days ago.
        </p>
        <p v-if="data.pattern">
          Discounted on
          <strong class="text-ink">{{ data.pattern.sharePercent }}%</strong>
          of the last {{ data.pattern.daysObserved }} days with price data;
          the deepest was <strong class="text-ink">−{{ data.pattern.deepestCut }}%</strong>.
        </p>
      </div>

      <SaleOutlook :outlook="data.saleOutlook" />

      <ManualPriceForm
        :igdb-id="igdbId"
        :currency="currency"
        @saved="(fresh) => (data = fresh)"
      />

      <p v-if="data.note" class="mt-3 rounded-lg bg-elevated px-3 py-2 text-[11px] leading-relaxed text-ink-faint">
        {{ data.note }}
      </p>

      <p class="mt-3 text-[10.5px] text-ink-faint">
        Prices by IsThereAnyDeal &amp; CheapShark<template v-if="data.fetchedAt">
          · checked {{ shortDate(data.fetchedAt) }}</template>
      </p>
    </template>
  </section>
</template>
