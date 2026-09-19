<script setup lang="ts">
import { onUnmounted, ref, watch } from "vue";

import AppIcon from "./AppIcon.vue";
import { money, shopLabel } from "@/composables/useFormat";
import type { QueueStore } from "@/types/models";

const props = defineProps<{ stores: QueueStore[]; selected: string; open: boolean }>();
const emit = defineEmits<{ toggle: []; select: [shop: string] }>();

const button = ref<HTMLButtonElement | null>(null);
const menu = ref<{ top: number; left: number } | null>(null);

const MENU_WIDTH = 216;

/**
 * The menu is teleported to the body and positioned by hand.
 *
 * Rendered in place it would be clipped by the scrolling list, and a dropdown
 * that only opens downwards is unusable on the last row. Teleporting also
 * keeps it out of the row that SortableJS clones while dragging.
 */
function place() {
  const rect = button.value?.getBoundingClientRect();
  if (!rect) return;

  // Below the button unless that would run off the bottom, in which case above.
  const height = Math.min(props.stores.length * 44 + 8, 280);
  const below = rect.bottom + 6;
  const top = below + height > window.innerHeight - 8 ? rect.top - height - 6 : below;

  menu.value = {
    top: Math.max(8, top),
    left: Math.min(rect.right - MENU_WIDTH, window.innerWidth - MENU_WIDTH - 8),
  };
}

function onOutside(e: MouseEvent) {
  if (!button.value?.contains(e.target as Node)) emit("toggle");
}
function onKey(e: KeyboardEvent) {
  if (e.key === "Escape") emit("toggle");
}
// A fixed menu would drift away from its button when the page moves under
// it; closing is the honest answer.
function onClose() {
  emit("toggle");
}

function bind(on: boolean) {
  if (on) {
    window.addEventListener("keydown", onKey);
    window.addEventListener("resize", onClose);
    // Capture, so scrolling the list itself counts and not just the window.
    window.addEventListener("scroll", onClose, true);
    document.addEventListener("mousedown", onOutside);
  } else {
    window.removeEventListener("keydown", onKey);
    window.removeEventListener("resize", onClose);
    window.removeEventListener("scroll", onClose, true);
    document.removeEventListener("mousedown", onOutside);
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) {
      place();
      bind(true);
    } else {
      menu.value = null;
      bind(false);
    }
  },
);
onUnmounted(() => bind(false));
</script>

<template>
  <button
    ref="button"
    type="button"
    class="flex max-w-full items-center gap-1 rounded-md border border-line bg-elevated px-1.5 py-0.5 text-[11px] text-ink-dim transition-colors hover:text-ink"
    :aria-expanded="open"
    aria-haspopup="listbox"
    @click.stop="emit('toggle')"
  >
    <span class="truncate">{{ shopLabel(selected) }}</span>
    <AppIcon name="chevron" :size="12" class="shrink-0 text-ink-faint" />
  </button>

  <Teleport to="body">
    <div
      v-if="open && menu"
      class="fixed z-[80] overflow-hidden rounded-lg border border-line-solid bg-overlay shadow-2xl shadow-black/60"
      :style="{ top: `${menu.top}px`, left: `${menu.left}px`, width: `${MENU_WIDTH}px` }"
      role="listbox"
      @mousedown.stop
    >
      <button
        v-for="store in stores"
        :key="store.shop"
        type="button"
        class="flex w-full items-center gap-2 px-2.5 py-2 text-left text-[12px] transition-colors hover:bg-elevated"
        :class="store.shop === selected ? 'text-ink' : 'text-ink-dim'"
        role="option"
        :aria-selected="store.shop === selected"
        @click="emit('select', store.shop)"
      >
        <AppIcon
          name="check"
          :size="13"
          class="shrink-0"
          :class="store.shop === selected ? 'text-accent-ink' : 'opacity-0'"
        />
        <span class="min-w-0 flex-1 truncate">{{ shopLabel(store.shop) }}</span>

        <span v-if="store.offer" class="shrink-0 tabular-nums">
          {{ money(store.offer.price, store.offer.currency) }}
        </span>
        <!-- Kept in the list rather than hidden: "not sold here" is an answer,
             and the store may still have a sale worth waiting for. -->
        <span v-else class="shrink-0 text-[11px] text-ink-faint">not sold</span>
      </button>
    </div>
  </Teleport>
</template>
