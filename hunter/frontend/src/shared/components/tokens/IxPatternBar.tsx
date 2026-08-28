import { Badge } from 'components/ui/Badge';
import { Select } from 'components/ui/Select';
import type { IxPatternTarget } from 'hooks/useIxPatternTarget';

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
    patterns,
    activeRuleCount,
    inferred,
    offHost,
    saving,
    error,
  } = target;
  const count = patterns.length;

  return (
    <span className="inline-flex flex-wrap items-center gap-2">
      <Badge variant="info" size="sm">
        Tagged patterns
      </Badge>
      <span className="font-mono text-[11px] text-text-dim">
        {count} pattern{count === 1 ? '' : 's'}
      </span>

      <Select
        fieldSize="sm"
        value={targetId ?? ''}
        onChange={(e) => setTargetId(e.target.value || null)}
        title="Fingerprint the Tagged badges write to"
        className="max-w-[16rem]"
      >
        <option value="">Edit patterns of…</option>
        {fingerprints.map((f) => (
          <option key={f.id} value={f.id}>
            {f.name}
          </option>
        ))}
      </Select>

      {saving && <span className="text-[11px] text-text-dim">Saving…</span>}
      {!targetId && (
        <span className="text-[11px] text-text-dim">
          Pick a fingerprint to make the Tagged badges editable.
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
          Not this chart&rsquo;s fingerprint — the Tagged badges follow the picked one, the chart
          lines do not.
        </span>
      )}
      {activeRuleCount > 0 && (
        <span className="text-[11px] text-warning">
          {activeRuleCount} active rule{activeRuleCount === 1 ? '' : 's'} use this fingerprint —
          editing changes how they classify flow.
        </span>
      )}
      {error && <span className="text-[11px] text-red">{error}</span>}
    </span>
  );
}
