import { useState } from 'react';
import { useSelector } from 'react-redux';
import { RulesView } from 'components/strategy/RulesView';
import { RuleAnalyzePanel } from 'components/strategy/RuleAnalyzePanel';
import { LazyLivePositionInspectModal } from '@live/components/strategy/LazyLivePositionInspectModal';
import { useRuleScoresRealtime } from '@live/hooks/useRuleScoresRealtime';
import { selectOpenByRule, selectRuleOpenCounts } from '@live/slices/liveStatusSlice';
import type { StrategyRule } from 'lib/strategy/types';

/**
 * Live Copy board — the Rules Control chrome, over copy rules. The metric
 * Rules page does not list these rows.
 */
export function CopyPage() {
  const ruleLiveCounts = useSelector(selectRuleOpenCounts);
  useRuleScoresRealtime();
  const [scoreScope, setScoreScope] = useState<'current' | 'all'>('current');

  return (
    <RulesView
      board="copy"
      showScores
      scoreScope={scoreScope}
      onScoreScopeChange={setScoreScope}
      ruleLiveCounts={ruleLiveCounts}
      renderAnalyze={({ ruleId, rule, clear }) => (
        <LiveCopyEvidence
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

function LiveCopyEvidence({
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
      renderInspect={({ position, rule: inspectRule, onClose: close }) => (
        <LazyLivePositionInspectModal position={position} rule={inspectRule} onClose={close} />
      )}
    />
  );
}
