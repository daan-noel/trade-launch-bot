import { useMemo } from 'react';
import { runSummarySections } from 'lib/strategy/runSummary';
import { SummaryStatsPanel } from 'components/strategy/SummaryStatsPanel';
import type { SimulatedSummary } from 'types';

/**
 * Full dry-run summary — same shared two-band panel as Simulate / sweep /
 * live positions (`runSummarySections` → `SummaryStatsPanel`), so a promoted
 * combo is reviewed under the same labels before save.
 */
export function SimSummary({ summary }: { summary: SimulatedSummary }) {
  const { hero, sections } = useMemo(
    () => runSummarySections(summary, { migrated: summary.n_migrated, extended: true }),
    [summary],
  );

  return (
    <SummaryStatsPanel
      title="Dry-run results"
      heroStats={hero}
      sections={sections}
      accentClass="bg-info"
      className="mb-0 mt-2"
    />
  );
}
