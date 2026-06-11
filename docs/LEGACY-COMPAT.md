# Legacy compatibility tripwires

This file indexes every deliberate legacy-compat shim in the engine: where the
shim lives, why it exists, and what deadline forces its removal. Each entry has
a grep token so an audit pass can find every site without manual recall.

Add a new entry whenever you introduce a `serde(alias)`, a `deserialize_with`
that handles a legacy on-disk shape, or any "accept both old and new" code path.
Removing a shim means deleting both the entry here AND the corresponding
tripwire const in the source.

## Format

Each entry MUST include:

- **Grep token** — a unique `LEGACY_*` constant name that appears verbatim both
  in the source tripwire const and in this index.
- **What it covers** — the on-disk shape being accepted in addition to the
  current one.
- **Added in** — workspace version when the shim landed.
- **Removal trigger** — the version boundary at which the tripwire `assert!`
  fires (usually `+14` patch releases as a soft window for downstream catch-up).
- **Source** — file:line of the tripwire const.

## Active shims

_None currently._

## Removed shims

### `LEGACY_DESER_ETB_CONTROLLER_2026Q2` — REMOVED in v0.1.54

Covered the pre-2026-Q2 `under_your_control: bool` shape, lifted to typed
shapes at three layers per CR 110.2a: `Effect::ChangeZone.enters_under`
(`Option<ControllerRef>`), and the `enters_under_player` (`Option<PlayerId>`)
runtime carriers on `PendingChangeZoneIteration` and
`WaitingFor::EffectZoneChoice`.

- **Added in:** 0.1.39 (AST lift, CR 110.2a).
- **Removal trigger:** workspace version > 0.1.53 — fired at the v0.1.54
  release. Per the removal procedure, both compat deserializers
  (`deserialize_enters_under_compat`, `deserialize_enters_under_player_compat`),
  the three `#[serde(alias = "under_your_control", deserialize_with = ...)]`
  attributes, and the tripwire const were deleted. Emission was always the
  modern shape, so on-disk payloads written by ≥0.1.39 clients are unaffected;
  only pre-0.1.39 boolean payloads lose their (best-effort) compat.
