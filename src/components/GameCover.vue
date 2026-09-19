<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { igdbImage, type IgdbSize } from "@/composables/useIgdbImage";

const props = withDefaults(
  defineProps<{
    imageId: string | null;
    name: string;
    size?: IgdbSize;
    /** `contain` shows the whole cover; `cover` fills and crops. */
    fit?: "cover" | "contain";
  }>(),
  { size: "cover_big", fit: "cover" },
);

const failed = ref(false);
const src = computed(() => igdbImage(props.imageId, props.size));

// A different game in the same slot must get a fresh chance to load.
watch(src, () => (failed.value = false));

/** Deterministic hue per title, so the placeholder is stable across renders. */
const hue = computed(() => {
  let h = 0;
  for (const ch of props.name) h = (h * 31 + ch.charCodeAt(0)) % 360;
  return h;
});

const initials = computed(() =>
  props.name
    .replace(/^(the|a|an)\s+/i, "")
    .split(/\s+/)
    .slice(0, 2)
    .map((w) => w[0] ?? "")
    .join("")
    .toUpperCase(),
);
</script>

<template>
  <img
    v-if="src && !failed"
    :src="src"
    :alt="`${name} cover art`"
    loading="lazy"
    decoding="async"
    class="h-full w-full"
    :class="fit === 'contain' ? 'object-contain' : 'object-cover'"
    @error="failed = true"
  />
  <!-- No cover on IGDB, or the CDN failed: never show a broken image. -->
  <div
    v-else
    class="grid h-full w-full place-items-center"
    :style="{
      background: `linear-gradient(150deg, hsl(${hue} 30% 22%), hsl(${(hue + 40) % 360} 28% 12%))`,
    }"
    :aria-label="`${name} (no cover art)`"
    role="img"
  >
    <span class="text-lg font-semibold tracking-wide text-white/45">{{ initials }}</span>
  </div>
</template>
