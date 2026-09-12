<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { storeToRefs } from "pinia";

import PageHeader from "@/components/PageHeader.vue";
import EmptyState from "@/components/EmptyState.vue";
import AppIcon from "@/components/AppIcon.vue";
import AddGameModal from "@/components/AddGameModal.vue";
import BoardColumn from "@/components/BoardColumn.vue";
import GameCard from "@/components/GameCard.vue";
import GameDetailDrawer from "@/components/GameDetailDrawer.vue";
import { BUCKETS, resolveChange, type Bucket, type DraggableChange } from "@/composables/buckets";
import { useLibraryStore } from "@/stores/library";
import type { LibraryEntry } from "@/types/models";

const library = useLibraryStore();
const { entries, loading, error, selected, selectedId } = storeToRefs(library);

const adding = ref(false);
const mode = ref<"board" | "grid">("board");
const filter = ref("");

onMounted(library.load);

const visible = computed(() => {
  const q = filter.value.trim().toLowerCase();
  if (!q) return entries.value;
  return entries.value.filter((e) => e.game.name.toLowerCase().includes(q));
});

/**
 * The board holds its own arrays rather than a computed, because vuedraggable
 * mutates them as you drag. A computed would be rebuilt from unchanged state
 * on the next render and snap the card back to where it started.
 */
const board = ref<{ bucket: Bucket; entries: LibraryEntry[] }[]>([]);

function rebuild() {
  board.value = BUCKETS.map((bucket) => ({
    bucket,
    entries: visible.value
      .filter(bucket.matches)
      .slice()
      .sort((a, b) => a.priority - b.priority),
  }));
}

// Rebuild when the library itself changes — load, add, delete, filter, or an
// edit made from the drawer. Drag-driven changes already mutated the arrays.
watch([visible, filter], rebuild, { immediate: true, deep: true });

async function onChange(bucket: Bucket, evt: DraggableChange) {
  const plan = resolveChange(board.value, bucket, evt);
  if (!plan) return;
  if (plan.patch) await library.patch(plan.patch.id, plan.patch.patch);
  await library.reorder(plan.order);
}

function open(entry: LibraryEntry) {
  library.select(entry.id === selectedId.value ? null : entry.id);
}
</script>

<template>
  <PageHeader
    title="Library"
    :subtitle="
      entries.length ? `${entries.length} game${entries.length === 1 ? '' : 's'} tracked`
                     : 'Everything you own, and everything you want'
    "
  >
    <template #actions>
      <input
        v-if="entries.length"
        v-model="filter"
        type="search"
        placeholder="Filter…"
        class="w-36 rounded-lg border border-line bg-surface px-2.5 py-1.5 text-[12.5px] outline-none transition-colors focus:border-line-solid placeholder:text-ink-faint"
      />

      <div v-if="entries.length" class="flex rounded-lg border border-line bg-surface p-0.5">
        <button
          v-for="m in (['board', 'grid'] as const)"
          :key="m"
          type="button"
          class="rounded-md px-2 py-1 text-[12px] capitalize transition-colors"
          :class="mode === m ? 'bg-elevated text-ink' : 'text-ink-faint hover:text-ink-dim'"
          @click="mode = m"
        >
          {{ m }}
        </button>
      </div>

      <button
        type="button"
        class="flex items-center gap-1.5 rounded-lg bg-accent px-3 py-1.5 text-[13px] font-medium text-white transition-opacity hover:opacity-90"
        @click="adding = true"
      >
        <AppIcon name="plus" :size="15" />
        Add game
      </button>
    </template>
  </PageHeader>

  <div class="flex min-h-0 flex-1">
    <div class="flex min-w-0 flex-1 flex-col">
      <p v-if="error" class="px-8 pt-4 text-[12.5px] text-danger">{{ error }}</p>

      <p v-if="loading && !entries.length" class="px-8 pt-6 text-[12.5px] text-ink-faint">
        Loading your library…
      </p>

      <EmptyState
        v-else-if="!entries.length"
        icon="library"
        title="No games yet"
        body="Search IGDB for anything you own or have your eye on. Wishlist items are the ones price tracking will watch."
      >
        <button
          type="button"
          class="rounded-lg bg-accent px-3 py-1.5 text-[13px] font-medium text-white transition-opacity hover:opacity-90"
          @click="adding = true"
        >
          Add your first game
        </button>
      </EmptyState>

      <!-- Columns keep a readable minimum and the board scrolls sideways instead;
           otherwise opening the drawer crushes the cards to unreadable slivers. -->
      <div
        v-else-if="mode === 'board'"
        class="flex min-h-0 flex-1 gap-3 overflow-x-auto px-6 py-4"
      >
        <BoardColumn
          v-for="col in board"
          :key="col.bucket.key"
          v-model="col.entries"
          :bucket="col.bucket"
          :selected-id="selectedId"
          @open="open"
          @change="onChange(col.bucket, $event)"
        />
      </div>

      <div v-else class="pane-scroll min-h-0 flex-1 px-8 py-5">
        <div class="grid grid-cols-[repeat(auto-fill,minmax(128px,1fr))] gap-3.5">
          <GameCard
            v-for="entry in visible"
            :key="entry.id"
            :entry="entry"
            :selected="entry.id === selectedId"
            @open="open(entry)"
          />
        </div>
        <p v-if="!visible.length" class="py-10 text-center text-[12.5px] text-ink-faint">
          Nothing matches “{{ filter }}”.
        </p>
      </div>
    </div>

    <GameDetailDrawer v-if="selected" :entry="selected" @close="library.select(null)" />
  </div>

  <AddGameModal v-if="adding" @close="adding = false" />
</template>

