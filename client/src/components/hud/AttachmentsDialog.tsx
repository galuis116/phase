import { type CSSProperties } from "react";

import type { ObjectId, PlayerId } from "../../adapter/types.ts";
import { useGameStore } from "../../stores/gameStore.ts";
import { getPlayerDisplayName, useMultiplayerStore } from "../../stores/multiplayerStore.ts";
import { DialogShell } from "../modal/DialogShell.tsx";
import { DialogAttachmentCard } from "./DialogAttachmentCard.tsx";

/** What's enchanted/equipped/fortified — the thing on the left of the dialog.
 *  A discriminated union so the dialog renders the appropriate visual without
 *  callers passing pre-built JSX (which would scatter "how do we depict a
 *  host" across every consumer). The two arms cover every CR 301/303
 *  attachment shape: Equipment/Aura/Fortification on a permanent (`object`),
 *  and Aura on a player (`player`). */
export type AttachmentHost =
  | { type: "player"; playerId: PlayerId }
  | { type: "object"; objectId: ObjectId };

interface Props {
  isOpen: boolean;
  onClose: () => void;
  host: AttachmentHost;
  attachmentIds: readonly ObjectId[];
}

// Dialog cards render the full Scryfall image at a size where oracle text is
// readable without zooming. 220px wide hits the readability threshold; the
// host card is smaller (140px) since the player avatar is identity, not text.
const ATTACHMENT_W_PX = 220;
const HOST_W_PX = 140;

const HOST_SIZE_VARS: CSSProperties = {
  "--card-w": `${HOST_W_PX}px`,
  "--card-h": `${Math.round(HOST_W_PX * 1.4)}px`,
} as CSSProperties;

/**
 * Modal that shows a host (creature, planeswalker, battle, or player) on the
 * left and every permanent attached to it on the right. Each attachment is a
 * full Scryfall card via `<DialogAttachmentCard>` (not the battlefield-
 * compact `<PermanentCard>`) so the player can actually read what's
 * enchanting them — the dialog's primary purpose.
 *
 * `<DialogAttachmentCard>` preserves the two interactions that matter for
 * Auras: click-to-target (when the engine is asking for an enchantment
 * target) and click-to-activate (rare on Auras but exists). Counters render
 * as overlay badges in the top-right.
 *
 * Used for player-attached Aura clusters today (Curse of Opulence, Faith's
 * Fetters, etc.) and structured to take object hosts as well, so
 * PermanentCard's "host with N attachments" stack can adopt the same dialog
 * for a less-cluttered N>=2 affordance.
 */
export function AttachmentsDialog({ isOpen, onClose, host, attachmentIds }: Props) {
  const hostName = useHostName(host);

  if (!isOpen) return null;

  const eyebrow = host.type === "player" ? "Enchantments on Player" : "Attached to";
  const title = hostName;
  const subtitle =
    attachmentIds.length === 1
      ? "1 attached permanent"
      : `${attachmentIds.length} attached permanents`;

  return (
    <DialogShell
      eyebrow={eyebrow}
      title={title}
      subtitle={subtitle}
      size="lg"
      scrollable
      onClose={onClose}
    >
      <div className="flex items-start gap-4 px-4 py-4 lg:px-6 lg:py-5">
        <div className="shrink-0" style={HOST_SIZE_VARS}>
          <HostVisual host={host} />
        </div>
        <div aria-hidden className="w-px self-stretch bg-white/10" />
        {/* `min-w-0` lets the flex-1 child shrink below its content's
            intrinsic width so wrapping engages cleanly when the dialog is
            constrained. Cards wrap to a new row when there isn't horizontal
            room — `scrollable` on the DialogShell handles the vertical
            overflow if the row count grows. */}
        <div className="flex min-w-0 flex-1 flex-wrap content-start gap-3">
          {attachmentIds.map((id) => (
            <DialogAttachmentCard key={id} objectId={id} widthPx={ATTACHMENT_W_PX} />
          ))}
        </div>
      </div>
    </DialogShell>
  );
}

function HostVisual({ host }: { host: AttachmentHost }) {
  if (host.type === "object") {
    // Object hosts (creatures/planeswalkers/battles with Auras attached) get
    // the same full-card readable treatment. Reuses DialogAttachmentCard
    // with no extra interaction wiring beyond what the host normally exposes
    // — the user clicked the dialog to read about the host, after all.
    return <DialogAttachmentCard objectId={host.objectId} widthPx={HOST_W_PX} />;
  }
  return <PlayerHostCard playerId={host.playerId} />;
}

/**
 * Card-shaped visual for a player host. Uses the planeswalker avatar art
 * (when available) as the "art" face and the player's life total as the
 * recognizable identifier — mirroring the player's HUD plate so the dialog
 * reads "this Aura → enchants → that player" at a glance.
 */
function PlayerHostCard({ playerId }: { playerId: PlayerId }) {
  const avatarUrl = useMultiplayerStore((s) => s.playerAvatars.get(playerId) ?? null);
  const life = useGameStore((s) => s.gameState?.players[playerId]?.life ?? 0);
  const name = getPlayerDisplayName(playerId, playerId);

  return (
    <div className="relative flex h-[var(--card-h)] w-[var(--card-w)] flex-col overflow-hidden rounded-lg border border-white/15 bg-slate-950 shadow-lg">
      {avatarUrl ? (
        <img
          src={avatarUrl}
          alt={name}
          className="h-full w-full object-cover"
          draggable={false}
        />
      ) : (
        <div className="flex flex-1 items-center justify-center bg-gradient-to-b from-slate-800 to-slate-950 text-3xl">
          🧙
        </div>
      )}
      <div className="absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/85 to-transparent px-2 pb-1.5 pt-3">
        <div className="truncate text-[10px] font-semibold uppercase tracking-[0.18em] text-slate-100">
          {name}
        </div>
        <div className="text-lg font-bold text-rose-200 tabular-nums">{life}</div>
      </div>
    </div>
  );
}

function useHostName(host: AttachmentHost): string {
  const objectName = useGameStore((s) =>
    host.type === "object" ? s.gameState?.objects[host.objectId]?.name ?? "Unknown" : "",
  );
  if (host.type === "object") return objectName;
  return getPlayerDisplayName(host.playerId, host.playerId);
}
