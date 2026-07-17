import { RulesView } from 'components/strategy/RulesView';
import { DryRunPanel } from '@lab/components/strategy/DryRunPanel';

/** Rules authoring page (lab app) — the shared editor with the lab-only dry-run
 *  panel injected, so an unsaved draft can be simulated against recent history. */
export function RulesPage() {
  return (
    <RulesView renderDryRun={(draft, canRun) => <DryRunPanel draft={draft} canRun={canRun} />} />
  );
}
