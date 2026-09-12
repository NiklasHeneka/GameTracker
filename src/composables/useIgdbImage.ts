export type IgdbSize =
  | "thumb"
  | "cover_small"
  | "cover_big"
  | "screenshot_med"
  | "screenshot_big"
  | "720p"
  | "1080p";

/**
 * IGDB serves images straight from its CDN, which the app's CSP allows.
 * `_2x` is the retina variant — worth it for covers, which are the entire
 * visual weight of the library.
 */
export function igdbImage(
  imageId: string | null | undefined,
  size: IgdbSize = "cover_big",
  retina = true,
): string | null {
  if (!imageId) return null;
  return `https://images.igdb.com/igdb/image/upload/t_${size}${retina ? "_2x" : ""}/${imageId}.jpg`;
}

export function releaseYear(unixSeconds: number | null | undefined): number | null {
  if (unixSeconds === null || unixSeconds === undefined) return null;
  return new Date(unixSeconds * 1000).getUTCFullYear();
}
