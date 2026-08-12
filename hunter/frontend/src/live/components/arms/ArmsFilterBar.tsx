/**
 * The single cohort driver for the Console Arms section — date range · rule ·
 * mode · end reason. The funnel strip and the paged table below both read this
 * cohort, so the counts can never describe a different population than the rows.
 */

import { memo, useMemo } from 'react';
import { Button } from 'components/ui/Button';
import { DateTimeRangePicker } from 'components/ui/DateTimeRangePicker';
import {
  isoToPickerInput,
  pickerInputToIso,
} from 'components/ui/dateTimeRangePickerUtils';
import { SearchableSelect } from 'components/ui/SearchableSelect';
import { ModeToggle } from 'components/strategy/ModeToggle';
import { useGetStrategyRulesQuery } from 'store/sharedEndpoints';
import type { HistoryRange } from 'lib/strategy/nav';
import { ARM_END_LABEL } from '@live/components/floor/liveChartCards';
import { ARM_REASONS, type ArmCohortApi, type ArmReason } from '@live/pages/console/armCohort';

const RANGE_PRESETS: { value: HistoryRange; label: string; description?: string }[] = [
  { value: 'today', label: 'Today', description: 'UTC midnight → now' },
  { value: '7d', label: '7 days' },
  { value: '30d', label: '30 days' },
  { value: 'all', label: 'All time' },
  { value: 'custom', label: 'Custom' },
];

/** `waiting` is the section's own label for a live episode (`end_reason IS
 *  NULL`); every other key comes from the backend vocabulary. */
const REASON_LABEL: Record<ArmReason, string> = {
  waiting: 'Still waiting',
  entered: ARM_END_LABEL.entered,
  dead: ARM_END_LABEL.dead,
  migrated: ARM_END_LABEL.migrated,
  unsatisfiable: ARM_END_LABEL.unsatisfiable,
  paused: ARM_END_LABEL.paused,
  duplicate_identity: ARM_END_LABEL.duplicate_identity,
};

/** Glanceable hues: the one ending that traded reads green, the rest neutral. */
function reasonToneClass(reason: string | null | undefined): string {
  if (reason === 'entered') return 'text-secondary';
  if (reason === 'waiting') return 'text-warning';
  return 'text-text-dim';
}

export const ArmsFilterBar = memo(function ArmsFilterBar({
  cohort,
  armedCount,
}: {
  cohort: ArmCohortApi;
  /** Episodes in the cohort — the honest "what am I looking at". */
  armedCount: number | null;
}) {
  const { data: rules = [] } = useGetStrategyRulesQuery();
  const ruleOptions = useMemo(
    () =>
      [...rules]
        .map((r) => ({ value: r.id, label: r.rule_name || r.id.slice(0, 8), data: r }))
        .sort((a, b) => a.label.localeCompare(b.label)),
    [rules],
  );
  const reasonOptions = useMemo(
    () =>
      ARM_REASONS.map((r) => ({
        value: r,
        label: REASON_LABEL[r],
        data: { cls: reasonToneClass(r) },
      })),
    [],
  );

  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-2 rounded-lg border border-white/6 bg-bg-panel p-2.5">
      <DateTimeRangePicker
        aria-label="Arm date range"
        size="sm"
        timeZone="UTC"
        zoneLabel="UTC"
        emptyLabel="Select date range"
        presets={RANGE_PRESETS}
        customPreset="custom"
        value={{
          preset: cohort.range,
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

      <div className="min-w-[170px]">
        <SearchableSelect
          options={reasonOptions}
          value={cohort.reason}
          onChange={(v) => cohort.set({ reason: (v || null) as ArmReason | null })}
          emptyOptionLabel="Any outcome"
          placeholder="Any outcome"
          noResultsLabel="No outcome matches"
          renderOption={(opt) => (
            <span className={`font-semibold ${opt.data.cls}`}>{opt.label}</span>
          )}
        />
      </div>

      <span className="ml-auto flex items-center gap-2 text-[11px] text-text-dim">
        {armedCount != null && (
          <span className="tabular-nums">
            <span className="font-semibold text-text">{armedCount.toLocaleString()}</span> episodes
          </span>
        )}
        {cohort.active && (
          <Button variant="ghost" size="xs" onClick={cohort.reset}>
            Clear
          </Button>
        )}
      </span>
    </div>
  );
});
