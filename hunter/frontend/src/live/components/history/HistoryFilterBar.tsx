/**
 * The single cohort driver for the Console History section — date range · rule ·
 * mode · status · exit reason. Everything below it (charts deck AND the paged
 * table) reads the same cohort, so the charts can never describe a different
 * population than the rows underneath them.
 */

import { memo, useMemo } from 'react';
import { Button } from 'components/ui/Button';
import { DateTimeRangePicker } from 'components/ui/DateTimeRangePicker';
import {
  isoToPickerInput,
  pickerInputToIso,
} from 'components/ui/dateTimeRangePickerUtils';
import { SearchableSelect } from 'components/ui/SearchableSelect';
import { ModeBadge } from 'components/strategy/ModeBadge';
import { ModeToggle } from 'components/strategy/ModeToggle';
import { cn } from 'lib/cn';
import { useGetStrategyRulesQuery } from 'store/sharedEndpoints';
import type { HistoryRange } from 'lib/strategy/nav';
import type { HistoryCohortApi } from '@live/pages/console/historyCohort';
import {
  HISTORY_EXIT_FILTER_OPTIONS,
  historyExitFilterToneClass,
  isHistoryMetricExitNeedle,
} from '@live/pages/console/historyExitFilter';
import { historyFocusLabel } from '@live/pages/console/historyFocus';
import { activeExitTileLabel } from '@live/pages/console/historyExitSummary';

const RANGE_PRESETS: { value: HistoryRange; label: string; description?: string }[] = [
  { value: 'today', label: 'Today', description: 'UTC midnight → now' },
  { value: '7d', label: '7 days' },
  { value: '30d', label: '30 days' },
  { value: 'all', label: 'All time' },
  { value: 'custom', label: 'Custom' },
];

/** Glanceable status hues — in-flight amber, stuck/failed red, open info. */
function statusToneClass(status: string | null | undefined): string {
  switch (status) {
    case 'End':
      return 'text-secondary';
    case 'Holding':
      return 'text-info';
    case 'BuySubmitted':
    case 'ExitPending':
    case 'ExitUnconfirmed':
      return 'text-warning';
    case 'ExitStuck':
    case 'EntryFailed':
      return 'text-red';
    default:
      return 'text-text-dim';
  }
}

/** Terminal + open statuses a reviewer actually filters on. */
const STATUSES: { value: string; label: string }[] = [
  { value: 'End', label: 'Closed (End)' },
  { value: 'EntryFailed', label: 'Entry failed' },
  { value: 'Holding', label: 'Holding' },
  { value: 'ExitStuck', label: 'Exit stuck' },
  { value: 'ExitPending', label: 'Exit pending' },
  { value: 'ExitUnconfirmed', label: 'Exit unconfirmed' },
  { value: 'BuySubmitted', label: 'Buy submitted' },
];

export const HistoryFilterBar = memo(function HistoryFilterBar({
  cohort,
  closedCount,
  entryFailed,
  ruleNameOf,
}: {
  cohort: HistoryCohortApi;
  /** Closes in the cohort (from the series) — the honest "what am I looking at". */
  closedCount: number | null;
  entryFailed: number | null;
  ruleNameOf?: (id: string) => string | null;
}) {
  const { data: rules = [] } = useGetStrategyRulesQuery();
  const ruleOptions = useMemo(
    () =>
      [...rules]
        .map((r) => ({ value: r.id, label: r.rule_name || r.id.slice(0, 8), data: r }))
        .sort((a, b) => a.label.localeCompare(b.label)),
    [rules],
  );
  const statusOptions = useMemo(
    () =>
      STATUSES.map((s) => ({
        value: s.value,
        label: s.label,
        data: { cls: statusToneClass(s.value) },
      })),
    [],
  );
  const exitOptions = useMemo(
    () =>
      HISTORY_EXIT_FILTER_OPTIONS.map((o) => ({
        value: o.value,
        label: o.label,
        data: { cls: historyExitFilterToneClass(o.value), kind: o.kind },
      })),
    [],
  );
  const focusLabel = cohort.focus
    ? historyFocusLabel(cohort.focus, ruleNameOf)
    : null;
  const metricExitLabel = isHistoryMetricExitNeedle(cohort.exitReason)
    ? activeExitTileLabel(cohort.exitReason, null)
    : null;

  return (
    <div className="flex flex-col gap-2 rounded-lg border border-white/6 bg-bg-panel p-2.5">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <DateTimeRangePicker
          aria-label="Date range"
          size="sm"
          timeZone="UTC"
          zoneLabel="UTC"
          emptyLabel="Select date range"
          presets={RANGE_PRESETS}
          customPreset="custom"
          value={{
            preset: cohort.range,
            // Pass computed preset bounds too so the popover draft is seeded
            // (Custom starts from the active window) and the trigger can show
            // a compact "7 days · MM/DD → now" hint.
            from: isoToPickerInput(cohort.fromIso),
            to: isoToPickerInput(cohort.toIso),
          }}
          onChange={({ preset, from, to }) => {
            if (preset !== 'custom') {
              cohort.set({ range: preset });
              return;
            }
            cohort.set({
              range: 'custom',
              fromIso: pickerInputToIso(from),
              toIso: pickerInputToIso(to),
            });
          }}
        />

        <ModeToggle
          layout="ops"
          size="sm"
          aria-label="Execution mode"
          value={cohort.mode}
          onChange={(mode) => cohort.set({ mode })}
        />

        <div className="min-w-[190px]">
          <SearchableSelect
            options={ruleOptions}
            value={cohort.ruleId}
            onChange={(v) => cohort.set({ ruleId: v || null })}
            emptyOptionLabel="All rules"
            placeholder="All rules"
            noResultsLabel="No rule matches"
          />
        </div>

        <div className="grid min-w-[280px] grid-cols-2 gap-2">
          <SearchableSelect
            options={statusOptions}
            value={cohort.status}
            onChange={(v) => cohort.set({ status: v || null })}
            emptyOptionLabel="Any status"
            placeholder="Any status"
            noResultsLabel="No status matches"
            className={cn(
              cohort.status && 'font-semibold',
              statusToneClass(cohort.status),
            )}
            renderOption={(opt) => (
              <span className={cn('font-semibold', opt.data.cls)}>{opt.label}</span>
            )}
          />
          <SearchableSelect
            options={exitOptions}
            // Metric± tiles write synthetic needles not in this dropdown —
            // show "Any exit" there; the exit-mix tile carries the selection.
            value={isHistoryMetricExitNeedle(cohort.exitReason) ? null : cohort.exitReason}
            onChange={(v) => cohort.set({ exitReason: v || null })}
            emptyOptionLabel="Any exit"
            placeholder="Any exit"
            noResultsLabel="No exit matches"
            className={cn(
              cohort.exitReason &&
                !isHistoryMetricExitNeedle(cohort.exitReason) &&
                'font-semibold',
              historyExitFilterToneClass(
                isHistoryMetricExitNeedle(cohort.exitReason) ? null : cohort.exitReason,
              ),
            )}
            renderOption={(opt) => (
              <span
                className={cn('font-semibold', opt.data.cls)}
                title={
                  opt.data.kind === 'metric'
                    ? 'Metric exits matching this name (e.g. stall >= 300)'
                    : 'System exit reason'
                }
              >
                {opt.label}
              </span>
            )}
          />
        </div>

        {cohort.active && (
          <Button size="sm" variant="ghost" onClick={cohort.reset}>
            Clear
          </Button>
        )}
      </div>

      <div className="flex flex-wrap items-center gap-2 text-[11px] text-text-dim">
        <span>
          Cohort:{' '}
          <span className="text-text">
            {closedCount != null ? `${closedCount} closed` : '…'}
          </span>
          {entryFailed != null && entryFailed > 0 && (
            <>
              {' · '}
              <span title="Buys that never filled — no SOL deployed, excluded from PnL">
                {entryFailed} entry-failed
              </span>
            </>
          )}
        </span>
        {cohort.mode === 'paper' && <ModeBadge mode="paper" />}
        {cohort.mode === 'real' && <ModeBadge mode="real" />}
        {cohort.mode === 'all' && (
          <>
            <ModeBadge mode="real" />
            <ModeBadge mode="paper" />
          </>
        )}
        {metricExitLabel && (
          <span className="inline-flex items-center gap-1 rounded-md border border-primary/30 bg-primary/10 px-1.5 py-0.5 text-primary">
            Exit: <span className="font-semibold text-text">{metricExitLabel}</span>
            <button
              type="button"
              className="ml-0.5 text-text-dim hover:text-text"
              title="Clear exit tile filter"
              onClick={() => cohort.set({ exitReason: null })}
            >
              ✕
            </button>
          </span>
        )}
        {focusLabel && (
          <span className="inline-flex items-center gap-1 rounded-md border border-primary/30 bg-primary/10 px-1.5 py-0.5 text-primary">
            Focus: <span className="font-semibold text-text">{focusLabel}</span>
            <button
              type="button"
              className="ml-0.5 text-text-dim hover:text-text"
              title="Clear chart focus"
              onClick={() => cohort.set({ focus: null })}
            >
              ✕
            </button>
          </span>
        )}
      </div>
    </div>
  );
});
