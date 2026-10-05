import { useState } from 'react';
import { RulesView } from 'components/strategy/RulesView';
import { DryRunPanel } from '@lab/components/strategy/DryRunPanel';
import { LabRuleEvidence } from '@lab/components/strategy/LabRuleEvidence';

/**
 * Lab Copy board — the same scoreboard and builder as Rules, over copy rules.
 */
export function CopyPage() {
  const [scoreScope, setScoreScope] = useState<'current' | 'all'>('all');

  return (
    <RulesView
      board="copy"
      showScores
      scoreScope={scoreScope}
      onScoreScopeChange={setScoreScope}
      renderDryRun={(draft, canRun) => <DryRunPanel draft={draft} canRun={canRun} />}
      renderAnalyze={({ ruleId, rule, clear }) => (
        <LabRuleEvidence
          key={ruleId}
          ruleId={ruleId}
          rule={rule}
          onClose={clear}
          scoreScope={scoreScope}
        />
      )}
    />
  );
}
