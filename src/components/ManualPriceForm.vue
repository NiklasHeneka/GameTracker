<script setup lang="ts">
import { computed, ref } from "vue";

import { setManualPrice } from "@/api/prices";
import type { PriceOverview } from "@/types/models";

const props = defineProps<{ igdbId: number; currency: string }>();
const emit = defineEmits<{ saved: [overview: PriceOverview] }>();

/** Default shop per platform, so the common case needs no typing. */
const PLATFORMS = [
  { family: "playstation", label: "PlayStation", shop: "PlayStation Store" },
  { family: "xbox", label: "Xbox", shop: "Microsoft Store" },
  { family: "nintendo", label: "Nintendo", shop: "Nintendo eShop" },
  { family: "pc", label: "PC", shop: "" },
] as const;

const open = ref(false);
const family = ref<string>("playstation");
const shop = ref("");
const price = ref("");
const saving = ref(false);
const error = ref<string | null>(null);

const defaultShop = computed(
  () => PLATFORMS.find((p) => p.family === family.value)?.shop ?? "",
);
const effectiveShop = computed(() => shop.value.trim() || defaultShop.value);
const amount = computed(() => Number(price.value.replace(",", ".")));
const valid = computed(
  () => effectiveShop.value.length > 0 && Number.isFinite(amount.value) && amount.value >= 0,
);

async function save() {
  if (!valid.value) return;
  saving.value = true;
  error.value = null;
  try {
    emit(
      "saved",
      await setManualPrice(
        props.igdbId,
        family.value,
        effectiveShop.value,
        amount.value,
        props.currency,
      ),
    );
    price.value = "";
    shop.value = "";
    open.value = false;
  } catch (e) {
    error.value = (e as { message?: string }).message ?? String(e);
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <div class="mt-3">
    <button
      v-if="!open"
      type="button"
      class="text-[11.5px] text-ink-faint transition-colors hover:text-ink"
      @click="open = true"
    >
      + Add a price yourself
    </button>

    <div v-else class="rounded-xl border border-line bg-surface px-3 py-3">
      <p class="text-[12px] font-medium">Add a price</p>
      <p class="mt-1 text-[11px] leading-relaxed text-ink-faint">
        For stores with no usable price API. Yours is kept as you entered it and is never
        overwritten by an automatic refresh.
      </p>

      <div class="mt-2.5 grid grid-cols-2 gap-2">
        <label class="block">
          <span class="mb-1 block text-[11px] text-ink-faint">Platform</span>
          <select
            v-model="family"
            class="w-full rounded-lg border border-line bg-elevated px-2 py-1.5 text-[12px]"
          >
            <option v-for="p in PLATFORMS" :key="p.family" :value="p.family">{{ p.label }}</option>
          </select>
        </label>

        <label class="block">
          <span class="mb-1 block text-[11px] text-ink-faint">Price ({{ currency }})</span>
          <input
            v-model="price"
            type="text"
            inputmode="decimal"
            placeholder="29.99"
            class="w-full rounded-lg border border-line bg-elevated px-2 py-1.5 text-[12px]"
            @keydown.enter="save"
          />
        </label>
      </div>

      <label class="mt-2 block">
        <span class="mb-1 block text-[11px] text-ink-faint">Store</span>
        <input
          v-model="shop"
          type="text"
          :placeholder="defaultShop || 'Store name'"
          class="w-full rounded-lg border border-line bg-elevated px-2 py-1.5 text-[12px]"
          @keydown.enter="save"
        />
      </label>

      <p v-if="error" class="mt-2 text-[11px] text-danger">{{ error }}</p>

      <div class="mt-2.5 flex items-center gap-2">
        <button
          type="button"
          :disabled="!valid || saving"
          class="rounded-lg bg-accent px-2.5 py-1.5 text-[12px] font-medium text-white transition-opacity hover:opacity-90 disabled:opacity-40"
          @click="save"
        >
          {{ saving ? "Saving…" : "Save" }}
        </button>
        <button
          type="button"
          class="text-[12px] text-ink-faint transition-colors hover:text-ink"
          @click="open = false"
        >
          Cancel
        </button>
      </div>
    </div>
  </div>
</template>
