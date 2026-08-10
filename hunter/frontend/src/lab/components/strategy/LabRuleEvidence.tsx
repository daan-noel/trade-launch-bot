import { useMemo } from 'react';
import { RuleAnalyzePanel } from 'components/strategy/RuleAnalyzePanel';
import { inspectFromPosition } from 'components/strategy/inspectTarget';
import { InlineAlert } from 'components/ui/Modal';
import { useMintEpisodeMarkers } from 'hooks/useMintEpisodeMarkers';
import { LazyLabTokenInspectModal } from '@lab/components/strategy/LazyLabTokenInspectModal';
import type { StrategyRule } from 'lib/strategy/types';
import type { RulePositionRecord } from 'types';

/**
 * Lab Evidence — the shared rule-evidence panel over the **traded** (real/paper)
 * positions the live engine produced, with the lab-only token inspect wired in.
 *
 * This is the payoff of serving positions on the analysis box: clicking a real fill
 * opens the metric panes pinned to the rule that traded it (`ruleOverride`) and
 * anchored on the actual entry (`positionEntry`), so you can read what every metric
 * said at the moment the engine fired — and what the position-scoped
 * retrace/bounce/pnl/held series did afterwards. Neither is possible on the live app: that
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
        <InlineAlert variant="warning">
          Snapshot from the last DB sync — not a live feed. Click a position to open
          chart + metric panes.
        </InlineAlert>
      }
      renderInspect={({ position, rule: inspectRule, onClose: close }) => (
        <LabPositionInspect position={position} rule={inspectRule} onClose={close} />
      )}
    />
  );
}

/**
 * One traded position's inspect modal, drawing the mint's **whole** traded history —
 * every episode, every exit leg — not just the row that was clicked. Its own
 * component rather than an inline `renderInspect` body because the episode overlay
 * is a hook, and hooks cannot live in a render callback.
 */
function LabPositionInspect({
  position,
  rule,
  onClose,
}: {
  position: RulePositionRecord;
  rule: StrategyRule | null;
  onClose: () => void;
}) {
  const target = useMemo(() => inspectFromPosition(position), [position]);
  const eventMarkers = useMintEpisodeMarkers({
    mint: position.mint_address,
    mode: position.mode,
    focus: target,
    focusPositionId: position.id,
  });

  return (
    <LazyLabTokenInspectModal
      target={target}
      eventMarkers={eventMarkers}
      titleSuffix="Position inspect"
      ruleOverride={
        rule
          ? {
              paramsJson: rule.params,
              fingerprintId: rule.fingerprint_id,
              label: rule.rule_name,
            }
          : null
      }
      onClose={onClose}
    />
  );
}
