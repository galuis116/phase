import type {
  ContinuousModification,
  EffectRef,
  GameObject,
  Keyword,
  ObjectAttribution,
  ObjectId,
  TransientContinuousEffect,
} from "../adapter/types";
import { getKeywordName } from "./keywordProps";

/**
 * Resolves an `EffectRef` to the granted `ContinuousModification` plus the
 * display name of its source. Returns `null` when the referenced
 * static-definition slot or transient effect can't be found in the current
 * state (stale serialization, dead transient, etc.).
 *
 * Dereference is a pure lookup — no game logic. The engine writes
 * attribution at layer-application time so by the time the FE consumes it,
 * the references are valid against the *same* state snapshot the
 * attribution was computed from.
 */
export interface ResolvedAttribution {
  modification: ContinuousModification;
  sourceName: string;
  sourceId: ObjectId;
}

/**
 * The minimal state slice needed to resolve an `EffectRef`. Callers pass
 * narrowly-subscribed Zustand slices instead of the whole `GameState`,
 * which keeps `PermanentCard` re-renders bound to attribution-relevant
 * state changes only.
 */
export interface AttributionDeref {
  objects: Record<string, GameObject> | undefined;
  transientContinuousEffects: TransientContinuousEffect[] | undefined;
}

function resolveStatic(
  deref: AttributionDeref,
  source: ObjectId,
  defIndex: number,
  modIndex: number,
): ResolvedAttribution | null {
  const sourceObj = deref.objects?.[String(source)];
  if (!sourceObj) return null;
  const def = sourceObj.static_definitions[defIndex] as
    | { modifications?: ContinuousModification[] }
    | undefined;
  const mod = def?.modifications?.[modIndex];
  if (!mod) return null;
  return { modification: mod, sourceName: sourceObj.name, sourceId: source };
}

function resolveTransient(
  deref: AttributionDeref,
  id: number,
  modIndex: number,
): ResolvedAttribution | null {
  const tce = deref.transientContinuousEffects?.find((t) => t.id === id);
  if (!tce) return null;
  const mod = tce.modifications[modIndex];
  if (!mod) return null;
  return {
    modification: mod,
    sourceName: tce.source_name,
    sourceId: tce.source_id,
  };
}

export function resolveEffectRef(
  deref: AttributionDeref,
  ref: EffectRef,
): ResolvedAttribution | null {
  if (ref.type === "Transient") {
    return resolveTransient(deref, ref.data.id, ref.data.mod_index);
  }
  return resolveStatic(
    deref,
    ref.data.source,
    ref.data.def_index,
    ref.data.mod_index,
  );
}

/**
 * Builds a `keyword_name → source_name` map for one object by dereferencing
 * every `EffectRef` in its `Layer::Ability` bucket that grants a keyword.
 *
 * Self-grants (source === objectId) are filtered out because a creature
 * granting itself a keyword via its own static ability is functionally
 * "base" from the player's perspective; the engine emits them and the
 * display layer chooses to hide them per CR 113.3c intuition.
 *
 * Returns an empty map when the object has no attribution entries. Pass
 * `attribution` as `undefined` for the legacy-state case.
 */
export function buildGrantedKeywordSources(
  attribution: ObjectAttribution | undefined,
  objectId: ObjectId,
  deref: AttributionDeref,
): Map<string, string> {
  const result = new Map<string, string>();
  const abilityLayer = attribution?.by_layer?.Ability;
  if (!abilityLayer) return result;

  for (const ref of abilityLayer) {
    const resolved = resolveEffectRef(deref, ref);
    if (!resolved) continue;
    if (resolved.sourceId === objectId) continue; // hide self-grants
    const mod = resolved.modification;
    if (mod.type !== "AddKeyword") continue;
    const keyword = (mod as { type: "AddKeyword"; keyword: Keyword }).keyword;
    const name = getKeywordName(keyword);
    if (!result.has(name)) {
      result.set(name, resolved.sourceName);
    }
  }
  return result;
}
