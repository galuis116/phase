---
name: bug-coverage-classifier
description: Use when triaging a Phase bug report (Discord, GitHub issue, hallway report) to determine whether the misbehaving aspect of the card is in a known-unsupported clause (defer unless trivial), in a supposedly-supported clause (real defect — parser misparse or runtime bug), not pinned to a single card (cross-card/runtime/UI concern), or unclear. Pure-compute classifier; reuses the engine-authoritative parse_details tree the Alt-hover overlay and card-bot already consume. Output is advisory — never auto-applied as a label or routing decision without maintainer review.
---

# Bug Coverage Classifier

## When to use

- A user filed a bug report against a specific card.
- You need to know whether the bug is in a documented-as-unsupported clause (= known gap, defer) or in something we claim works (= defect, investigate).
- You want a structured verdict you can paste into a triage artifact, an issue comment, or your own notes.

Use directly via the CLI, or programmatically from another script (e.g. `scripts/sync-bug-reports.ts`) by importing `classify` from `scripts/classify-bug-coverage.ts`.

## What it does NOT do

- **Does not auto-apply GH labels.** Per the lesson from the existing `proposed_action` heuristic field, advisory classifier output must never drive side effects. The maintainer applies labels manually after eyeballing the verdict.
- **Does not write to disk or to GitHub.** Pure compute, single JSON object on stdout.
- **Does not distinguish parser misparse from runtime bug.** When a clause has `supported: true` and the bug points at it, the verdict is `supported_aspect_defect` — both classes route to maintainer investigation because `supported: true` is known-unreliable for that judgement (per project memory `project_backlog_is_parser_misparses.md`).

## Invocation

### CLI (one card per call)

```bash
bun scripts/classify-bug-coverage.ts \
  --card "Lightning Bolt" \
  --description "It deals 4 damage instead of 3" \
  [--fragment "deals 3 damage to any target"] \
  [--build preview|release]
```

### Stdin (for piped invocation)

```bash
echo '{"card_name":"Lightning Bolt","bug_description":"deals 4 instead of 3","oracle_text_fragment":"deals 3 damage to any target"}' \
  | bun scripts/classify-bug-coverage.ts --stdin
```

### Programmatic

```typescript
import { classify } from "./scripts/classify-bug-coverage";

const result = await classify({
  card_name: "Lightning Bolt",
  bug_description: "It deals 4 damage instead of 3",
  oracle_text_fragment: "deals 3 damage to any target",
  build: "preview",
});
```

## Output schema

Single JSON object on stdout. One of four verdicts:

| Verdict | Meaning | Triage action |
|---|---|---|
| `unsupported_aspect` | Matched clause has `supported: false`. Known coverage gap. | Defer unless trivial. The bug report is correct that the card misbehaves, but we already know. |
| `supported_aspect_defect` | Matched clause has `supported: true`. Bug is either parser misparse (AST wrong) or runtime bug (handler wrong). | Investigate. Higher priority — we promised this clause works. |
| `not_card_data_attributable` | Bug is not pinned to any single card clause (combat assignment, priority, AI, UI, multiplayer, etc.) OR the card-name entry is flagged `not_a_card` in `triage/unknown-card-mapping.json`. | Investigate outside `parse_details`. Not a parser/effect-handler concern. |
| `cannot_determine` | Bug description doesn't unambiguously map to any clause and doesn't smell like a runtime concern. Needs more info. | Ask the reporter for an Oracle text fragment, or eyeball it yourself. |

Each result includes:
- `confidence`: `high` / `medium` / `low`.
- `matched_clause`: `{ oracle_text_fragment, parse_details_path (RFC 6901), supported, label }` or `null`.
- `evidence`: one-paragraph rationale citing match reasons.
- `coverage_commit`: `BuildMeta.commit_short` from the build the verdict was computed against. Important: stamps that recorded an old coverage_commit should be re-classified if the engine has shipped coverage changes since (parser-velocity sprints flip `supported` flags routinely).

## Confidence threshold

Per plan: a `supported_aspect_defect` or `unsupported_aspect` verdict requires the matched node to score `medium` or `high` against the bug evidence. Scoring axes (composite):

1. **Oracle text fragment** fully or partially contained in a node's `source_text` (strongest signal — pass `--fragment` if the report quotes the misbehaving line).
2. **Keyword overlap** between `bug_description` tokens and node text (label + source_text).
3. **Numeric values** shared between bug description and node text (e.g. "deals 3" matches a node mentioning 3).

A bug description that triggers no node above threshold falls through to either `not_card_data_attributable` (matches one of the runtime/UI/multiplayer phrases) or `cannot_determine`.

## Card-name normalization

Before lookup, the classifier consults `triage/unknown-card-mapping.json` for known corrections (misspellings, MDFC half-names, dropped apostrophes). Cards flagged `not_a_card` in the mapping (e.g. token types) short-circuit to `not_card_data_attributable` with a self-documenting evidence line.

## What this reuses

- `scripts/card-bot/coverageData.ts` — `lookupCard()` + `getMeta()` for engine-authoritative `parse_details` + build provenance. Same data the Alt-hover overlay renders.
- `scripts/card-bot/config.ts` — `Build` type and `DEFAULT_BUILD` (preview by default).
- `triage/unknown-card-mapping.json` — card-name corrections curated by `bug-triage`.

## Caveats

- **Single card per invocation.** For multi-card bug reports ("X interacts wrong with Y"), call the classifier once per card and union the results — that's the consumer's responsibility, not the classifier's.
- **Node-matching is the load-bearing step.** The classifier can pick the wrong clause on a multi-clause card if `--fragment` is not provided and the bug description is vague. Always pass `--fragment` when the report quotes Oracle text.
- **`supported_aspect_defect` is intentionally ambiguous** between parser misparse and runtime bug. Distinguishing them requires reading the Rust source; that's downstream investigation work, not triage classification.
