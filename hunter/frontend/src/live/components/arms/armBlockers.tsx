/**
 * Rendering for `strategy_arms.end_detail` — the disarm's own answer to "the rule
 * watched this token and passed; what was it short of".
 *
 * `unsatisfiable` is the one `end_reason` whose name explains nothing: it means an
 * upper bound on a metric that never decreases was crossed, so the buy condition can
 * never hold again (`m_state.age_sec < 20` once the coin is 20 s old, or
 * `m_flow.buy_count <= 5` once it has 6 buys). The engine captures the buy conditions
 * still failing at that instant and they land here.
 *
 * The condition text goes through `conditionExprFromJson` → `formatConditions` —
 * the same pair the rule editor round-trips authored DNF through — so a threshold
 * reads here exactly as it reads where it was written.
 *
 * Everything is a plain string builder plus two thin components: the table cell
 * and the modal fact state the same thing at two densities, and a second phrasing
 * for the second surface is how they start disagreeing.
 */

import { conditionExprFromJson, formatConditions } from 'lib/strategy/grammar';
import { formatDecimalTrim } from 'utils/format';
import type { ArmEndDetail, ArmUnmetCondition } from 'lib/strategy/types';
import { formatWindowSpec } from 'lib/strategy/windowSpec';

/** A read's label as the arm row states it. Since the v2 metric system the stored
 *  `metric` IS the full label, `m_flow.buy_sol @!volume [10s]`: tag and span
 *  included, and the family kept (a bare `buy_sol` is also an `m_slot` quantity).
 *  Rows written before that carry `m_flow_window.gross_flow` with the window beside
 *  it, so the window is appended there: two windows of one metric are two different
 *  conditions. */
export function unmetLabel(c: ArmUnmetCondition): string {
  const w = c.metric.includes('[') ? '' : formatWindowSpec(c.window ?? (c.window_size_sec ? { size: c.window_size_sec, lag: 0, unit: 'sec' } : null));
  return w ? `${c.metric}(${w})` : c.metric;
}

/** The short name a narrow cell shows: the label without its `m_` prefix. */
export const metricLeaf = (path: string) => path.replace(/^m_/, '');

/** `gross_flow(60s) 24.71 · needs >=40` — what it read, then what it needed. */
export function unmetText(c: ArmUnmetCondition): string {
  const expr = conditionExprFromJson(c.conditions);
  const need = expr ? formatConditions(expr) : '';
  // `null` is a metric the fold could not read — which satisfies nothing, and so
  // is a blocker in its own right rather than a missing number.
  const got = c.value == null ? 'unreadable' : formatDecimalTrim(c.value, 2);
  return need ? `${unmetLabel(c)} ${got} · needs ${need}` : `${unmetLabel(c)} ${got}`;
}

/** The one-line verdict: what blocked it, or that only the clock did. */
export function endDetailText(d: ArmEndDetail): string {
  const deadline = `${d.killed_by.metric} ${d.killed_by.operator} ${d.killed_by.threshold} can no longer hold`;
  if (d.unmet.length === 0) {
    // Not "no data": everything else held, so the coin qualified too late.
    return `${deadline}; every other condition held`;
  }
  return `${deadline}; ${d.unmet.map(unmetText).join(' · ')}`;
}

/** The Arms table's **Blocked by** cell. Dash for every ending but
 *  `unsatisfiable`, which is the only one that records a detail. */
export function ArmBlockedByCell({ detail }: { detail: ArmEndDetail | null }) {
  if (!detail) return <span className="text-text-dim">—</span>;
  const first = detail.unmet[0];
  if (!first) {
    return (
      <span className="text-text-dim" title={endDetailText(detail)}>
        clock only
      </span>
    );
  }
  return (
    <span className="inline-flex items-baseline gap-1" title={endDetailText(detail)}>
      <span className="text-text-mid">{unmetLabel(first)}</span>
      <span className="tabular-nums text-text-dim">
        {first.value == null ? '—' : formatDecimalTrim(first.value, 2)}
      </span>
      {/* Entry is an AND: a second unmet condition is not a runner-up, it is
          equally binding. Say how many rather than implying a ranking. */}
      {detail.unmet.length > 1 && (
        <span className="text-text-dim">+{detail.unmet.length - 1}</span>
      )}
    </span>
  );
}

/** The modal header's statement of the same fact, at full width. */
export function ArmEndDetailLine({ detail }: { detail: ArmEndDetail | null }) {
  if (!detail) return null;
  return (
    <div className="mt-2 rounded border border-warning/25 bg-warning/5 px-2 py-1.5 text-[11px] leading-relaxed">
      <span className="font-bold uppercase tracking-wider text-text-dim">Short of </span>
      {detail.unmet.length === 0 ? (
        <span className="text-text-mid">
          nothing: every other condition held, so the coin qualified too late.
        </span>
      ) : (
        <span className="text-text-mid">
          {detail.unmet.map((c) => unmetText(c)).join(' · ')}
        </span>
      )}
      <div className="mt-0.5 text-text-dim">
        Gave up: {detail.killed_by.metric} {detail.killed_by.operator} {detail.killed_by.threshold} can no longer
        hold
      </div>
    </div>
  );
}
