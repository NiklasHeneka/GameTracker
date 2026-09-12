<script setup lang="ts">
import draggable from "vuedraggable";

import GameCard from "./GameCard.vue";
import type { Bucket, DraggableChange } from "@/composables/buckets";
import type { LibraryEntry } from "@/types/models";

defineProps<{ bucket: Bucket; selectedId: number | null }>();
const emit = defineEmits<{ open: [entry: LibraryEntry]; change: [evt: DraggableChange] }>();

// vuedraggable owns this array and mutates it as you drag. Binding one-way
// would let Vue re-render from unchanged state and snap the card back.
const entries = defineModel<LibraryEntry[]>({ required: true });
</script>

<template>
  <section class="flex w-0 min-w-[236px] flex-1 flex-col">
    <header class="mb-2.5 flex items-baseline gap-2 px-0.5">
      <h2 class="text-[12.5px] font-semibold">{{ bucket.label }}</h2>
      <span class="text-[11.5px] text-ink-faint">{{ entries.length }}</span>
      <span class="ml-auto truncate text-[10.5px] text-ink-faint">{{ bucket.hint }}</span>
    </header>

    <div class="relative min-h-0 flex-1">
      <!-- Behind the list rather than inside it: a child of the sortable
           container would itself become a drop slot. -->
      <p
        v-if="entries.length === 0"
        class="pointer-events-none absolute inset-x-0 top-8 text-center text-[11.5px] text-ink-faint"
      >
        Drag games here
      </p>

      <draggable
        v-model="entries"
        :group="{ name: 'library' }"
        item-key="id"
        class="pane-scroll grid h-full auto-rows-max grid-cols-2 content-start gap-2.5 rounded-xl border border-dashed border-line-solid/60 p-2.5"
        :animation="180"
        :swap-threshold="0.7"
        :scroll-sensitivity="90"
        :scroll-speed="14"
        ghost-class="gt-ghost"
        chosen-class="gt-chosen"
        fallback-class="gt-dragging"
        drag-class="gt-dragging"
        :force-fallback="true"
        :fallback-on-body="true"
        :fallback-tolerance="5"
        @change="emit('change', $event)"
      >
        <template #item="{ element }">
          <div class="cursor-grab active:cursor-grabbing">
            <GameCard
              :entry="element"
              :selected="element.id === selectedId"
              @open="emit('open', element)"
            />
          </div>
        </template>
      </draggable>
    </div>
  </section>
</template>
