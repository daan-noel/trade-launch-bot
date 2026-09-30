import { useMemo } from 'react';
import { cohortTable } from '@lab/lib/entryContext/counts';
import type { EntryLogic } from '@lab/lib/entryContext/logic';
import type { EntryRow } from '@lab/lib/entryContext/types';
import { PassTable, type PassCell } from './StatTable';
import { HIS_HELP } from './summaryHelp';

const n = (v: number) => v.toLocaleString();
const cell = (c: { his: number; matched: number }, tip: (pass: string, all: string) => string): PassCell => ({
  all: c.his,
  pass: c.matched,
  tip: tip(n(c.matched), n(c.his)),
});

/**
 * His buys against the idea (the filters). Two rows: his buys in the token pool
 * (the probe found the target before them), which use every filter; and every buy
 * of his, which uses only the filters that need no selected IXs. Per row, buys and
 * tokens before and after the filters. A token counts when one of its buys does.
 */
export function EntrySummary({
  entries,
  logic,
  signaled,
  probeOn,
  pool,
  ixFilters,
  anyFilters,
}: {
  entries: EntryRow[];
  logic: EntryLogic;
  /** The probe's verdict: the target's transaction landed in the probe window. */
  signaled: (e: EntryRow) => boolean;
  probeOn: boolean;
  /** What the pool is, in words: `6Vo3 in the 3 slots before his buy`. */
  pool: string;
  /** Target IXs filters in force: the all row skips them. */
  ixFilters: number;
  /** Any IXs filters in force. */
  anyFilters: number;
}) {
  const table = useMemo(
    () => cohortTable(entries, logic, signaled, probeOn),
    [entries, logic, signaled, probeOn],
  );

  if (entries.length === 0) return null;

  const all = table.all.buys.his;
  const inPool = table.target.buys.his;
  const poolBuys = cell(table.target.buys, (p, a) => `${p} of his ${a} buys in the pool pass every filter.`);
  const allBuys = cell(table.all.buys, (p, a) => `${p} of his ${a} buys pass the Any IXs filters.`);

  return (
    <div className="mb-4 flex flex-col gap-1.5">
      <PassTable
        groups={['Buys', 'Tokens']}
        help={HIS_HELP.table(poolBuys, allBuys)}
        rows={[
          {
            key: 'pool',
            label: 'In pool',
            sub: probeOn ? `his buys right after the selected IXs (${pool})` : 'Probe off: no pool',
            help: probeOn ? HIS_HELP.pool(pool, poolBuys) : HIS_HELP.poolOff,
            uses: 'every',
            usesNote: `${ixFilters} + ${anyFilters} set`,
            cells: [
              poolBuys,
              cell(table.target.tokens, (p, a) => `${p} of their ${a} tokens have a buy that passes every filter.`),
            ],
            highlight: true,
          },
          {
            key: 'all',
            label: 'All his buys',
            sub: 'every buy of his, any IXs',
            help: HIS_HELP.all(allBuys),
            uses: 'any',
            usesNote: `${anyFilters} set${ixFilters > 0 ? ` · ${ixFilters} Target IXs skipped` : ''}`,
            cells: [
              allBuys,
              cell(table.all.tokens, (p, a) => `${p} of his ${a} tokens have a buy that passes the Any IXs filters.`),
            ],
          },
        ]}
      />
      {probeOn && all > 0 && (
        <p className="text-[11px] text-text-dim">
          In pool = {Math.round((inPool / all) * 100)}% of his buys ({inPool.toLocaleString()} of{' '}
          {all.toLocaleString()}).
        </p>
      )}
    </div>
  );
}
