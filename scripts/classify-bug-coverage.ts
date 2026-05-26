#!/usr/bin/env bun
// Classifier: given a card name + bug description, returns whether the
// misbehaving aspect is a known-unsupported clause, a defect in a supposedly-
// supported clause, not attributable to a single card (cross-card/runtime/UI),
// or undetermined.
//
// Reuses `scripts/card-bot/coverageData.ts` for the engine-authoritative
// parse_details tree (same data the client's Alt-hover overlay renders). The
// only new logic here is "match a bug description to a ParsedItem node and
// read its supported flag." Everything else — data source, R2 fetch, cache,
// per-node supported flag — is inherited from the existing module.
//
// Usage:
//   bun scripts/classify-bug-coverage.ts \
//     --card "Lightning Bolt" \
//     --description "It deals 4 damage instead of 3" \
//     [--fragment "deals 3 damage to any target"] \
//     [--build preview|release]
//
//   echo '{"card_name":"Lightning Bolt","bug_description":"..."}' | \
//     bun scripts/classify-bug-coverage.ts --stdin
//
// Output: single JSON object on stdout (see ClassifierResult type below).

import { readFileSync } from "node:fs";
import { join } from "node:path";

import {
  DEFAULT_BUILD,
  type Build,
  isBuild,
} from "./card-bot/config";
import {
  getMeta,
  lookupCard,
  type CoverageEntry,
  type ParsedItem,
} from "./card-bot/coverageData";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

type Verdict =
  | "unsupported_aspect"
  | "supported_aspect_defect"
  | "not_card_data_attributable"
  | "cannot_determine";

type Confidence = "high" | "medium" | "low";

interface ClassifierInput {
  card_name: string;
  bug_description: string;
  oracle_text_fragment?: string;
  build?: Build;
}

interface MatchedClause {
  oracle_text_fragment: string;
  /** RFC 6901 JSON Pointer into parse_details (e.g. "/0", "/0/children/2"). */
  parse_details_path: string;
  supported: boolean;
  label: string;
}

interface ClassifierResult {
  card_name: string;
  verdict: Verdict;
  confidence: Confidence;
  matched_clause: MatchedClause | null;
  evidence: string;
  coverage_commit: string | null;
}

// ---------------------------------------------------------------------------
// Card-name normalization via triage/unknown-card-mapping.json
// ---------------------------------------------------------------------------

interface UnknownCardCorrection {
  correct_name: string | null;
  status: string;
  notes?: string;
}

const REPO_ROOT = join(import.meta.dir, "..");
const UNKNOWN_CARD_MAPPING_PATH = join(REPO_ROOT, "triage", "unknown-card-mapping.json");

let unknownCardCache: Record<string, UnknownCardCorrection> | null = null;

function loadUnknownCardMapping(): Record<string, UnknownCardCorrection> {
  if (unknownCardCache) return unknownCardCache;
  try {
    const raw = readFileSync(UNKNOWN_CARD_MAPPING_PATH, "utf8");
    unknownCardCache = JSON.parse(raw) as Record<string, UnknownCardCorrection>;
  } catch {
    unknownCardCache = {};
  }
  return unknownCardCache;
}

/**
 * Applies the triage corpus's known name corrections. Returns `null` when the
 * mapping explicitly flags the input as `not_a_card` (e.g. a token type that
 * isn't a real card). Otherwise returns the corrected name, falling back to
 * the original input when no mapping exists.
 */
function normalizeCardName(input: string): string | null {
  const mapping = loadUnknownCardMapping();
  // The mapping is keyed by the original (possibly misspelled) name.
  const correction = mapping[input];
  if (correction) {
    if (correction.status === "not_a_card") return null;
    if (correction.correct_name) return correction.correct_name;
  }
  return input;
}

// ---------------------------------------------------------------------------
// Out-of-card-data detection
// ---------------------------------------------------------------------------

/**
 * Bug descriptions that reference these concepts are about engine/runtime/UI
 * concerns that have no direct ParsedItem to point at. We treat a strong
 * signal on this list — combined with the absence of any plausible node
 * match — as `not_card_data_attributable`.
 */
const OUT_OF_CARD_DATA_KEYWORDS: readonly string[] = [
  // Combat/turn structure
  "block assignment",
  "blocker assignment",
  "attacker order",
  "first strike step",
  "combat damage step",
  "priority pass",
  "apnap",
  // Stack/timing
  "trigger order",
  "stack order",
  "trigger ordering",
  // AI behavior
  "ai opponent",
  "ai plays",
  "ai picks",
  "ai miscounts",
  "ai concedes",
  "ai stalls",
  // Frontend / UI
  "button doesn't",
  "button does not",
  "card hover",
  "drag and drop",
  "ui glitch",
  "rendering",
  "animation",
  // Multiplayer / networking
  "disconnect",
  "reconnect",
  "lobby",
  "multiplayer sync",
  "websocket",
  "desync",
] as const;

function detectOutOfCardData(bugDescription: string): { match: boolean; matchedPhrase?: string } {
  const lower = bugDescription.toLowerCase();
  for (const phrase of OUT_OF_CARD_DATA_KEYWORDS) {
    if (lower.includes(phrase)) {
      return { match: true, matchedPhrase: phrase };
    }
  }
  return { match: false };
}

// ---------------------------------------------------------------------------
// Keyword extraction for fragment-less matching
// ---------------------------------------------------------------------------

/**
 * Stopwords that don't help disambiguate which clause a bug is about. We keep
 * gameplay verbs and effect/cost vocabulary in the candidate set.
 */
const STOPWORDS = new Set([
  "the", "a", "an", "is", "are", "was", "were", "be", "been", "being",
  "it", "its", "this", "that", "these", "those",
  "and", "or", "but", "if", "then", "else",
  "to", "from", "of", "in", "on", "at", "by", "for", "with",
  "i", "me", "my", "you", "your", "we", "us", "our", "they", "them",
  "have", "has", "had", "do", "does", "did", "will", "would", "should",
  "can", "could", "may", "might", "must",
  "card", "cards", "game", "games", "play", "played", "playing",
]);

function tokenize(text: string): string[] {
  return text
    .toLowerCase()
    .replace(/[^a-z0-9\s]/g, " ")
    .split(/\s+/)
    .filter(t => t.length >= 2 && !STOPWORDS.has(t));
}

// ---------------------------------------------------------------------------
// Node matching
// ---------------------------------------------------------------------------

interface NodeMatch {
  node: ParsedItem;
  path: string; // RFC 6901
  score: number;
  reasons: string[];
}

/** Walks parse_details and yields every node with its RFC-6901 path. */
function* walkNodes(
  items: ParsedItem[],
  basePath = "",
): Generator<{ node: ParsedItem; path: string }> {
  for (let i = 0; i < items.length; i++) {
    const path = `${basePath}/${i}`;
    yield { node: items[i], path };
    if (items[i].children?.length) {
      yield* walkNodes(items[i].children!, `${path}/children`);
    }
  }
}

/**
 * Scores a single node against the bug evidence. Higher = better match.
 * Returns 0 when no signal at all (skip from candidate set).
 */
function scoreNode(
  node: ParsedItem,
  bugDescription: string,
  oracleTextFragment: string | undefined,
  bugKeywords: Set<string>,
): { score: number; reasons: string[] } {
  const reasons: string[] = [];
  let score = 0;

  const nodeText = [node.label, node.source_text ?? ""].join(" ").toLowerCase();
  if (!nodeText.trim()) return { score: 0, reasons };

  // Strongest signal: explicit fragment substring match against source_text.
  if (oracleTextFragment) {
    const frag = oracleTextFragment.toLowerCase().trim();
    const src = (node.source_text ?? "").toLowerCase();
    if (frag && src.includes(frag)) {
      score += 10;
      reasons.push(`oracle_text_fragment fully contained in source_text`);
    } else if (frag && src && (src.includes(frag.slice(0, Math.min(frag.length, 40))) || frag.includes(src.slice(0, Math.min(src.length, 40))))) {
      // Partial overlap when neither fully contains the other.
      score += 4;
      reasons.push(`partial source_text overlap with oracle_text_fragment`);
    }
  }

  // Keyword overlap between bug description and node text.
  const nodeTokens = new Set(tokenize(nodeText));
  let keywordHits = 0;
  for (const kw of bugKeywords) {
    if (nodeTokens.has(kw)) keywordHits++;
  }
  if (keywordHits >= 3) {
    score += 5;
    reasons.push(`${keywordHits} bug-description keywords match node text`);
  } else if (keywordHits === 2) {
    score += 3;
    reasons.push(`2 bug-description keywords match node text`);
  } else if (keywordHits === 1) {
    score += 1;
    reasons.push(`1 bug-description keyword matches node text`);
  }

  // Numeric-value match (deals 3 / counts to 5 / sets life to N).
  const bugNumbers = (bugDescription.match(/\b\d+\b/g) ?? []).map(s => s);
  const nodeNumbers = (nodeText.match(/\b\d+\b/g) ?? []).map(s => s);
  const sharedNumbers = bugNumbers.filter(n => nodeNumbers.includes(n));
  if (sharedNumbers.length > 0) {
    score += 2;
    reasons.push(`shared numeric values: ${sharedNumbers.join(",")}`);
  }

  return { score, reasons };
}

function findBestMatch(
  entry: CoverageEntry,
  bugDescription: string,
  oracleTextFragment: string | undefined,
): NodeMatch | null {
  const bugKeywords = new Set(tokenize(bugDescription));
  let best: NodeMatch | null = null;
  for (const { node, path } of walkNodes(entry.parse_details)) {
    const { score, reasons } = scoreNode(node, bugDescription, oracleTextFragment, bugKeywords);
    if (score === 0) continue;
    if (!best || score > best.score) {
      best = { node, path, score, reasons };
    }
  }
  return best;
}

/**
 * Confidence threshold per plan: strong match requires at least 2 of 3 of
 * {effect verb hit, trigger condition hit, numeric value hit}. We approximate
 * with the composite score from `scoreNode`:
 *   ≥ 10 → high   (fragment fully contained, or 3+ keywords + numbers)
 *   ≥ 4  → medium (partial fragment overlap, or 2 keywords + numbers)
 *   < 4  → low    (one keyword, no fragment) — fails the threshold
 */
function confidenceFromScore(score: number): Confidence | null {
  if (score >= 10) return "high";
  if (score >= 4) return "medium";
  return null; // below threshold → cannot_determine
}

// ---------------------------------------------------------------------------
// Top-level classification
// ---------------------------------------------------------------------------

async function classify(input: ClassifierInput): Promise<ClassifierResult> {
  const build: Build = input.build ?? DEFAULT_BUILD;
  const coverageMeta = await getMeta(build);
  const coverage_commit = coverageMeta?.commit_short ?? null;

  // Normalize the card name via triage/unknown-card-mapping.json.
  const normalized = normalizeCardName(input.card_name);
  if (normalized === null) {
    return {
      card_name: input.card_name,
      verdict: "not_card_data_attributable",
      confidence: "high",
      matched_clause: null,
      evidence: `triage/unknown-card-mapping.json flags "${input.card_name}" as not_a_card (e.g. a token type, not a real card).`,
      coverage_commit,
    };
  }

  // Fetch the card's coverage entry.
  const entry = await lookupCard(build, normalized);
  if (!entry) {
    // Card not in coverage data even after correction. Could be a misspelling
    // beyond the mapping's coverage, OR a real card that the build doesn't
    // include. Either way, we can't classify.
    return {
      card_name: input.card_name,
      verdict: "cannot_determine",
      confidence: "low",
      matched_clause: null,
      evidence: `Card "${normalized}" not found in coverage data for build "${build}". Verify spelling or check triage/unknown-card-mapping.json.`,
      coverage_commit,
    };
  }

  // Find the best-matching node in the parse tree.
  const match = findBestMatch(entry, input.bug_description, input.oracle_text_fragment);
  const confidence = match ? confidenceFromScore(match.score) : null;

  if (match && confidence) {
    const supported = match.node.supported;
    return {
      card_name: entry.card_name,
      verdict: supported ? "supported_aspect_defect" : "unsupported_aspect",
      confidence,
      matched_clause: {
        oracle_text_fragment: match.node.source_text ?? "",
        parse_details_path: match.path,
        supported,
        label: match.node.label,
      },
      evidence: supported
        ? `Matched clause has supported: true. Bug is either a parser misparse (AST shape wrong) or a runtime resolution bug (handler wrong). The classifier does not distinguish — both route to maintainer investigation. Match reasons: ${match.reasons.join("; ")}.`
        : `Matched clause has supported: false. Known coverage gap; defer unless trivial. Match reasons: ${match.reasons.join("; ")}.`,
      coverage_commit,
    };
  }

  // No node matched above the confidence threshold. Check if the bug references
  // out-of-card-data concepts (combat, AI, UI, multiplayer, etc.).
  const oocd = detectOutOfCardData(input.bug_description);
  if (oocd.match) {
    return {
      card_name: entry.card_name,
      verdict: "not_card_data_attributable",
      confidence: "medium",
      matched_clause: null,
      evidence: `Bug description references "${oocd.matchedPhrase}" — an engine/runtime/UI concern not pinned to any single parsed clause. Investigate outside the card's parse_details.`,
      coverage_commit,
    };
  }

  // Bug description doesn't map to any clause and doesn't smell like a runtime
  // concern either. Need human review.
  return {
    card_name: entry.card_name,
    verdict: "cannot_determine",
    confidence: "low",
    matched_clause: null,
    evidence: `Bug description did not unambiguously match any parsed clause (best score ${match?.score ?? 0}). Provide an oracle_text_fragment quoting the specific Oracle line that misbehaves, or describe the bug with more clause-specific vocabulary.`,
    coverage_commit,
  };
}

// ---------------------------------------------------------------------------
// CLI entry point
// ---------------------------------------------------------------------------

function parseArgs(argv: string[]): ClassifierInput | { stdin: true; build?: Build } {
  const args = new Map<string, string>();
  let stdin = false;
  for (let i = 2; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--stdin") {
      stdin = true;
      continue;
    }
    if (arg.startsWith("--")) {
      const key = arg.slice(2);
      const value = argv[i + 1];
      if (value === undefined || value.startsWith("--")) {
        args.set(key, "");
      } else {
        args.set(key, value);
        i++;
      }
    }
  }

  const buildArg = args.get("build");
  const build: Build | undefined = buildArg && isBuild(buildArg) ? buildArg : undefined;

  if (stdin) {
    return { stdin: true, build };
  }

  const card_name = args.get("card");
  const bug_description = args.get("description");
  if (!card_name || !bug_description) {
    throw new Error(
      "Usage: --card <name> --description <text> [--fragment <text>] [--build preview|release]\n" +
      "   or: --stdin (read JSON from stdin)"
    );
  }

  return {
    card_name,
    bug_description,
    oracle_text_fragment: args.get("fragment") || undefined,
    build,
  };
}

async function readStdin(): Promise<string> {
  const chunks: Buffer[] = [];
  for await (const chunk of process.stdin) {
    chunks.push(chunk as Buffer);
  }
  return Buffer.concat(chunks).toString("utf8");
}

async function main(): Promise<void> {
  const parsed = parseArgs(process.argv);
  let input: ClassifierInput;
  if ("stdin" in parsed) {
    const raw = await readStdin();
    const json = JSON.parse(raw) as ClassifierInput;
    input = { ...json, build: parsed.build ?? json.build };
  } else {
    input = parsed;
  }
  const result = await classify(input);
  process.stdout.write(JSON.stringify(result, null, 2) + "\n");
}

// Programmatic export so bug-triage's sync-bug-reports.ts can import directly
// instead of shelling out.
export { classify, type ClassifierInput, type ClassifierResult, type Verdict };

if (import.meta.main) {
  main().catch(err => {
    process.stderr.write(`Error: ${err instanceof Error ? err.message : String(err)}\n`);
    process.exit(1);
  });
}
