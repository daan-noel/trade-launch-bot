import { lazy, Suspense, useCallback, useEffect, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { RulesView } from 'components/strategy/RulesView';
import { Tabs, TabsList, TabsPanel, TabsTrigger } from 'components/ui/Tabs';
import { LoadingState } from 'components/ui/LoadingState';
import { formatDocumentTitle } from 'components/layout/documentTitle';
import { DryRunPanel } from '@lab/components/strategy/DryRunPanel';
import { LabRuleEvidence } from '@lab/components/strategy/LabRuleEvidence';
import { labNav } from '@lab/nav';
import { MODE_PARAM } from 'lib/strategy/mode';
import { RULES_SIMULATE_TAB, STRATEGY_PARAMS } from 'lib/strategy/nav';
import { TAG_PARAMS } from 'lib/strategy/tags';

const SimulatePage = lazy(() =>
  import('@lab/pages/strategies/SimulatePage').then((m) => ({ default: m.SimulatePage })),
);

type CopyBoard = 'copy' | 'simulate';

function boardFromParams(params: URLSearchParams): CopyBoard {
  return params.get(STRATEGY_PARAMS.tab) === RULES_SIMULATE_TAB ? 'simulate' : 'copy';
}

/**
 * Lab Copy page. One page, two boards: Copy (scoreboard, editor, dry-run,
 * Evidence) and Simulate (lake backtest of saved copy rules). Only the active
 * board mounts. Metric rules stay on the Rules page.
 *
 * Switching boards drops `mode` / `tags` / `notags` and keeps `rule`. Each board
 * then restores its own stored scope. Those params are shared names on one URL,
 * so leaving them in place would copy one board's filter onto the other.
 */
export function CopyPage() {
  const [params, setParams] = useSearchParams();
  const board = boardFromParams(params);
  const [scoreScope, setScoreScope] = useState<'current' | 'all'>('all');

  useEffect(() => {
    const page = board === 'simulate' ? 'Simulate' : 'Copy';
    document.title = formatDocumentTitle(labNav.identity.appTitle, page);
  }, [board]);

  const setBoard = useCallback(
    (next: string) => {
      const boardNext: CopyBoard = next === RULES_SIMULATE_TAB ? 'simulate' : 'copy';
      if (boardNext === board) return;
      setParams(
        (prev) => {
          const nextParams = new URLSearchParams(prev);
          if (boardNext === 'simulate') nextParams.set(STRATEGY_PARAMS.tab, RULES_SIMULATE_TAB);
          else nextParams.delete(STRATEGY_PARAMS.tab);
          nextParams.delete(MODE_PARAM);
          nextParams.delete(TAG_PARAMS.include);
          nextParams.delete(TAG_PARAMS.exclude);
          return nextParams;
        },
        { replace: true },
      );
    },
    [board, setParams],
  );

  return (
    <Tabs value={board} onValueChange={setBoard}>
      <TabsList className="px-4">
        <TabsTrigger value="copy">Copy</TabsTrigger>
        <TabsTrigger value="simulate">Simulate</TabsTrigger>
      </TabsList>
      <TabsPanel value="copy" className="pt-0">
        <RulesView
          board="copy"
          linkToSimulate
          hideHeading
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
      </TabsPanel>
      <TabsPanel value="simulate" className="pt-0">
        <Suspense fallback={<LoadingState label="Loading simulate…" />}>
          <SimulatePage board="copy" />
        </Suspense>
      </TabsPanel>
    </Tabs>
  );
}
