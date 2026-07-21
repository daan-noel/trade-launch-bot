import { useSelector } from 'react-redux';
import { RulesView } from 'components/strategy/RulesView';
import { RuleAnalyzePanel } from '@live/components/strategy/RuleAnalyzePanel';
import { selectRuleOpenCounts } from '@live/slices/liveStatusSlice';

/**
 * Live Rules — activate/pause/stop + scoreboard + master–detail Analyze panel
 * (summary, temporal bands, DB-backed position history) for the selected rule.
 */
export function RulesPage() {
  const ruleLiveCounts = useSelector(selectRuleOpenCounts);
  return (
    <RulesView
      showScores
      ruleLiveCounts={ruleLiveCounts}
      renderAnalyze={({ ruleId, rule, clear }) => (
        <RuleAnalyzePanel
          ruleId={ruleId}
          rule={rule}
          embedded
          onClose={clear}
        />
      )}
    />
  );
}
