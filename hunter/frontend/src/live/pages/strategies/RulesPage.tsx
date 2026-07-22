import { useState } from 'react';
import { useSelector } from 'react-redux';
import { RulesView } from 'components/strategy/RulesView';
import { RuleAnalyzePanel } from 'components/strategy/RuleAnalyzePanel';
import { selectOpenByRule, selectRuleOpenCounts } from '@live/slices/liveStatusSlice';
import type { StrategyRule } from 'lib/strategy/types';

/**
 * Live Rules Control — sticky scoreboard (activate/pause) + Evidence pane
 * (run navigator, summary, positions) for the selected rule.
 */
export function RulesPage() {
  const ruleLiveCounts = useSelector(selectRuleOpenCounts);
  const [scoreScope, setScoreScope] = useState<'current' | 'all'>('current');

  return (
    <RulesView
      showScores
      scoreScope={scoreScope}
      onScoreScopeChange={setScoreScope}
      ruleLiveCounts={ruleLiveCounts}
      renderAnalyze={({ ruleId, rule, clear }) => (
        <LiveRuleEvidence
          key={ruleId}
          ruleId={ruleId}
          rule={rule}
          clear={clear}
          scoreScope={scoreScope}
        />
      )}
    />
  );
}

/** The shared Evidence panel with the live-only wiring: the SSE position feed and
 *  this rule's open count from the live status slice. The selector is called in its
 *  own component so a live-status tick re-renders only the panel, not RulesView. */
function LiveRuleEvidence({
  ruleId,
  rule,
  clear,
  scoreScope,
}: {
  ruleId: string;
  rule: StrategyRule;
  clear: () => void;
  scoreScope: 'current' | 'all';
}) {
  const liveOpen = useSelector(selectOpenByRule(ruleId));
  return (
    <RuleAnalyzePanel
      ruleId={ruleId}
      rule={rule}
      embedded
      onClose={clear}
      initialScopeKind={scoreScope}
      liveOpenCount={liveOpen.length}
      liveUpdates
    />
  );
}
