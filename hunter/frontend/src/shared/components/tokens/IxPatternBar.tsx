import { Badge } from 'components/ui/Badge';
import { Checkbox } from 'components/ui/Checkbox';
import { Select } from 'components/ui/Select';
import { ToggleGroup } from 'components/ui/ToggleGroup';
import type { IxPatternTarget, TapeList } from 'hooks/useIxPatternTarget';
import {
  type IxPatternFeeField,
  type IxPatternFeeMask,
} from 'lib/strategy/ixPatternRows';

/** The three lists a badge click can write into. Tagged and dump are ordered
 *  `ix_labels` sequences (a build may sit on BOTH). Working is a different
 *  vocabulary — template grain ids for `m_burst_slot.working_templates`. */
const LISTS: {
  value: TapeList;
  label: string;
  title: string;
  activeClassName?: string;
}[] = [
  {
    value: 'tagged',
    label: 'tagged',
    title: 'm_flow_ix.ix_patterns — which trades the flow split calls volume-side',
  },
  {
    value: 'dump',
    label: 'dump',
    title: 'm_dump_ix.ix_patterns — the builds whose SELLS dump_sell_count counts',
    activeClassName: 'bg-warning/20 text-warning',
  },
  {
    value: 'working',
    label: 'working',
    title: 'm_burst_slot.working_templates — template grain ids harvest treats as working',
    activeClassName: 'bg-green/20 text-green',
  },
];

const FEE_PIN_TOGGLES: { field: IxPatternFeeField; label: string; title: string }[] = [
  {
    field: 'cu_limit',
    label: 'cu_limit',
    title: 'Copy this tx\'s cu_limit onto the staged row. Off (the default) stages the ix structure only, even when the tx has a limit.',
  },
  {
    field: 'cu_price',
    label: 'cu_price',
    title: 'Copy this tx\'s cu_price. Many clients recompute this per transaction — pin it only when you have seen it hold.',
  },
  {
    field: 'tip_lamports',
    label: 'tip',
    title: 'Copy this tx\'s tip. A tip is an auction bid and almost never a stable identity.',
  },
];

/**
 * Sticky fee-field modifiers for a trades table. Checking cu_limit then clicking
 * a tx stages that tx's ix labels plus that tx's cu_limit — not the other two.
 * All off = structure only. Hidden on the working list (grain ids, not ix+fee)
 * and on a read-only snapshot.
 */
export function FeePinToggles({
  mask,
  onChange,
  disabled = false,
}: {
  mask: IxPatternFeeMask;
  onChange: (next: IxPatternFeeMask) => void;
  disabled?: boolean;
}) {
  return (
    <span className="inline-flex flex-wrap items-center gap-1.5" title="Fields copied from the clicked tx. Default off = catch-all (any budget). Checking a box then clicking a selected catch-all narrows it to that pin — the two never sit together.">
      <span className="text-[9px] uppercase tracking-wide text-text-dim/60">pin</span>
      {FEE_PIN_TOGGLES.map(({ field, label, title }) => (
        <label key={field} className="inline-flex cursor-pointer items-center gap-0.5" title={title}>
          <Checkbox
            boxSize="sm"
            checked={!!mask[field]}
            disabled={disabled}
            onChange={() => onChange({ ...mask, [field]: !mask[field] })}
            aria-label={`Pin ${label} from the clicked tx`}
          />
          <span className="font-mono text-[10px] text-text-dim">{label}</span>
        </label>
      ))}
    </span>
  );
}

/**
 * The strip above a chart's trades table: which fingerprint's
 * `ix_patterns` the Tagged badges edit, and what a click there costs.
 *
 * There is no staging step. `metric_config` is not part of fingerprint identity,
 * so a write lands on the same row and every rule bound to it starts classifying
 * flow differently — hence the named target and the loud active-rule count, which
 * are the parts of the old draft flow that were actually earning their keep.
 *
 * The target is normally the host's own fingerprint and the picker just states it.
 * The two ways it can be something else — guessed from the pattern set, or picked
 * away from the host — are both called out, because from the badge alone they are
 * indistinguishable from editing the row you are looking at.
 */
export function IxPatternBar({
  target,
  readOnly = false,
}: {
  target: IxPatternTarget;
  /** A stored run's frozen snapshot — its numbers were computed under those
   *  patterns, so they are not this chart's to change. */
  readOnly?: boolean;
}) {
  if (readOnly) {
    return (
      <span
        className="text-[10px] uppercase tracking-wide text-text-dim/60"
        title="This chart shows a stored run's own ix_patterns — the numbers were computed under them, so they are not editable here."
      >
        run snapshot
      </span>
    );
  }

  const {
    fingerprints,
    targetId,
    setTargetId,
    list,
    setList,
    patterns,
    workingTemplates,
    feePins,
    setFeePins,
    activeRuleCount,
    inferred,
    offHost,
    saving,
    error,
  } = target;
  const count = list === 'working' ? workingTemplates.length : patterns.length;
  const isWorking = list === 'working';

  return (
    <span className="inline-flex flex-wrap items-center gap-2">
      <Badge variant={isWorking ? 'success' : list === 'dump' ? 'warning' : 'info'} size="sm">
        {isWorking ? 'Working grains' : list === 'dump' ? 'Dump builds' : 'Tagged patterns'}
      </Badge>
      <ToggleGroup
        size="sm"
        tone="neutral"
        aria-label="Which list a badge click writes into"
        value={list}
        onChange={setList}
        options={LISTS}
      />
      {!isWorking && (
        <FeePinToggles mask={feePins} onChange={setFeePins} disabled={!targetId} />
      )}
      <span className="font-mono text-[11px] text-text-dim">
        {count} {isWorking ? `grain${count === 1 ? '' : 's'}` : `pattern${count === 1 ? '' : 's'}`}
      </span>

      <Select
        fieldSize="sm"
        value={targetId ?? ''}
        onChange={(e) => setTargetId(e.target.value || null)}
        title="Fingerprint the badges write to"
        className="max-w-[16rem]"
      >
        <option value="">Fingerprint…</option>
        {fingerprints.map((f) => (
          <option key={f.id} value={f.id}>
            {f.name}
          </option>
        ))}
      </Select>

      {saving && <span className="text-[11px] text-text-dim">Saving…</span>}
      {!targetId && (
        <span className="text-[11px] text-text-dim">
          Pick a fingerprint to make the badges editable.
        </span>
      )}
      {/* A guessed target is never presented as a known one: this host had no
          fingerprint, so the row was found by matching the pattern set — which
          any number of rows can carry, empty sets most of all. */}
      {inferred && (
        <span
          className="text-[11px] text-text-dim"
          title="This host has no fingerprint of its own, so the target was matched by its pattern set. Confirm it before editing."
        >
          matched by patterns — confirm before editing
        </span>
      )}
      {/* Editing off-host is allowed but never silent: the badges below now
          answer for a different row than the chart's own lines. */}
      {offHost && (
        <span className="text-[11px] text-warning">
          Not this chart&rsquo;s fingerprint — the badges follow the picked one, the chart
          lines do not.
        </span>
      )}
      {activeRuleCount > 0 && (
        <span className="text-[11px] text-warning">
          {activeRuleCount} active rule{activeRuleCount === 1 ? '' : 's'} use this fingerprint —
          editing changes what they read.
        </span>
      )}
      {error && <span className="text-[11px] text-red">{error}</span>}
    </span>
  );
}
