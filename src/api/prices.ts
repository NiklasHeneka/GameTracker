import { call } from "./ipc";
import type { DealRow, PriceOverview, RefreshReport } from "@/types/models";

export const getPrices = (igdbId: number, force = false) =>
  call<PriceOverview>("get_prices", { igdbId, force });

export const refreshWishlistPrices = () => call<RefreshReport>("refresh_wishlist_prices");

/** Just the unowned games on Play Next, rather than the whole wishlist. */
export const refreshQueuePrices = () => call<RefreshReport>("refresh_queue_prices");

export const listActiveDeals = () => call<DealRow[]>("list_active_deals");

export const setManualPrice = (
  igdbId: number,
  platformFamily: string,
  shop: string,
  price: number,
  currency: string,
  url: string | null = null,
) => call<PriceOverview>("set_manual_price", { igdbId, platformFamily, shop, price, currency, url });
