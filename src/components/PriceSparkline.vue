<script setup lang="ts">
import { computed } from "vue";
import type { HistoryPoint } from "@/types/models";

const props = withDefaults(defineProps<{ history: HistoryPoint[]; days?: number }>(), {
  days: 365,
});

const W = 300;
const H = 40;

const MAX_POINTS = 110;

/**
 * A year of daily points in 300px is half a pixel each, which renders as an
 * unreadable barcode — especially for a game that is discounted most days.
 * Bucket the window and keep the *cheapest* price in each bucket: the lows are
 * what a buyer is looking for, and the shape survives.
 */
const recent = computed(() => {
  const cutoff = Date.now() / 1000 - props.days * 86400;
  const points = props.history.filter((p) => p.ts >= cutoff).sort((a, b) => a.ts - b.ts);
  if (points.length <= MAX_POINTS) return points;

  const first = points[0]!.ts;
  const last = points[points.length - 1]!.ts;
  const width = Math.max(1, Math.ceil((last - first) / MAX_POINTS));

  const buckets = new Map<number, HistoryPoint>();
  for (const p of points) {
    const key = Math.floor((p.ts - first) / width);
    const held = buckets.get(key);
    if (!held || p.price < held.price) buckets.set(key, p);
  }
  return [...buckets.values()].sort((a, b) => a.ts - b.ts);
});

/**
 * Prices are a step function — a price holds until the next change — so the
 * line is drawn with square corners rather than sloping between points, which
 * would imply a gradual drift that never happened.
 */
const path = computed(() => {
  const pts = recent.value;
  if (pts.length < 2) return null;

  const t0 = pts[0]!.ts;
  const t1 = Math.max(pts[pts.length - 1]!.ts, t0 + 1);
  const prices = pts.map((p) => p.price);
  const lo = Math.min(...prices);
  const hi = Math.max(...prices);
  const span = hi - lo || 1;

  const x = (t: number) => ((t - t0) / (t1 - t0)) * W;
  const y = (p: number) => H - 2 - ((p - lo) / span) * (H - 6);

  let d = `M ${x(pts[0]!.ts).toFixed(1)} ${y(pts[0]!.price).toFixed(1)}`;
  for (let i = 1; i < pts.length; i++) {
    const px = x(pts[i]!.ts).toFixed(1);
    d += ` H ${px} V ${y(pts[i]!.price).toFixed(1)}`;
  }
  d += ` H ${W}`;
  return d;
});

const lowMarker = computed(() => {
  const pts = recent.value;
  if (pts.length < 2) return null;
  const cheapest = pts.reduce((a, b) => (b.price < a.price ? b : a));
  const t0 = pts[0]!.ts;
  const t1 = Math.max(pts[pts.length - 1]!.ts, t0 + 1);
  const prices = pts.map((p) => p.price);
  const lo = Math.min(...prices);
  const span = (Math.max(...prices) - lo) || 1;
  return {
    x: ((cheapest.ts - t0) / (t1 - t0)) * W,
    y: H - 2 - ((cheapest.price - lo) / span) * (H - 6),
  };
});
</script>

<template>
  <svg
    v-if="path"
    :viewBox="`0 0 ${W} ${H}`"
    class="h-10 w-full"
    preserveAspectRatio="none"
    role="img"
    aria-label="Price over the last year"
  >
    <path :d="path" fill="none" stroke="var(--color-accent-ink)" stroke-width="1.5"
          vector-effect="non-scaling-stroke" />
    <circle v-if="lowMarker" :cx="lowMarker.x" :cy="lowMarker.y" r="2.5"
            fill="var(--color-deal)" />
  </svg>
  <p v-else class="py-2 text-[11px] text-ink-faint">Not enough price history yet.</p>
</template>
