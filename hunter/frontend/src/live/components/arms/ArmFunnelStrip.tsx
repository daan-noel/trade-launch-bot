/**
 * The arm funnel — how selective the cohort's rules were, in one strip.
 *
 * This is the read `strategy_positions` cannot give you: positions show what the
 * bot bought, the funnel shows everything it armed on and how each episode ended.
 * A rule that enters 2 of 400 arms and one that enters 2 of 5 look identical on
 * the History page.
 *
 * Every tile is a **lens**: clicking one narrows the cohort's `reason`, so the
 * table below shows exactly the episodes the tile counted. Clicking the active
 * tile clears it.
 */

import { memo } from 'react';
import { StatTile } from 'components/ui/StatTile';
import { cn } from 'lib/cn';
import { formatDurationShort } from 'utils/format';
import type { ArmFunnel } from 'lib/strategy/types';
import type { ArmCohortApi, ArmReason } from '@live/pages/console/armCohort';
import { metricLeaf } from './armBlockers';

/** Tiles in funnel order: what armed, what traded, what is still open, then the
 *  ways an episode ended without a trade — busiest first. */
const REASON_TILES: { reason: ArmReason; label: string; key: keyof ArmFunnel }[] = [
  { reason: 'waiting', label: 'Waiting', key: 'live' },
  { reason: 'dead', label: 'Dead', key: 'dead' },
  { reason: 'unsatisfiable', label: 'Unsat', key: 'unsatisfiable' },
  { reason: 'migrated', label: 'Migrated', key: 'migrated' },
  { reason: 'paused', label: 'Paused', key: 'paused' },
  { reason: 'duplicate_identity', label: 'Copycat', key: 'duplicate_identity' },
];

export const ArmFunnelStrip = memo(function ArmFunnelStrip({
  funnel,
  loading,
  cohort,
}: {
  funnel: ArmFunnel | null;
  loading: boolean;
  cohort: ArmCohortApi;
}) {
  const n = (v: number | null | undefined) =>
    loading || v == null ? '—' : v.toLocaleString();
  // Toggle semantics: re-clicking the active lens clears it, so a tile is never
  // a one-way trip into a cohort with no visible way out.
  const lens = (reason: ArmReason) =>
    cohort.set({ reason: cohort.reason === reason ? null : reason });
  const activeCls = (reason: ArmReason) =>
    cohort.reason === reason ? 'rounded ring-1 ring-primary/50' : '';

  const blocked = funnel?.blocked_by ?? [];
  const blockedTotal = blocked.reduce((s, b) => s + b.n, 0);

  return (
    <div className="space-y-1.5">
    <div className="grid grid-cols-2 gap-1.5 sm:grid-cols-4 lg:grid-cols-9">
      <StatTile label="Armed" value={n(funnel?.armed)} size="sm" bold />
      <button type="button" className={cn('text-left', activeCls('entered'))} onClick={() => lens('entered')}>
        <StatTile
          label="Entered"
          value={n(funnel?.entered)}
          sub={
            loading || !funnel
              ? undefined
              : `${funnel.entry_rate_pct.toFixed(funnel.entry_rate_pct < 1 ? 2 : 1)}% of arms`
          }
          size="sm"
          tone={funnel && funnel.entered > 0 ? 'green' : 'default'}
          bold
        />
      </button>
      {REASON_TILES.map((t) => (
        <button
          key={t.reason}
          type="button"
          className={cn('text-left', activeCls(t.reason))}
          onClick={() => lens(t.reason)}
        >
          <StatTile label={t.label} value={n(funnel?.[t.key] as number)} size="sm" />
        </button>
      ))}
      <StatTile
        label="Median wait"
        value={
          loading || funnel?.median_waited_sec == null
            ? '—'
            : formatDurationShort(Math.round(funnel.median_waited_sec))
        }
        sub="ended episodes"
        size="sm"
      />
    </div>

    {/* What the `Unsat` tile counts, broken out by the condition that held each
        episode out. The tile says how much the cohort threw away; this says
        which knob threw it — the whole reason to record a blocker at all.
        Each bar is a lens too, on the same toggle semantics as the tiles. */}
    {!loading && blocked.length > 0 && (
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 rounded border border-white/8 bg-bg-panel/60 px-2 py-1.5 text-[11px]">
        <span className="font-bold uppercase tracking-wider text-text-dim">Blocked by</span>
        {blocked.map((b) => (
          <button
            key={b.blocked_by}
            type="button"
            className={cn(
              'inline-flex items-baseline gap-1 rounded px-1 hover:bg-white/5',
              cohort.blockedBy === b.blocked_by ? 'ring-1 ring-primary/50' : '',
            )}
            onClick={() =>
              cohort.set({ blockedBy: cohort.blockedBy === b.blocked_by ? null : b.blocked_by })
            }
          >
            <span className="text-text-mid">{metricLeaf(b.blocked_by)}</span>
            <span className="tabular-nums font-semibold">{b.n.toLocaleString()}</span>
            <span className="tabular-nums text-text-dim">
              {blockedTotal > 0 ? `${Math.round((b.n / blockedTotal) * 100)}%` : ''}
            </span>
          </button>
        ))}
      </div>
    )}
    </div>
  );
});
