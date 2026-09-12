<script setup lang="ts">
import { onMounted } from "vue";

import { ref } from "vue";

import AppSidebar from "@/components/AppSidebar.vue";
import CommandPalette from "@/components/CommandPalette.vue";
import AddGameModal from "@/components/AddGameModal.vue";
import { useSettingsStore } from "@/stores/settings";

const store = useSettingsStore();

// The palette can ask for the add-game dialog from any view, so it lives here
// rather than inside the library.
const adding = ref(false);

// One load at startup; every view reads from the store rather than re-invoking.
onMounted(store.load);
</script>

<template>
  <div class="flex h-full bg-canvas text-ink">
    <AppSidebar />
    <main class="flex min-w-0 flex-1 flex-col">
      <RouterView v-slot="{ Component }">
        <component :is="Component" />
      </RouterView>
    </main>

    <CommandPalette @add-game="adding = true" />
    <AddGameModal v-if="adding" @close="adding = false" />
  </div>
</template>
