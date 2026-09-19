<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { storeToRefs } from "pinia";
import draggable from "vuedraggable";

import PageHeader from "@/components/PageHeader.vue";
import EmptyState from "@/components/EmptyState.vue";
import AppIcon from "@/components/AppIcon.vue";
import AddFromLibrary from "@/components/AddFromLibrary.vue";
import GameDetailDrawer from "@/components/GameDetailDrawer.vue";
import PlayNextRow from "@/components/PlayNextRow.vue";
import { useLibraryStore } from "@/stores/library";
import { useQueueStore } from "@/stores/queue";
import type { QueueRow } from "@/types/models";

const library = useLibraryStore();
const queue = useQueueStore();
const { entries, selected, selectedId } = storeToRefs(library);
const { rows, loading, error } = storeToRefs(queue);

const adding = ref(false);

onMounted(async () => {
  if (!entries.value.length) await library.load();
  await queue.load();
});

/**
 * vuedraggable owns this array and mutates it as you drag, so it has to be a
 * plain local copy. A computed would be rebuilt from unchanged store state on
 * the next render and snap the row back to where it started.
 */
const list = ref<QueueRow[]>([]);
watch(rows, (next) => (list.value = next.slice()), { immediate: true });

// A queue row carries its own copy of the entry. Without this, buying a game
// from the drawer would leave this page still calling it unowned.
watch(entries, (next) => queue.sync(next), { deep: true });

const subtitle = computed(() =>
  rows.value.length
    ? `${rows.value.length} game${rows.value.length === 1 ? "" : "s"} queued`
    : "What to play next, in the order you mean to play it",
);

async function onChange() {
  // vuedraggable has already rearranged `list`, so it is the authority on the
  // new order. One list means no partner event to race with, unlike the board.
  await queue.reorder(list.value.map((r) => r.entry.id));
}

function open(row: QueueRow) {
  library.select(row.entry.id === selectedId.value ? null : row.entry.id);
}
</script>

<template>
  <PageHeader title="Play Next" :subtitle="subtitle">
    <template #actions>
      <button
        type="button"
        class="flex items-center gap-1.5 rounded-lg bg-accent px-3 py-1.5 text-[13px] font-medium text-white transition-opacity hover:opacity-90"
        @click="adding = true"
      >
        <AppIcon name="plus" :size="15" />
        Add from library
      </button>
    </template>
  </PageHeader>

  <div class="flex min-h-0 flex-1">
    <div class="flex min-w-0 flex-1 flex-col">
      <p v-if="error" class="px-8 pt-4 text-[12.5px] text-danger">{{ error }}</p>

      <p v-if="loading && !rows.length" class="px-8 pt-6 text-[12.5px] text-ink-faint">
        Loading your list…
      </p>

      <EmptyState
        v-else-if="!rows.length"
        icon="queue"
        title="Nothing queued yet"
        body="Line up what to play next — anything from your library, owned or not. A finished game is welcome back on the list after a big update."
      >
        <button
          type="button"
          class="rounded-lg bg-accent px-3 py-1.5 text-[13px] font-medium text-white transition-opacity hover:opacity-90"
          @click="adding = true"
        >
          Add from library
        </button>
      </EmptyState>

      <!-- A container query, not a viewport one: opening the 420px drawer
           halves this pane while the window stays exactly as wide. -->
      <div v-else class="pane-scroll @container min-h-0 flex-1 px-6 py-4">
        <draggable
          v-model="list"
          :item-key="(row: QueueRow) => row.entry.id"
          class="flex flex-col gap-2.5"
          handle=".gt-grip"
          :animation="180"
          :scroll-sensitivity="90"
          :scroll-speed="14"
          ghost-class="gt-ghost"
          chosen-class="gt-chosen"
          fallback-class="gt-dragging"
          drag-class="gt-dragging"
          :force-fallback="true"
          :fallback-on-body="true"
          :fallback-tolerance="5"
          @change="onChange"
        >
          <template #item="{ element, index }">
            <PlayNextRow
              :row="element"
              :rank="index + 1"
              :selected="element.entry.id === selectedId"
              @open="open(element)"
              @remove="queue.remove(element.entry.id)"
            />
          </template>
        </draggable>
      </div>
    </div>

    <GameDetailDrawer v-if="selected" :entry="selected" @close="library.select(null)" />
  </div>

  <AddFromLibrary v-if="adding" @close="adding = false" />
</template>
