import type { FlowDiscoveryFeeVariant, FlowDiscoveryStructure } from 'types';

/**
 * Reading a build's fee budgets — the single definition behind the discovery
 * table's Budget column and the pattern cart's pin control.
 *
 * The one question every helper here serves: is this build's budget a compiled-in
 * preset, or a number its client recomputes per transaction? A preset is identity
 * and pins well. A recomputed value is not, and pinning it produces a list entry
 * that matches the one transaction it was copied from and then never fires again.
 * Cardinality is what separates the two, which is why nothing here reports a
 * budget without also reporting how many there are.
 */

/** The most-traded budget on a build — the backend already sorts them, so this is
 *  the head. `undefined` when the build has none (a pre-field result, or a build
 *  whose every trade predates fee capture). */
export function dominantFeeVariant(
  s: FlowDiscoveryStructure,
): FlowDiscoveryFeeVariant | undefined {
  return s.fee_variants?.[0];
}

/** Share of the build's fee-carrying trades that sit on its dominant budget, `0`
 *  when nothing was captured.
 *
 *  Deliberately measured against trades that HAVE a reading rather than against
 *  every trade: a build half of whose history predates capture is not therefore
 *  half-unpinnable, it is simply half-unmeasured, and mixing the two would rank a
 *  perfectly stable preset below a noisy one that happens to be newer. */
export function feeConcentration(s: FlowDiscoveryStructure): number {
  const variants = s.fee_variants ?? [];
  if (variants.length === 0) return 0;
  const known = variants.reduce((sum, v) => sum + v.n_trades, 0);
  return known > 0 ? variants[0].n_trades / known : 0;
}

/** Which band a build's budget spread falls in — the Budget column's filter, and
 *  the honest short answer to "can I pin this". */
export function feeSpreadBand(s: FlowDiscoveryStructure): string {
  const n = s.fee_variant_count ?? s.fee_variants?.length ?? 0;
  if (n === 0) return 'unknown';
  if (n === 1) return 'single';
  return n <= 4 ? 'few' : 'many';
}

/** A budget as one line: `300k CU @ 3333333 +2000000 tip`. Absent fields are
 *  omitted rather than rendered as zero — a field nobody set is not a field set
 *  to nothing. */
export function formatFeeVariant(v: FlowDiscoveryFeeVariant): string {
  const parts: string[] = [];
  if (v.cu_limit != null) parts.push(`${formatCu(v.cu_limit)} CU`);
  if (v.cu_price != null) parts.push(`@ ${v.cu_price.toLocaleString()}`);
  if (v.tip_lamports != null && v.tip_lamports > 0) {
    parts.push(`+${v.tip_lamports.toLocaleString()} tip`);
  }
  return parts.length > 0 ? parts.join(' ') : '—';
}

/** Compute units, abbreviated the way the presets are spoken: `300k`, `1.4M`. */
function formatCu(units: number): string {
  if (units >= 1_000_000) return `${(units / 1_000_000).toFixed(units % 1_000_000 === 0 ? 0 : 1)}M`;
  if (units >= 1_000) return `${(units / 1_000).toFixed(units % 1_000 === 0 ? 0 : 1)}k`;
  return String(units);
}

/** The whole variant list as hover text: every budget with its trade share, then
 *  the unknown count. This is the readout a list author actually decides on — the
 *  cell can only show the head. */
export function feeVariantTitle(s: FlowDiscoveryStructure): string {
  const variants = s.fee_variants ?? [];
  const unknown = s.fee_unknown_trades ?? 0;
  if (variants.length === 0) {
    return unknown > 0
      ? `${unknown.toLocaleString()} trades, none carrying a fee reading — nothing to pin yet`
      : 'no fee readings on this build';
  }
  const known = variants.reduce((sum, v) => sum + v.n_trades, 0);
  const lines = variants.map((v) => {
    const pct = known > 0 ? ((v.n_trades / known) * 100).toFixed(0) : '0';
    const sol =
      v.priority_lamports != null ? ` = ${(v.priority_lamports / 1e9).toFixed(6)} SOL` : '';
    return `${formatFeeVariant(v)}${sol} — ${v.n_trades.toLocaleString()} trades (${pct}%)`;
  });
  const total = s.fee_variant_count ?? variants.length;
  if (total > variants.length) {
    lines.push(`… ${total - variants.length} more budgets not shown`);
  }
  if (unknown > 0) {
    lines.push(`${unknown.toLocaleString()} trades with no fee reading (predate fee capture)`);
  }
  if (total > 1) {
    lines.push(
      '',
      total <= 4
        ? 'Several budgets: check whether the top one is the operator and the rest are noise.'
        : 'Many budgets: this client recomputes its fee per transaction. Pin the shape only.',
    );
  }
  return lines.join('\n');
}
