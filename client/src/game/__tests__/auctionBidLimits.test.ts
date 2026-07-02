import { describe, expect, it } from "vitest";

import {
  clampAuctionBid,
  MAX_AUCTION_BID,
} from "../auctionBidLimits";

describe("clampAuctionBid", () => {
  it("clamps below-min bids up to the minimum", () => {
    expect(clampAuctionBid(0, 4)).toBe(4);
  });

  it("clamps above-max bids down to the engine limit", () => {
    expect(clampAuctionBid(Number.MAX_SAFE_INTEGER, 1)).toBe(MAX_AUCTION_BID);
  });

  it("passes through legal bids unchanged", () => {
    expect(clampAuctionBid(7, 4)).toBe(7);
  });
});
