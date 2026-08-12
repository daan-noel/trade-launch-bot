/** Client-side PREVIEW of `hunter_engine::metrics::flow_split::FlowState`
 *  (Rust SSOT: hunter/engine/src/metrics/flow_split.rs:305-386) — used by
 *  token charts (Flow Discovery, Simulate, inspect) to redraw vol/non-vol
 *  lines without a backend round-trip. Visualization only — never wired to
 *  live trading decisions. Equivalence classes match via `JSON.stringify`
 *  of ordered `ix_labels` arrays.
 *
 *  Mirrors the Rust classify order: creator wallet → always volume; wallet
 *  already tagged (forward-only contagion) → volume; else structural
 *  `ix_labels` match against the pattern set; else organic. */

export interface FlowTradeLite {
  wallet_address: string;
  /** Signed by convention of the caller — this module only reads magnitude. */
  sol: number;
  ix_labels: readonly string[] | null | undefined;
}

export interface FlowClassifyOptions {
  /** `JSON.stringify(labels)` keys of the checked volume_ix_patterns. */
  patternKeys: ReadonlySet<string>;
  /** Token creator wallet address — always classified as volume-side, and
   *  seeds the contagion set (mirrors `FlowState::set_creator`). */
  creatorWallet?: string | null;
}

/**
 * WHY a trade counts as volume-side. The per-trade Vol badge tests structure
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
  isVol: boolean;
  /** `null` ⇔ `isVol === false`. */
  reason: FlowReason | null;
  volSol: number;
  nonVolSol: number;
}

/** Classify `trades` (must already be in canonical order — slot -> tx_index
 *  -> leg_index) into vol/non-vol, forward-tagging wallets as they're seen. */
export function classifyFlowTrades<T extends FlowTradeLite>(
  trades: readonly T[],
  opts: FlowClassifyOptions,
): (T & FlowClassified)[] {
  const taggedWallets = new Set<string>();
  if (opts.creatorWallet) taggedWallets.add(opts.creatorWallet);

  const out: (T & FlowClassified)[] = [];
  for (const t of trades) {
    const structuralMatch =
      !!t.ix_labels &&
      t.ix_labels.length > 0 &&
      opts.patternKeys.has(JSON.stringify(t.ix_labels));
    // Read the contagion set BEFORE this trade can join it, so a wallet's first
    // structural match is reported as `structural` and only its later trades as
    // `wallet` — otherwise every trade of a tagged wallet looks like contagion
    // and nothing points back at the pattern that started it.
    const wasTagged = taggedWallets.has(t.wallet_address);
    const isVol = wasTagged || structuralMatch;
    const reason: FlowReason | null = wasTagged
      ? t.wallet_address === opts.creatorWallet
        ? 'creator'
        : 'wallet'
      : structuralMatch
        ? 'structural'
        : null;
    if (isVol) taggedWallets.add(t.wallet_address);
    const g = Math.abs(t.sol);
    out.push({ ...t, isVol, reason, volSol: isVol ? g : 0, nonVolSol: isVol ? 0 : g });
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
