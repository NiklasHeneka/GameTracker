<script setup lang="ts">
import { computed } from "vue";
import { storeToRefs } from "pinia";

import AppIcon from "./AppIcon.vue";
import { useSettingsStore } from "@/stores/settings";

const NAV = [
  { to: "/", icon: "library", label: "Library" },
  { to: "/deals", icon: "tag", label: "Deals" },
  { to: "/stats", icon: "chart", label: "Stats" },
] as const;

const store = useSettingsStore();
const { config } = storeToRefs(store);

// Nudge the user to Settings until IGDB credentials are in place — without
// them the app cannot fetch a single game.
const needsSetup = computed(() => config.value !== null && !config.value.ready);

// `active-class` matches by prefix, which would keep "/" lit on every route.
// Every route here is a leaf, so exact matching is what we actually want.
const link =
  "group flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-[13px] " +
  "text-ink-dim transition-colors duration-150 hover:bg-elevated/60 hover:text-ink";
const linkActive = "bg-elevated !text-ink [&_svg]:text-accent-ink";
</script>

<template>
  <aside
    class="chrome flex w-60 shrink-0 flex-col border-r border-line bg-surface/70"
    data-tauri-drag-region
  >
    <!-- Clearance for the macOS traffic lights, which float over this column. -->
    <div :style="{ height: 'var(--sidebar-top)' }" data-tauri-drag-region></div>

    <div class="px-5 pb-5" data-tauri-drag-region>
      <div class="flex items-center gap-2.5">
        <span
          class="grid h-8 w-8 place-items-center rounded-lg bg-accent/15 text-accent-ink ring-1 ring-accent/25"
        >
          <AppIcon name="library" :size="17" />
        </span>
        <div class="leading-tight">
          <p class="text-[13px] font-semibold tracking-tight">GameTracker</p>
          <p class="text-[11px] text-ink-faint">Backlog &amp; prices</p>
        </div>
      </div>
    </div>

    <nav class="flex flex-col gap-0.5 px-3">
      <RouterLink
        v-for="item in NAV"
        :key="item.to"
        :to="item.to"
        :class="link"
        active-class=""
        :exact-active-class="linkActive"
      >
        <AppIcon :name="item.icon" :size="17" class="text-ink-faint group-hover:text-ink-dim" />
        {{ item.label }}
      </RouterLink>
    </nav>

    <div class="flex-1"></div>

    <div class="px-3 pb-4">
      <RouterLink to="/settings" :class="link" active-class="" :exact-active-class="linkActive">
        <AppIcon name="gear" :size="17" class="text-ink-faint group-hover:text-ink-dim" />
        Settings
        <span
          v-if="needsSetup"
          class="ml-auto rounded-full bg-sale/15 px-1.5 py-0.5 text-[10px] font-medium text-sale"
          title="IGDB credentials are missing"
        >
          setup
        </span>
      </RouterLink>
    </div>
  </aside>
</template>
