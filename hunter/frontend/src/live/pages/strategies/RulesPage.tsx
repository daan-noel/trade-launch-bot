import { useState } from 'react';
import { useSelector } from 'react-redux';
import { RulesView } from 'components/strategy/RulesView';
import { RuleAnalyzePanel } from '@live/components/strategy/RuleAnalyzePanel';
import { selectRuleOpenCounts } from '@live/slices/liveStatusSlice';

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
        <RuleAnalyzePanel
          key={ruleId}
          ruleId={ruleId}
          rule={rule}
          embedded
          onClose={clear}
          initialScopeKind={scoreScope}
        />
      )}
    />
  );
}
