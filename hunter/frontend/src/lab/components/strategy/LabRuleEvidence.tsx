import { RuleAnalyzePanel } from 'components/strategy/RuleAnalyzePanel';
import { inspectFromPosition } from 'components/strategy/inspectTarget';
import { LazyLabTokenInspectModal } from '@lab/components/strategy/LazyLabTokenInspectModal';
import type { StrategyRule } from 'lib/strategy/types';

/**
 * Lab Evidence — the shared rule-evidence panel over the **traded** (real/paper)
 * positions the live engine produced, with the lab-only token inspect wired in.
 *
 * This is the payoff of serving positions on the analysis box: clicking a real fill
 * opens the metric panes pinned to the rule that traded it (`ruleOverride`) and
 * anchored on the actual entry (`positionEntry`), so you can read what every metric
 * said at the moment the engine fired — and what the position-scoped
 * retrace/pnl/held series did afterwards. Neither is possible on the live app: that
 * bin serves no `metric-series` route.
 *
 * The rows come from the local mirror `scripts/db-incremental-sync.ps1` fills, so
 * they're a snapshot as of the last sync — there's no ingest or SSE here, hence
 * `liveUpdates` stays off and no live open-count is claimed.
 */
export function LabRuleEvidence({
  ruleId,
  rule,
  onClose,
  scoreScope = 'all',
}: {
  ruleId: string;
  rule: StrategyRule;
  onClose: () => void;
  /** Keeps the Evidence default aligned with the list's scoreboard scope, so
   *  drilling into a rule shows the population you were just ranking on. */
  scoreScope?: 'current' | 'all';
}) {
  return (
    <RuleAnalyzePanel
      ruleId={ruleId}
      rule={rule}
      embedded
      onClose={onClose}
      initialScopeKind={scoreScope}
      notice={
        <p className="text-[11px] text-text-dim">
          Traded positions from the last DB sync — a snapshot, not a live feed. Click a
          position to inspect its chart + metric panes.
        </p>
      }
      renderInspect={({ position, rule: inspectRule, onClose: close }) => (
        <LazyLabTokenInspectModal
          target={inspectFromPosition(position)}
          titleSuffix="Position inspect"
          ruleOverride={
            inspectRule
              ? {
                  paramsJson: inspectRule.params,
                  fingerprintId: inspectRule.fingerprint_id,
                  label: inspectRule.rule_name,
                }
              : null
          }
          onClose={close}
        />
      )}
    />
  );
}
