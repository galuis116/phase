import { beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";

import type { WaitingFor } from "../../adapter/types";
import { MAX_AUCTION_BID } from "../../game/auctionBidLimits";
import { useGameStore } from "../../stores/gameStore";
import { AuctionBidUI } from "../AuctionBidUI";

function auctionWaitingFor(
  overrides: Partial<Extract<WaitingFor, { type: "AuctionBid" }>["data"]> = {},
): WaitingFor {
  return {
    type: "AuctionBid",
    data: {
      player: 0,
      current_high_bid: 3,
      high_bidder: 1,
      eligible: [0, 1],
      remaining_in_round: [],
      passes_in_a_row: 0,
      winner_effect: {},
      target: null,
      controller: 1,
      source_id: 42,
      ...overrides,
    },
  };
}

describe("AuctionBidUI", () => {
  const dispatch = vi.fn();

  beforeEach(() => {
    cleanup();
    dispatch.mockReset();
    useGameStore.setState({
      waitingFor: auctionWaitingFor({ player: 0 }),
      gameState: {
        objects: {
          42: { id: 42, name: "Illicit Auction" },
        },
      } as never,
      dispatch,
    });
  });

  it("renders nothing when the viewer cannot act", () => {
    useGameStore.setState({
      waitingFor: auctionWaitingFor({ player: 1 }),
    });
    const { container } = render(<AuctionBidUI />);
    expect(container).toBeEmptyDOMElement();
  });

  it("dispatches a topping bid clamped to the engine maximum", () => {
    render(<AuctionBidUI />);

    const input = screen.getByRole("spinbutton", { name: /choose a life amount/i });
    fireEvent.change(input, { target: { value: String(Number.MAX_SAFE_INTEGER) } });
    fireEvent.click(screen.getByRole("button", { name: /bid/i }));

    expect(dispatch).toHaveBeenCalledWith({
      type: "SubmitBid",
      data: { amount: MAX_AUCTION_BID },
    });
  });

  it("dispatches a pass at the current high bid during topping", () => {
    render(<AuctionBidUI />);
    fireEvent.click(screen.getByRole("button", { name: /^pass$/i }));

    expect(dispatch).toHaveBeenCalledWith({
      type: "SubmitBid",
      data: { amount: 3 },
    });
  });

  it("dispatches opening bid zero during the player-chosen opening phase", () => {
    useGameStore.setState({
      waitingFor: auctionWaitingFor({
        player: 0,
        current_high_bid: 0,
        high_bidder: null,
      }),
    });

    render(<AuctionBidUI />);
    fireEvent.click(screen.getByRole("button", { name: /^pass$/i }));

    expect(dispatch).toHaveBeenCalledWith({
      type: "SubmitBid",
      data: { amount: 0 },
    });
  });
});
