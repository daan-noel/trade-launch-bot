import { useMemo } from 'react';

import { Badge } from 'components/ui/Badge';
import { InfoTooltip } from 'components/ui/InfoTooltip';
import { formatDecimalTrim } from 'utils/format';
import type { ProfileWalletInfo } from 'components/token-price-chart/types';
import type { TraderTokenRow } from 'types';
import { CO_TRADE_BUCKETS, CO_TRADE_BUCKET_HINT, coTradeMix } from './coTrade';

/**
 * Co-trade headline for the current cohort: how much of the primary's window the
 * comparison wallets shared, and — the part that decides whether any of it means
 * anything — how those shared entries are distributed across the coupling
 * buckets.
 *
 * Read the MIX, not the overlap count. Two wallets active on the same day will
 * land on some of the same memecoins by chance alone, and that coincidence shows
 * up as `independent`. A real shared trigger concentrates in `co-slot` (same
 * block, so neither wallet could have seen the other) and its immediate
 * neighbours. An overlap of 40 that is all `independent` is a weaker finding
 * than an overlap of 4 that is all `co-slot`.
 */
export function CoTradeSummary({
  rows,
  comparison,
  profileWallets,
}: {
  /** The cohort the table currently shows — the same rows the columns read. */
  rows: TraderTokenRow[];
  /** Comparison wallet addresses, in the order the picker holds them. */
  comparison: string[];
  profileWallets: ProfileWalletInfo[];
}) {
  const mix = useMemo(() => coTradeMix(rows), [rows]);
  const byAddress = useMemo(
    () => new Map(profileWallets.map((w) => [w.address, w])),
    [profileWallets],
  );
  if (comparison.length === 0) return null;

  const sharePct = mix.total > 0 ? (mix.overlap / mix.total) * 100 : 0;
  // Coupled = every bucket a single tape event could plausibly explain. The one
  // number worth reading next to the overlap count.
  const coupled = mix.byBucket['co-slot'] + mix.byBucket.leads + mix.byBucket.follows;
  const coupledPct = mix.overlap > 0 ? (coupled / mix.overlap) * 100 : 0;

  return (
    <div className="mb-3 flex flex-wrap items-center gap-x-4 gap-y-2 rounded-md border border-white/8 bg-white/3 px-3 py-2 text-[11px]">
      <span className="flex items-center gap-1.5">
        <span className="text-[10px] font-bold uppercase tracking-widest text-text-dim">
          Comparing
        </span>
        {comparison.map((addr) => {
          const info = byAddress.get(addr);
          return (
            <span key={addr} className="inline-flex items-center gap-1" title={addr}>
              <span
                className="size-1.5 rounded-full"
                style={{ background: info?.color ?? '#888' }}
              />
              <span className="text-text">
                {info?.label ?? `${addr.slice(0, 4)}…${addr.slice(-4)}`}
              </span>
            </span>
          );
        })}
      </span>

      <span className="text-text">
        <span className="font-bold">{mix.overlap}</span>
        <span className="text-text-dim">
          {' '}
          of {mix.total} tokens shared ({formatDecimalTrim(sharePct, 1)}%)
        </span>
      </span>

      <span className="flex flex-wrap items-center gap-1.5">
        {CO_TRADE_BUCKETS.map((b) => (
          <Badge
            key={b}
            variant='neutral'
            size="sm"
            title={CO_TRADE_BUCKET_HINT[b]}
            className={mix.byBucket[b] === 0 ? 'opacity-40' : undefined}
          >
            {b} {mix.byBucket[b]}
          </Badge>
        ))}
        {mix.unknown > 0 && (
          <Badge
            variant="neutral"
            size="sm"
            title="Shared the token, but one of the two wallets has no entry leg inside the window, so the two entries cannot be ordered."
          >
            unordered {mix.unknown}
          </Badge>
        )}
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
  );
}
