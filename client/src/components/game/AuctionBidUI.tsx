import { AnimatePresence, motion } from "framer-motion";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { useCanActForWaitingState } from "../../hooks/usePlayerId.ts";
import { useGameStore } from "../../stores/gameStore.ts";
import { gameButtonClass } from "../ui/buttonStyles.ts";

/**
 * Overlay for the `WaitingFor::AuctionBid` state.
 *
 * CR 119.3 + CR 101.4: Open-bid life auction (Illicit Auction, Pain's Reward,
 * Mages' Contest). The acting player either tops the current high bid or passes.
 * A bid `amount > current_high_bid` tops; passing dispatches
 * `SubmitBid { amount: current_high_bid }` (any value `<= current_high_bid` is a
 * pass). During the player-chosen opening phase (Pain's Reward) the first bid
 * sets the opening high bid, so the minimum is 0 and a "pass" of 0 is a legal
 * opening bid of zero.
 *
 * Pure display layer: it never bids on the player's behalf. Bids may exceed
 * the bidder's life total — life is lost only when the auction settles.
 */
export function AuctionBidUI() {
  const { t } = useTranslation("game");
  const waitingFor = useGameStore((s) => s.waitingFor);
  const gameState = useGameStore((s) => s.gameState);
  const dispatch = useGameStore((s) => s.dispatch);
  const canAct = useCanActForWaitingState();

  const isAuction = waitingFor?.type === "AuctionBid";
  const data = isAuction ? waitingFor.data : null;
  const currentHighBid = data?.current_high_bid ?? 0;
  // The opening phase (Pain's Reward) is exactly when no high bidder is set yet.
  const openingPhase = data ? data.high_bidder === null : false;

  // CR 119.3: A topping bid must strictly exceed the current high bid. During
  // the opening phase the minimum is 0 ("a bid of any number").
  const minBid = openingPhase ? 0 : currentHighBid + 1;

  const [value, setValue] = useState(minBid);

  useEffect(() => {
    if (isAuction) setValue(minBid);
  }, [isAuction, minBid]);

  const sourceName = useMemo(() => {
    if (!gameState || !data) return null;
    return gameState.objects[data.source_id]?.name ?? null;
  }, [gameState, data]);

  const handleBid = useCallback(() => {
    const amount = Math.max(value, minBid);
    dispatch({ type: "SubmitBid", data: { amount } });
  }, [dispatch, value, minBid]);

  const handlePass = useCallback(() => {
    // CR 119.3: A pass is any bid that does not top the high bid. During the
    // opening phase, the player must still set an opening — pass of 0.
    dispatch({
      type: "SubmitBid",
      data: { amount: openingPhase ? 0 : currentHighBid },
    });
  }, [dispatch, openingPhase, currentHighBid]);

  if (!data || !canAct) return null;

  return (
    <AnimatePresence>
      <motion.div
        className="pointer-events-none fixed inset-x-0 bottom-0 z-40 flex justify-center pb-4"
        initial={{ y: 80, opacity: 0 }}
        animate={{ y: 0, opacity: 1 }}
        exit={{ y: 80, opacity: 0 }}
        transition={{ duration: 0.25 }}
      >
        <div className="pointer-events-auto min-w-[320px] max-w-[420px] rounded-xl bg-gray-900/95 p-4 shadow-2xl ring-1 ring-gray-700">
          <h3 className="mb-3 text-center text-sm font-semibold text-gray-300">
            {t("auctionBid.title")}
            {sourceName && (
              <span className="ml-1 text-gray-400">&mdash; {sourceName}</span>
            )}
          </h3>

          <p className="mb-3 text-center text-xs text-gray-400">
            {openingPhase
              ? t("auctionBid.openingPrompt")
              : t("auctionBid.highBid", { bid: currentHighBid })}
          </p>

          <div className="mb-4 px-2">
            <label className="flex items-center gap-3 text-sm text-gray-200">
              <span className="shrink-0 font-mono text-base text-cyan-300">
                {t("auctionBid.bidEquals", { value })}
              </span>
              <input
                type="number"
                min={minBid}
                value={value}
                onChange={(e) =>
                  setValue(Math.max(minBid, Number(e.target.value) || minBid))
                }
                className="w-full rounded-lg border border-gray-600 bg-gray-800 px-3 py-1.5 font-mono text-base text-cyan-300"
                aria-label={t("auctionBid.bidAria")}
              />
              <span className="shrink-0 text-xs text-gray-500">
                {t("auctionBid.minBid", { min: minBid })}
              </span>
            </label>
          </div>

          <div className="flex justify-center gap-3">
            <button
              onClick={handleBid}
              className={gameButtonClass({ tone: "emerald", size: "md" })}
            >
              {t("auctionBid.submitBid", { value })}
            </button>
            <button
              onClick={handlePass}
              className="rounded-lg bg-gray-700 px-4 py-1.5 text-sm font-semibold text-gray-200 transition hover:bg-gray-600"
            >
              {t("auctionBid.pass")}
            </button>
          </div>
        </div>
      </motion.div>
    </AnimatePresence>
  );
}
