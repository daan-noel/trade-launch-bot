/** Client-side PREVIEW of `hunter_engine::metrics::flow_split::FlowState`
 *  (Rust SSOT: hunter/engine/src/metrics/flow_split.rs:305-386) — used by the
 *  Flow Discovery per-token preview chart to redraw vol/non-vol lines
 *  instantly as `volume_ix_patterns` checkboxes toggle, without a backend
 *  round-trip. This is an approximation for visualization only (same house
 *  style as `FlowDiscoveryPage`'s `contagionByStructure` heuristic) — it is
 *  never wired to any live trading decision, so exact numeric-hash parity
 *  with the Rust FNV-1a `ix_hash` isn't required, only the same equivalence
 *  classes (label-array identity via `JSON.stringify`).
 *
 *  Mirrors the Rust classify order exactly: creator wallet -> always volume;
 *  wallet already tagged from an earlier trade (forward-only contagion) ->
 *  volume; else structural `ix_labels` match against the checked pattern set;
 *  else organic. Any volume trade tags its wallet for every later trade.
 *  See `flow-classify-parity.test.ts` for a fixture asserting this matches
 *  the Rust classifier's decisions on a canned trade sequence. */

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

export interface FlowClassified {
  isVol: boolean;
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
    const isVol = taggedWallets.has(t.wallet_address) || structuralMatch;
    if (isVol) taggedWallets.add(t.wallet_address);
    const g = Math.abs(t.sol);
    out.push({ ...t, isVol, volSol: isVol ? g : 0, nonVolSol: isVol ? 0 : g });
  }
  return out;
}

/** Build the `patternKeys` set from draft pattern arrays the same way
 *  `FlowDiscoveryPage`'s structure checkboxes already key structures. */
export function patternKeysFrom(patterns: readonly (readonly string[])[]): Set<string> {
  return new Set(patterns.filter((p) => p.length > 0).map((p) => JSON.stringify(p)));
}
