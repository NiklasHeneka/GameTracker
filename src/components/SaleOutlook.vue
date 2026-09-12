<script setup lang="ts">
import { money, shopLabel, shortDate } from "@/composables/useFormat";
import type { StoreSaleOutlook } from "@/types/models";

defineProps<{ outlook: StoreSaleOutlook[] }>();

function when(o: StoreSaleOutlook): string {
  if (o.liveNow) return "on now";
  if (o.daysAway === 0) return "starts today";
  if (o.daysAway === 1) return "starts tomorrow";
  return `in ${o.daysAway} days`;
}
</script>

<template>
  <section v-if="outlook.length" class="mt-4">
    <h4 class="mb-2 text-[11px] font-medium text-ink-faint">Next storewide sales</h4>

    <ul class="space-y-2">
      <li v-for="o in outlook" :key="o.shop" class="rounded-lg border border-line bg-surface px-3 py-2">
        <div class="flex items-baseline gap-2">
          <span class="text-[12.5px] font-medium">{{ shopLabel(o.shop) }}</span>
          <span class="truncate text-[11.5px] text-ink-dim">{{ o.event }}</span>
          <span
            class="ml-auto shrink-0 rounded-md px-1.5 py-0.5 text-[10.5px] font-medium"
            :class="o.liveNow ? 'bg-deal-soft text-deal' : 'bg-elevated text-ink-dim'"
          >
            {{ when(o) }}
          </span>
        </div>

        <p class="mt-1 text-[11px] text-ink-faint">
          {{ shortDate(o.nextStart) }} – {{ shortDate(o.nextEnd) }}
          <!-- Only the store can confirm dates; anything else is last year's
               pattern and must not be presented as fact. -->
          <span v-if="!o.confirmed" title="Estimated from previous years, not announced">
            · estimated
          </span>
        </p>

        <p v-if="o.lastEvent" class="mt-1 text-[11px] leading-relaxed text-ink-dim">
          <template v-if="o.lastEvent.hadData && (o.lastEvent.bestCut ?? 0) > 0">
            Last {{ o.lastEvent.event }}:
            <strong class="text-ink">−{{ o.lastEvent.bestCut }}%</strong>
            ({{ money(o.lastEvent.bestPrice!, o.lastEvent.currency) }})
          </template>
          <template v-else-if="o.lastEvent.hadData">
            Not discounted here in the last {{ o.lastEvent.event }}.
          </template>
          <template v-else>
            No price recorded during the last {{ o.lastEvent.event }}.
          </template>
        </p>
      </li>
    </ul>
  </section>
</template>
