import { useState } from 'react';
import { RulesView } from 'components/strategy/RulesView';
import { DryRunPanel } from '@lab/components/strategy/DryRunPanel';
import { LabRuleEvidence } from '@lab/components/strategy/LabRuleEvidence';

/**
 * Rules page (lab app) — the same scoreboard + Evidence cockpit as live Rules
 * Control, minus what only exists on a box that trades.
 *
 * The scoreboard columns (PnL / Avg% / Exp / Win% / W-L / N) and the TOTAL rollup are
 * position rollups, not live-engine facts, so they mean the same thing here: this is
 * where you rank rules on their **real** results and drill into a fill. Two lab-only
 * panels are injected on top — dry-run (simulate an unsaved draft) and Evidence with
 * the metric-pane inspect.
 *
 * Deliberately NOT passed: `ruleLiveCounts`. Those come from the live SSE status
 * slice (the engine's in-memory open/pending bags) and this box runs no engine —
 * `RulesView` drops the live columns and the TOTAL strip falls back to the DB open
 * count, so the board degrades honestly instead of showing a stale "live" number.
 */
export function RulesPage() {
  const [scoreScope, setScoreScope] = useState<'current' | 'all'>('all');

  return (
    <RulesView
      linkToSimulate
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
