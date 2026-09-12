/** Format a price in the currency the source actually quoted. */
export function money(amount: number, currency: string): string {
  try {
    return new Intl.NumberFormat(undefined, {
      style: "currency",
      currency,
      minimumFractionDigits: 2,
    }).format(amount);
  } catch {
    // An unrecognised currency code must not blank the price.
    return `${amount.toFixed(2)} ${currency}`;
  }
}

export function shortDate(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
    year: "numeric",
  });
}

/** "3d 4h", "5h 20m", "12m" — how long until `unixSeconds`. */
export function until(unixSeconds: number, from = Date.now()): string | null {
  const seconds = unixSeconds - Math.floor(from / 1000);
  if (seconds <= 0) return null;

  const d = Math.floor(seconds / 86400);
  const h = Math.floor((seconds % 86400) / 3600);
  const m = Math.floor((seconds % 3600) / 60);

  if (d > 0) return h > 0 ? `${d}d ${h}h` : `${d}d`;
  if (h > 0) return m > 0 ? `${h}h ${m}m` : `${h}h`;
  return `${m}m`;
}

export function daysAgo(unixSeconds: number, from = Date.now()): number {
  return Math.floor((Math.floor(from / 1000) - unixSeconds) / 86400);
}

/**
 * Shorter store labels for the narrow deals panel, where the full names
 * truncate to things like "Epic Game S…".
 */
const SHOP_LABELS: Record<string, string> = {
  "Epic Game Store": "Epic Games",
  "Epic Games Store": "Epic Games",
  "Microsoft Store": "Microsoft",
  "PlayStation Store": "PlayStation",
  "Humble Store": "Humble",
};

export function shopLabel(shop: string): string {
  return SHOP_LABELS[shop] ?? shop;
}
