<script setup lang="ts">
defineProps<{ title: string; subtitle?: string }>();
</script>

<template>
  <header
    class="chrome flex h-[68px] shrink-0 items-end gap-4 border-b border-line px-8 pb-4"
    data-tauri-drag-region
  >
    <div class="min-w-0">
      <h1 class="truncate text-[19px] font-semibold tracking-tight">{{ title }}</h1>
      <p v-if="subtitle" class="mt-0.5 truncate text-[12.5px] text-ink-faint">{{ subtitle }}</p>
    </div>
    <div class="ml-auto flex items-center gap-2">
      <slot name="actions" />
    </div>
  </header>
</template>
