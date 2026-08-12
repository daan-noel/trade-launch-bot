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

  return (
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
  );
});
