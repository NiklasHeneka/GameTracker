<script setup lang="ts">
import { computed, onUnmounted, ref } from "vue";
import { until } from "@/composables/useFormat";

const props = defineProps<{ expiry: number }>();

// Tick once a minute so "3d 4h" stays honest without burning a frame budget.
const nowMs = ref(Date.now());
const timer = setInterval(() => (nowMs.value = Date.now()), 60_000);
onUnmounted(() => clearInterval(timer));

const remaining = computed(() => until(props.expiry, nowMs.value));
// Under a day is worth acting on.
const urgent = computed(() => props.expiry - nowMs.value / 1000 < 86_400);
</script>

<template>
  <span
    v-if="remaining"
    class="rounded-md px-1.5 py-0.5 text-[10.5px] font-medium"
    :class="urgent ? 'bg-danger/15 text-danger' : 'bg-sale/15 text-sale'"
    :title="`Sale ends ${new Date(expiry * 1000).toLocaleString()}`"
  >
    ends in {{ remaining }}
  </span>
</template>
