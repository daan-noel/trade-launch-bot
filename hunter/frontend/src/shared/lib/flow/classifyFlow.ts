/** Client-side mirror of the engine's tag classifier
 *  (`hunter_engine::metrics::tags::state::TagState::fold_half`, Rust SSOT
 *  hunter/engine/src/metrics/tags/state.rs) - the charts and the trades table redraw
 *  a tag edit without a backend round trip. Visualization only, never wired to a
 *  trading decision.
 *
 *  One trade, one verdict, in the engine's order:
 *
 *  1. On the tag's `side` (absent = both), the trade carries the tag when ANY matcher
 *     holds - program, ix_shape, ix_template, ix_contains, ix_lacks, wallet, creator,
 *     then the sticky set, then `cluster` LAST (it counts the trade into its slot
 *     group, so it runs only when nothing else qualified it).
 *  2. Else, under `exclude_creation_slot`, a creation-slot buyer - and every later
 *     trade of that wallet - counts on NEITHER side.
 *  3. Else the trade is the rest (`@!tag`).
 *
 *  Trades must be the coin's FULL history in canonical order (slot -> tx_index ->
 *  leg_index): sticky, cluster and the creation slot are all forward-only state. */

import { anyRowMatchesTrade, type IxPatternRow } from 'lib/strategy/ixPatternRows';
import type { MatcherKey, TagDef } from 'lib/strategy/tagsDoc';
import { isLaunchGrain, templateGrain, templateProgram } from 'lib/strategy/templateGrain';

/** The tag a surface classifies against: a fingerprint tag, a lens set read as one,
 *  or a staging draft. `name` is display only (`@name`). */
export type FlowTag = Pick<TagDef, 'name' | 'match' | 'side' | 'sticky' | 'exclude_creation_slot'>;

/** One leg of a trade. */
export type FlowSide = 'buy' | 'sell';

export interface FlowTradeLite {
  wallet_address: string;
  /** Read as a magnitude, whatever sign convention the caller uses. */
  sol: number;
  ix_labels: readonly string[] | null | undefined;
  /** Which leg this is. A trade without one is off-side under a sided tag, is never
   *  a creation-slot buyer, and forms its own cluster groups. */
  side?: FlowSide | null;
  /** The trade's slot. Read by `cluster` (groups per slot) and
   *  `exclude_creation_slot`; absent reads as slot 0. */
  slot?: number | null;
  /** The fee budget this tx declared: read by fee-pinned ix shapes and by the
   *  cluster's group identity. Absent = not captured. */
  cu_limit?: number | null;
  cu_price?: number | null;
  tip_lamports?: number | null;
}

export interface FlowClassifyOptions {
  tag: FlowTag;
  /** The coin's creator wallet - the `creator` matcher's subject. */
  creatorWallet?: string | null;
  /** Analysis-only (a flow lens): wallets that always read as the rest and never
   *  move sticky, cluster or creation-slot state - the studied trader itself, so a
   *  lens does not classify its own subject. The engine has no such option. */
  excludeWallets?: ReadonlySet<string> | null;
}

/**
 * Why a trade sits where it does. A matcher key or `sticky` = it carries the tag
 * (`@tag`), through that matcher (the first that held, in the classifier's order).
 * `creation_slot` = excluded, on neither side. No reason = the rest (`@!tag`).
 */
export type FlowReason = MatcherKey | 'sticky' | 'creation_slot';

/** Which half a trade lands on. */
export type FlowHalf = 'tagged' | 'rest' | 'excluded';

export interface FlowClassified {
  half: FlowHalf;
  /** `half === 'tagged'`. */
  isTagged: boolean;
  /** `null` exactly when the trade is the rest (or unfoldable). */
  reason: FlowReason | null;
  /** SOL on the tagged half; 0 otherwise. */
  taggedSol: number;
  /** SOL on the rest; 0 otherwise (an excluded trade moves neither). */
  untaggedSol: number;
}

interface ClusterGroup {
  ix: string | null;
  side: FlowSide | null;
  fee: string;
  firstSol: number;
  close: number;
}

/** A tag compiled once per classification pass, never per trade. */
interface CompiledTag {
  programs: ReadonlySet<string>;
  shapes: readonly IxPatternRow[];
  templates: ReadonlySet<string>;
  contains: readonly string[];
  lacks: readonly string[];
  wallets: ReadonlySet<string>;
  creator: boolean;
}

function compile(tag: FlowTag): CompiledTag {
  const m = tag.match;
  return {
    programs: new Set(m.program ?? []),
    shapes: m.ix_shape ?? [],
    templates: new Set(m.ix_template ?? []),
    contains: m.ix_contains ?? [],
    lacks: m.ix_lacks ?? [],
    // The engine hashes `s.trim()` for wallets only.
    wallets: new Set((m.wallet ?? []).map((w) => w.trim()).filter(Boolean)),
    creator: m.creator === true,
  };
}

/** Marker containment is substring containment over each label (engine
 *  `marker_bits`): a label carries its program prefix. No labels = no markers. */
function hasMarker(labels: readonly string[], names: readonly string[]): boolean {
  return labels.some((l) => names.some((n) => l.includes(n)));
}

/** The first stateless matcher (or the sticky set) that qualifies the trade. */
function matchReason(
  c: CompiledTag,
  t: FlowTradeLite,
  labels: readonly string[],
  creatorWallet: string | null,
  sticky: ReadonlySet<string> | null,
): FlowReason | null {
  // Program, template and shape need labels (engine `*_hash` = None on none).
  const has = labels.length > 0;
  if (has && c.programs.size > 0 && c.programs.has(templateProgram(labels))) return 'program';
  if (has && c.shapes.length > 0 && anyRowMatchesTrade(c.shapes, labels, t)) return 'ix_shape';
  if (has && c.templates.size > 0 && c.templates.has(templateGrain(labels))) return 'ix_template';
  if (c.contains.length > 0 && hasMarker(labels, c.contains)) return 'ix_contains';
  // A label-less trade carries no marker, so it LACKS every one (engine bits = 0).
  if (c.lacks.length > 0 && !hasMarker(labels, c.lacks)) return 'ix_lacks';
  if (t.wallet_address && c.wallets.has(t.wallet_address)) return 'wallet';
  if (c.creator && creatorWallet && t.wallet_address === creatorWallet) return 'creator';
  if (sticky?.has(t.wallet_address)) return 'sticky';
  return null;
}

const feeKey = (t: FlowTradeLite) => `${t.cu_limit ?? ''}|${t.cu_price ?? ''}|${t.tip_lamports ?? ''}`;

/** Classify `trades` (canonical order) against one tag. */
export function classifyFlowTrades<T extends FlowTradeLite>(
  trades: readonly T[],
  opts: FlowClassifyOptions,
): (T & FlowClassified)[] {
  const { tag } = opts;
  const c = compile(tag);
  const creatorWallet = opts.creatorWallet ?? null;
  const excluded = opts.excludeWallets ?? null;
  const cluster = tag.match.cluster ?? null;
  const sticky = tag.sticky ? new Set<string>() : null;
  // Engine `set_creator`: under creator + sticky the creator starts in the set.
  if (sticky && c.creator && creatorWallet) sticky.add(creatorWallet);
  let birthSlot: number | null = null;
  const birthWallets = new Set<string>();
  let clusterSlot = 0;
  let groups: ClusterGroup[] = [];

  const clusterHit = (t: FlowTradeLite, labels: readonly string[], sol: number, slot: number) => {
    if (!cluster) return false;
    if (slot !== clusterSlot) {
      groups = [];
      clusterSlot = slot;
    }
    const ix = labels.length > 0 ? JSON.stringify(labels) : null;
    const side = t.side ?? null;
    const fee = feeKey(t);
    let g = groups.find((x) => x.ix === ix && x.side === side && x.fee === fee);
    if (!g) {
      g = { ix, side, fee, firstSol: sol, close: 0 };
      groups.push(g);
    }
    const close = Math.abs(sol - g.firstSol) <= (cluster.sol_tol_pct / 100) * g.firstSol;
    if (close) g.close += 1;
    return close && g.close >= cluster.min_prints;
  };

  const out: (T & FlowClassified)[] = [];
  const push = (t: T, half: FlowHalf, reason: FlowReason | null, sol: number) =>
    out.push({
      ...t,
      half,
      isTagged: half === 'tagged',
      reason,
      taggedSol: half === 'tagged' ? sol : 0,
      untaggedSol: half === 'rest' ? sol : 0,
    });

  for (const t of trades) {
    const sol = Math.abs(t.sol);
    const labels = t.ix_labels ?? [];
    const slot = t.slot ?? 0;
    if (excluded?.has(t.wallet_address)) {
      push(t, 'rest', null, sol);
      continue;
    }
    // Engine `on_trade`: a non-finite amount is not folded at all.
    if (!Number.isFinite(sol)) {
      push(t, 'excluded', null, 0);
      continue;
    }
    if (birthSlot === null && isLaunchGrain(labels)) birthSlot = slot;

    const onSide = tag.side == null || tag.side === t.side;
    let reason: FlowReason | null = onSide ? matchReason(c, t, labels, creatorWallet, sticky) : null;
    if (onSide && reason === null && clusterHit(t, labels, sol, slot)) reason = 'cluster';
    if (reason !== null) {
      sticky?.add(t.wallet_address);
      push(t, 'tagged', reason, sol);
      continue;
    }
    if (tag.exclude_creation_slot) {
      if (birthSlot === slot && t.side === 'buy') {
        birthWallets.add(t.wallet_address);
        push(t, 'excluded', 'creation_slot', sol);
        continue;
      }
      if (birthWallets.has(t.wallet_address)) {
        push(t, 'excluded', 'creation_slot', sol);
        continue;
      }
    }
    push(t, 'rest', null, sol);
  }
  return out;
}

/** A trade carrying the identity the trades table keys rows on. */
export interface FlowTradeIdentified extends FlowTradeLite {
  id: string;
}

/**
 * The verdict per trade id, for a table that sees one candle's rows and so cannot
 * recompute the forward-only state itself. `trades` must be the coin's FULL history
 * in canonical order. The rest is omitted - absent means `@!tag`.
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

/** `JSON.stringify(labels)` identity set of label sequences, blanks dropped. */
export function patternKeysFrom(patterns: readonly (readonly string[])[]): Set<string> {
  return new Set(patterns.filter((p) => p.length > 0).map((p) => JSON.stringify(p)));
}
