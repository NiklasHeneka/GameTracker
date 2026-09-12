<script setup lang="ts">
import AppIcon from "./AppIcon.vue";

defineProps<{ icon?: string; title: string; body?: string }>();
</script>

<template>
  <div class="grid flex-1 place-items-center px-8 py-16">
    <div class="max-w-sm text-center">
      <span
        class="mx-auto mb-4 grid h-12 w-12 place-items-center rounded-xl bg-elevated text-ink-faint ring-1 ring-line"
      >
        <AppIcon :name="icon ?? 'library'" :size="22" />
      </span>
      <h2 class="text-[15px] font-medium">{{ title }}</h2>
      <p v-if="body" class="mt-1.5 text-[13px] leading-relaxed text-ink-faint">{{ body }}</p>
      <div class="mt-5"><slot /></div>
    </div>
  </div>
</template>
