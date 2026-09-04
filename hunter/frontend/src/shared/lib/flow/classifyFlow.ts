/** Client-side PREVIEW of `hunter_engine::metrics::flow_ix::FlowState`
 *  (Rust SSOT: hunter/engine/src/metrics/flow_ix.rs:305-386) — used by
 *  token charts (Flow Discovery, Simulate, inspect) to redraw vol/non-vol
 *  lines without a backend round-trip. Visualization only — never wired to
 *  live trading decisions. Equivalence classes match via `JSON.stringify`
 *  of ordered `ix_labels` arrays (or `templateGrain` under {@link FlowMatchMode}
 *  `'grain'`).
 *
 *  Mirrors the Rust classify order: creator wallet → always volume; wallet
 *  already tagged (forward-only contagion) → volume; else structural
 *  `ix_labels` match against the pattern set; else organic.
 *
 *  Dump / working overlays reuse this fold with contagion off (those groups
 *  have no wallet rule). The structural test then IS the verdict. */

import { anyRowMatchesTrade, type IxPatternRow } from 'lib/strategy/ixPatternRows';
import { workingListHits } from 'lib/strategy/templateGrain';

export interface FlowTradeLite {
  wallet_address: string;
  /** Signed by convention of the caller — this module only reads magnitude. */
  sol: number;
  ix_labels: readonly string[] | null | undefined;
  /** Which leg this is. Required only under a {@link FlowClassifyOptions.side}
   *  narrowing; absent there, the trade cannot prove it is on the asked side and
   *  is treated as off-side. */
  side?: FlowSide | null;
  /** Fee budget this tx compiled — used when {@link FlowClassifyOptions.patternRows}
   *  carries pinned rows. Absent fields are wildcards on the row side. */
  cu_limit?: number | null;
  cu_price?: number | null;
  tip_lamports?: number | null;
}

/** One leg of a trade. `ix_labels` do NOT encode this — an aggregator's launch
 *  structure is byte-identical on the way in and the way out — so a side read is
 *  a filter over trades, never over patterns. */
export type FlowSide = 'buy' | 'sell';

/** How {@link FlowClassifyOptions.patternKeys} are compared to a trade.
 *
 *  `'labels'` (default) is `m_flow_ix` / `m_dump_ix`: exact ordered `ix_labels`.
 *  `'grain'` is `m_burst_slot.working_templates`: grain or program name. */
export type FlowMatchMode = 'labels' | 'grain';

export interface FlowClassifyOptions {
  /** Membership set. Under `'labels'`, `JSON.stringify(ix_labels)`; under
   *  `'grain'`, `templateGrain(ix_labels)`. */
  patternKeys: ReadonlySet<string>;
  /** When set (tagged/dump), structural match uses engine row matching — an
   *  unpinned row is a fee wildcard — instead of `patternKeys.has`. Empty /
   *  omitted falls back to the key set. Ignored under `'grain'`. */
  patternRows?: readonly IxPatternRow[] | null;
  /** Default `'labels'`. */
  match?: FlowMatchMode;
  /** Token creator wallet address — always classified as volume-side, and
   *  seeds the contagion set (mirrors `FlowState::set_creator`). */
  creatorWallet?: string | null;
  /**
   * Forward-only wallet tagging, the Rust classifier's behavior — default `true`.
   *
   * Set `false` for a STRUCTURAL-ONLY read: every trade is judged by its own
   * `ix_labels` alone, and neither a match nor the creator wallet taints the
   * wallet's later trades. Contagion answers "who is in the volume crew"; with
   * it on, one match makes a wallet volume-side forever, which smears a study of
   * "which STRUCTURES are around this moment" into a single wallet set within
   * seconds. Analysis-only surfaces (the Trader Analysis flow lens) therefore
   * default it off — the engine's own classification is never computed here.
   */
  contagion?: boolean;
  /**
   * Narrow classification to ONE leg — `'buy'`, `'sell'`, or `null`/absent for
   * both (the engine's behavior).
   *
   * A pattern key is an ordered `ix_labels` sequence, and those labels carry no
   * direction: the same aggregator structure matches a buy and the sell that
   * unwinds it, so an unnarrowed lens sums two opposite events onto one line.
   * The two readings are different theses — a matched structure BUYING just
   * before a trade is a crowd impulse joined, the same structure SELLING is exit
   * liquidity absorbed — and mixed they partially cancel.
   *
   * Off-side trades classify non-volume and never tag a wallet, so a narrowed
   * lens answers only about the leg asked for.
   */
  side?: FlowSide | null;
  /** Wallets that can never be volume-side and never tag anything — the studied
   *  trader itself, typically, so a lens does not classify its own subject. */
  excludeWallets?: ReadonlySet<string> | null;
}

/**
 * WHY a trade counts as volume-side. The per-trade Tagged badge tests structure
 * alone, but the chart's lines apply contagion on top — so a row can read
 * "Non-vol" while its SOL sits on the vol line. Naming the mechanism is the only
 * way a pattern edit and the line it moves stay legible to each other.
 *
 *  - `structural` — its ordered `ix_labels` match a staged pattern.
 *  - `creator`    — the token creator's own wallet (seeds the contagion set).
 *  - `wallet`     — a wallet already tagged by an earlier trade, whatever this
 *                   trade's own structure looks like.
 */
export type FlowReason = 'structural' | 'creator' | 'wallet';

export interface FlowClassified {
  isTagged: boolean;
  /** `null` ⇔ `isTagged === false`. */
  reason: FlowReason | null;
  taggedSol: number;
  untaggedSol: number;
}

function isStructuralMatch(t: FlowTradeLite, opts: FlowClassifyOptions): boolean {
  const labels = t.ix_labels;
  if (!labels || labels.length === 0) return false;
  if (opts.match === 'grain') return workingListHits(opts.patternKeys, labels);
  const rows = opts.patternRows;
  if (rows && rows.length > 0) return anyRowMatchesTrade(rows, labels, t);
  return opts.patternKeys.has(JSON.stringify(labels));
}

/** Classify `trades` (must already be in canonical order — slot -> tx_index
 *  -> leg_index) into vol/non-vol, forward-tagging wallets as they're seen. */
export function classifyFlowTrades<T extends FlowTradeLite>(
  trades: readonly T[],
  opts: FlowClassifyOptions,
): (T & FlowClassified)[] {
  const contagion = opts.contagion !== false;
  const excluded = opts.excludeWallets;
  const side = opts.side ?? null;
  const taggedWallets = new Set<string>();
  // The creator seeds contagion, so it is only a tag when contagion is on. With
  // it off, the creator's trades are judged by their structure like everyone
  // else's — otherwise "structural only" would still carry one wallet rule.
  if (contagion && opts.creatorWallet) taggedWallets.add(opts.creatorWallet);

  const out: (T & FlowClassified)[] = [];
  for (const t of trades) {
    // Off-side and excluded are the same verdict: non-volume, and no tagging —
    // a trade the lens is not asking about must not seed contagion either.
    if (excluded?.has(t.wallet_address) || (side !== null && t.side !== side)) {
      const mag = Math.abs(t.sol);
      out.push({ ...t, isTagged: false, reason: null, taggedSol: 0, untaggedSol: mag });
      continue;
    }
    const structuralMatch = isStructuralMatch(t, opts);
    // Read the contagion set BEFORE this trade can join it, so a wallet's first
    // structural match is reported as `structural` and only its later trades as
    // `wallet` — otherwise every trade of a tagged wallet looks like contagion
    // and nothing points back at the pattern that started it.
    const wasTagged = contagion && taggedWallets.has(t.wallet_address);
    const isTagged = wasTagged || structuralMatch;
    const reason: FlowReason | null = wasTagged
      ? t.wallet_address === opts.creatorWallet
        ? 'creator'
        : 'wallet'
      : structuralMatch
        ? 'structural'
        : null;
    if (contagion && isTagged) taggedWallets.add(t.wallet_address);
    const g = Math.abs(t.sol);
    out.push({ ...t, isTagged, reason, taggedSol: isTagged ? g : 0, untaggedSol: isTagged ? 0 : g });
  }
  return out;
}

/** A trade carrying the identity the trades table keys rows on. */
export interface FlowTradeIdentified extends FlowTradeLite {
  id: string;
}

/**
 * Effective (contagion-aware) classification, keyed by trade id — what the
 * chart's lines actually did with each trade, for a table that can only see one
 * candle's worth of rows and so cannot recompute contagion itself.
 *
 * `trades` must be the token's FULL history in canonical order: contagion is
 * forward-only, so classifying a slice would miss the earlier trade that tagged
 * the wallet. Non-volume trades are omitted from the map — absent means organic.
 */
export function flowReasonsById(
  trades: readonly FlowTradeIdentified[],
  opts: FlowClassifyOptions,
): Map<string, FlowReason> {
  const out = new Map<string, FlowReason>();
  for (const t of classifyFlowTrades(trades, opts)) {
    if (t.reason) out.set(t.id, t.reason);
  }
  return out;
}

/** Build the `patternKeys` set from draft pattern arrays the same way
 *  Flow Discovery structure checkboxes key structures. */
export function patternKeysFrom(patterns: readonly (readonly string[])[]): Set<string> {
  return new Set(patterns.filter((p) => p.length > 0).map((p) => JSON.stringify(p)));
}
