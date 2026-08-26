import { useMemo } from 'react';

import { Badge } from 'components/ui/Badge';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { formatDecimalTrim } from 'utils/format';
import { compareWalletColor } from 'components/token-price-chart/constants';
import type { ProfileWalletInfo } from 'components/token-price-chart/types';
import type { TraderTokenRow } from 'types';
import type { CoBucketKey } from './coTrade';
import {
  CO_BUCKET_HINT,
  CO_BUCKET_KEYS,
  CO_BUCKET_VARIANT,
  coDepthCounts,
  coTradeMix,
  coTradePerWallet,
  coupledCount,
} from './coTrade';

/**
 * Co-trade headline for the current cohort: how much of the primary's window the
 * comparison wallets shared, and - the part that decides whether any of it means
 * anything - how those shared entries are distributed across the coupling
 * buckets.
 *
 * Read the MIX, not the overlap count. Two wallets active on the same day will
 * land on some of the same memecoins by chance alone, and that coincidence shows
 * up as `independent`. A real shared trigger concentrates in `co-slot` (same
 * block, so neither wallet could have seen the other) and its immediate
 * neighbours. An overlap of 40 that is all `independent` is a weaker finding
 * than an overlap of 4 that is all `co-slot`.
 *
 * **Per wallet, then the total.** The totals count each row ONCE, on its tightest
 * coupling, so with several wallets compared they are the SET's ceiling and no
 * single wallet's evidence - one busy wallet can carry a strip the others had no
 * part in. The per-wallet chips are the actual multi-wallet read, and each one is
 * a focus toggle: focusing re-points these totals, the `Lag` / `Coupling` columns
 * and the co-traded-only filter at that wallet alone.
 *
 * **The coupling badges are selectable** — an OR set. Picking `co-slot` alone is
 * the strongest read the page offers (both wallets in the same block, so neither
 * saw the other); picking `leads` + `follows` asks the copying question instead;
 * `independent` is the coincidence pile, worth opening exactly once to confirm it
 * looks like noise. Each badge's count is what selecting it yields, so a badge and
 * its own filter can never disagree.
 *
 * **The depth ladder** answers the other multi-wallet question: how many tokens
 * carry ONE of the comparison wallets, two of them, all of them. The last rung is
 * the intersection - the tokens the primary and every named wallet were on, which
 * is the set to read when the question is whether these wallets are one operation
 * rather than three traders in the same market. Each rung sets the co-traded-only
 * filter to that depth, and the ladder counts the whole query result rather than
 * the filtered cohort so it never collapses onto the slice it just selected.
 */
export function CoTradeSummary({
  rows,
  depthRows,
  bucketRows,
  comparison,
  profileWallets,
  focus,
  onFocusChange,
  buckets,
  onBucketsChange,
  minWallets,
  onMinWalletsChange,
}: {
  /** The cohort the table currently shows - the same rows the columns read. */
  rows: TraderTokenRow[];
  /** The cohort the DEPTH ladder previews over: narrowed by every other control,
   *  never by the depth itself (see the ladder note above). */
  depthRows: TraderTokenRow[];
  /** The same, for the coupling badges: narrowed by everything but the bucket
   *  selection, so each badge keeps offering the switch back. */
  bucketRows: TraderTokenRow[];
  /** Comparison wallet addresses, in the order the picker holds them. */
  comparison: string[];
  profileWallets: ProfileWalletInfo[];
  /** The comparison wallet every single-answer surface currently speaks for, or
   *  `null` for the whole set. */
  focus: string | null;
  onFocusChange: (wallet: string | null) => void;
  /** The selected coupling buckets - an OR set, empty meaning no narrowing. */
  buckets: CoBucketKey[];
  onBucketsChange: (next: CoBucketKey[]) => void;
  /** How many comparison wallets the co-traded-only filter currently demands. */
  minWallets: number;
  onMinWalletsChange: (n: number) => void;
}) {
  const mix = useMemo(() => coTradeMix(rows, focus), [rows, focus]);
  const perWallet = useMemo(() => coTradePerWallet(rows, comparison), [rows, comparison]);
  const depth = useMemo(
    () => coDepthCounts(depthRows, comparison.length),
    [depthRows, comparison.length],
  );
  // Badge counts: what each bucket would give you right now. Same derivation the
  // badge filter runs, so the number and the click always agree.
  const bucketMix = useMemo(() => coTradeMix(bucketRows, focus), [bucketRows, focus]);
  const byAddress = useMemo(
    () => new Map(profileWallets.map((w) => [w.address, w])),
    [profileWallets],
  );
  if (comparison.length === 0) return null;

  const label = (addr: string) =>
    byAddress.get(addr)?.label ?? `${addr.slice(0, 4)}…${addr.slice(-4)}`;
  const sharePct = mix.total > 0 ? (mix.overlap / mix.total) * 100 : 0;
  // Coupled = every bucket a single tape event could plausibly explain. The one
  // number worth reading next to the overlap count.
  const coupled = coupledCount(mix);
  const coupledPct = mix.overlap > 0 ? (coupled / mix.overlap) * 100 : 0;

  return (
    <div className="mb-3 flex flex-col gap-2 rounded-md border border-white/8 bg-white/3 px-3 py-2 text-[11px]">
      {/* Per-wallet row - the multi-wallet read AND the focus control. A wallet
          that shared 900 tokens and one that shared 2 are indistinguishable in
          the totals below; only this row separates them. */}
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
        <span className="text-[10px] font-bold uppercase tracking-widest text-text-dim">
          Comparing
        </span>
        {perWallet.map((w, slot) => {
          const info = byAddress.get(w.wallet);
          const active = focus === w.wallet;
          const pct = w.overlap > 0 ? (w.coupled / w.overlap) * 100 : 0;
          return (
            <button
              key={w.wallet}
              type="button"
              aria-pressed={active}
              onClick={() => onFocusChange(active ? null : w.wallet)}
              title={`${w.wallet}\n${w.overlap} of ${w.total} tokens shared, ${w.coupled} within 3 slots (${formatDecimalTrim(pct, 0)}%)\nClick to read Lag / Coupling for this wallet alone.`}
              className={`inline-flex items-center gap-1.5 rounded border px-1.5 py-0.5 transition-colors ${
                active
                  ? 'border-white/30 bg-white/10'
                  : `border-white/10 bg-white/5 hover:border-white/20 ${focus ? 'opacity-50' : ''}`
              }`}
            >
              {/* Square, slot-colored - this strip is the LEGEND for the chart
                  markers, so the swatch matches the silhouette and the hue those
                  markers actually draw with (see `compareWalletColor`). */}
              <span
                className="size-2 rounded-[1px]"
                style={{ background: compareWalletColor(slot, info) }}
              />
              <span className="text-text">{label(w.wallet)}</span>
              <span className="text-text-dim">{w.overlap}</span>
              {w.overlap > 0 && (
                <span className={pct >= 50 ? 'font-bold text-accent' : 'text-text-dim/80'}>
                  {formatDecimalTrim(pct, 0)}%
                </span>
              )}
            </button>
          );
        })}
        <InfoTooltip
          title="Overlap and coupled share, per wallet"
          body={
            'Each chip: how many of the cohort tokens that wallet also traded, and what share of those overlaps landed within 3 slots of the primary. ' +
            'The totals below count every row once, on its tightest coupling, so they are the SET ceiling - one busy wallet can carry them on its own. ' +
            'Click a chip to point the Lag / Coupling columns, the co-traded-only filter and those totals at that wallet alone.'
          }
        />
      </div>

      {/* Totals row - the whole set, or the focused wallet alone. */}
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
        {focus && (
          <span className="flex items-center gap-1.5">
            <span className="text-[10px] font-bold uppercase tracking-widest text-text-dim">
              Focus
            </span>
            <span className="text-text">{label(focus)}</span>
            <button
              type="button"
              className="text-text-dim underline-offset-2 hover:text-text hover:underline"
              onClick={() => onFocusChange(null)}
            >
              clear
            </button>
          </span>
        )}

        <span className="text-text">
          <span className="font-bold">{mix.overlap}</span>
          <span className="text-text-dim">
            {' '}
            of {mix.total} tokens shared ({formatDecimalTrim(sharePct, 1)}%)
          </span>
        </span>

        <span className="flex flex-wrap items-center gap-1.5">
          {CO_BUCKET_KEYS.map((b) => {
            const count = b === 'unordered' ? bucketMix.unknown : bucketMix.byBucket[b];
            const selected = buckets.includes(b);
            // An empty bucket stays clickable only while selected (so it can be
            // cleared); otherwise it is a dead end and says so by being disabled.
            const dead = count === 0 && !selected;
            return (
              <button
                key={b}
                type="button"
                aria-pressed={selected}
                disabled={dead}
                onClick={() =>
                  onBucketsChange(
                    selected ? buckets.filter((x) => x !== b) : [...buckets, b],
                  )
                }
                title={`${CO_BUCKET_HINT[b]}

${count} tokens in this cohort. Click to keep only these; click again to release. Badges combine as OR.`}
                className={dead ? 'cursor-default' : 'cursor-pointer'}
              >
                <Badge
                  variant={selected ? CO_BUCKET_VARIANT[b] : 'neutral'}
                  size="sm"
                  // A ring, not just the variant: `independent` and `unordered`
                  // are neutral either way, so color alone cannot show selection.
                  className={
                    dead
                      ? 'opacity-40'
                      : selected
                        ? 'ring-1 ring-white/40'
                        : 'hover:border-white/25'
                  }
                >
                  {b} {count}
                </Badge>
              </button>
            );
          })}
          {buckets.length > 0 && (
            <button
              type="button"
              className="text-text-dim underline-offset-2 hover:text-text hover:underline"
              onClick={() => onBucketsChange([])}
            >
              clear
            </button>
          )}
          <InfoTooltip
            title="Coupling buckets are a filter"
            body={
              'Each badge counts the tokens whose coupling reads that way, and clicking it keeps only those - several badges combine as OR. ' +
              'co-slot alone is the strongest read on the page: same block, so neither wallet could have seen the other and both answered the same tape event. ' +
              'leads + follows is the copying question. independent is the coincidence pile - worth opening once to confirm it looks like noise, and worth deselecting after. ' +
              'The counts preview over the cohort as the other controls leave it, so they never collapse onto the selection you just made.'
            }
          />
        </span>

        {mix.overlap > 0 && (
          <span className="flex items-center gap-1 text-text-dim">
            <span>
              <span className={coupledPct >= 50 ? 'font-bold text-accent' : 'text-text'}>
                {formatDecimalTrim(coupledPct, 0)}%
              </span>{' '}
              coupled
            </span>
            <InfoTooltip
              title="Read this, not the overlap count"
              body={
                'Share of the overlaps landing within 3 slots either side - close enough that one tape event could have driven both wallets. ' +
                'Two busy wallets land on some of the same memecoins by chance alone, and that coincidence sits in the independent bucket. ' +
                'An overlap of 40 that is all independent is a weaker finding than an overlap of 4 that is all co-slot.'
              }
            />
          </span>
        )}
      </div>

      {/* Depth ladder - the union at one end, the INTERSECTION at the other. Only
          with more than one wallet compared: at a set of one the two coincide. */}
      {comparison.length > 1 && (
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
          <span className="text-[10px] font-bold uppercase tracking-widest text-text-dim">
            Shared by
          </span>
          {depth.map((count, i) => {
            const n = i + 1;
            const all = n === comparison.length;
            const active = minWallets === n;
            return (
              <button
                key={n}
                type="button"
                aria-pressed={active}
                onClick={() => onMinWalletsChange(n)}
                title={
                  all
                    ? `${count} tokens the primary and ALL ${n} comparison wallets were on - the intersection. Click to keep only those.`
                    : `${count} tokens at least ${n} of the comparison wallets were also on. Click to keep only those.`
                }
                className={`inline-flex items-center gap-1.5 rounded border px-1.5 py-0.5 transition-colors ${
                  active
                    ? 'border-white/30 bg-white/10'
                    : 'border-white/10 bg-white/5 hover:border-white/20'
                } ${count === 0 ? 'opacity-45' : ''}`}
              >
                <span className="text-text-dim">
                  {n === 1 ? 'any' : all ? `all ${n}` : `${n}+`}
                </span>
                <span className={all && count > 0 ? 'font-bold text-accent' : 'text-text'}>
                  {count}
                </span>
              </button>
            );
          })}
          <InfoTooltip
            title="Union at one end, intersection at the other"
            body={
              'How many tokens carry at least that many of the comparison wallets, over the whole query - not the filtered cohort, so the rungs hold still while you use them. ' +
              'The last rung is the intersection: the tokens the primary and EVERY named wallet were on. That is the set to read when the question is whether these wallets are one operation; the first rung is satisfied by any single wallet, which two busy traders hit by coincidence. ' +
              'Where the ladder falls off matters too - 200 tokens at 2 and 3 at 3 is one pair, not a family. Clicking a rung sets the co-traded-only filter to that depth.'
            }
          />
        </div>
      )}
    </div>
  );
}
