import { call } from "./ipc";
import type { QueueRow } from "@/types/models";

export const listQueue = () => call<QueueRow[]>("list_queue");

export const enqueue = (entryId: number) => call<QueueRow>("enqueue", { entryId });

export const dequeue = (entryId: number) => call<void>("dequeue", { entryId });

/** `entryIds` is the whole list in its new order, front to back. */
export const reorderQueue = (entryIds: number[]) => call<void>("reorder_queue", { ids: entryIds });
