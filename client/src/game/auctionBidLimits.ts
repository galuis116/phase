/** Mirrors engine `auction::MAX_REPRESENTABLE_BID` (`i32::MAX` as `u32`). */
export const MAX_AUCTION_BID = 2_147_483_647;

/** Clamp a UI-entered bid to the engine-accepted signed life-loss range. */
export function clampAuctionBid(amount: number, min: number): number {
  const n = Number.isFinite(amount) ? amount : min;
  return Math.min(Math.max(n, min), MAX_AUCTION_BID);
}
