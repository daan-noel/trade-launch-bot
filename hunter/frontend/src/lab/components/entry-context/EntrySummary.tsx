import { useMemo } from 'react';
import { cohortTable } from '@lab/lib/entryContext/counts';
import type { EntryLogic } from '@lab/lib/entryContext/logic';
import type { EntryRow } from '@lab/lib/entryContext/types';
import { CardIntro, StatTable } from './StatTable';

const n = (v: number) => v.toLocaleString();

/**
 * His buys against the idea (the buys table's filters). Two rows: buys where the
 * probe found the target, and every buy. Two columns: buys and tokens. A token
 * counts when one of its buys does.
 */
export function EntrySummary({
  entries,
  logic,
  signaled,
  probeOn,
}: {
  entries: EntryRow[];
  logic: EntryLogic;
  /** The probe's verdict: the target's transaction landed in the probe window. */
  signaled: (e: EntryRow) => boolean;
  probeOn: boolean;
}) {
  const table = useMemo(
    () => cohortTable(entries, logic, signaled, probeOn),
    [entries, logic, signaled, probeOn],
  );

  if (entries.length === 0) return null;

  const idea = logic.conditions.filter((c) => c.kind === 'signal' && !c.key.startsWith('pe_'));

  return (
    <div className="mb-4 flex flex-col gap-2">
      <CardIntro>
        his / matched. His = buys in that row. Matched = those that pass the filters in the buys table
        {idea.length > 0
          ? ` (${idea.map((c) => `${c.label} ${c.text}`).join(', ')})`
          : ' (none set yet, so every buy matches)'}
        . A token counts when one of its buys does.
      </CardIntro>
      <StatTable
        columns={[
          { label: 'Buys', tip: 'his / matched' },
          { label: 'Tokens', tip: 'A token counts when at least one of its buys does.' },
        ]}
        rows={[
          {
            key: 'target',
            label: 'Target signal',
            tip: 'His entries whose signal is the ix structure you selected. Matched = those that also pass the filters.',
            cells: [`${n(table.target.buys.his)} / ${n(table.target.buys.matched)}`, `${n(table.target.tokens.his)} / ${n(table.target.tokens.matched)}`],
          },
          {
            key: 'all',
            label: 'All entries',
            tip: 'Every entry of his. Matched = those that pass the filters.',
            cells: [`${n(table.all.buys.his)} / ${n(table.all.buys.matched)}`, `${n(table.all.tokens.his)} / ${n(table.all.tokens.matched)}`],
          },
        ]}
      />
    </div>
  );
}
