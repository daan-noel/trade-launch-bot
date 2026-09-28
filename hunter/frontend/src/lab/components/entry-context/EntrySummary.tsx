import { useMemo, useState } from 'react';
import { DataTable } from 'components/table/DataTable';
import type { ColumnDef } from 'components/table/types';
import { IxLabelsDisplay } from 'components/ui/IxLabelsDisplay';
import { ToggleGroup } from 'components/ui/ToggleGroup';
import { AXIS_BY_KEY } from '@lab/lib/entryContext/axes';
import {
  shareHistogram,
  structureBoard,
  type ShareBucket,
  type StructureStat,
} from '@lab/lib/entryContext/analysis';
import type { EntryRow } from '@lab/lib/entryContext/types';

type ShareBasis = 'tx' | 'sol';
type Scope = 'filtered' | 'all';

const pct = (n: number, d: number) => (d > 0 ? `${((100 * n) / d).toFixed(0)}%` : '-');

function Tile({ label, value, sub, title }: { label: string; value: string; sub?: string; title: string }) {
  return (
    <div className="min-w-[120px] rounded-md border border-white/8 bg-white/3 px-3 py-2" title={title}>
      <div className="text-[10px] font-bold uppercase tracking-widest text-text-dim">{label}</div>
      <div className="text-lg font-extrabold text-text">{value}</div>
      {sub && <div className="text-[11px] text-text-dim">{sub}</div>}
    </div>
  );
}

/**
 * Window vs control share distribution. Hand-rolled CSS bars (categorical x, as
 * `PnlDistribution`): the window in the primary hue, the control in neutral gray,
 * with a legend and a hover readout on every bar.
 */
function ShareHistogram({ buckets, label }: { buckets: ShareBucket[]; label: string }) {
  const max = Math.max(1, ...buckets.flatMap((b) => [b.window, b.control]));
  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-center gap-3 text-[11px] text-text-dim">
        <span className="inline-flex items-center gap-1">
          <span className="size-2 rounded-sm bg-primary" /> window
        </span>
        <span className="inline-flex items-center gap-1">
          <span className="size-2 rounded-sm bg-white/30" /> control
        </span>
        <span>entries by {label}</span>
      </div>
      <div className="flex h-[120px] items-end gap-[6px] border-b border-white/10">
        {buckets.map((b) => (
          <div key={b.lo} className="flex h-full flex-1 items-end gap-[2px]">
            {(['window', 'control'] as const).map((k) => (
              <div
                key={k}
                className={`flex-1 rounded-t ${k === 'window' ? 'bg-primary' : 'bg-white/30'}`}
                style={{ height: `${(100 * b[k]) / max}%`, minHeight: b[k] > 0 ? 2 : 0 }}
                title={`${k}: ${b[k]} entr${b[k] === 1 ? 'y' : 'ies'} with ${label} ${b.lo}-${b.hi}%`}
              />
            ))}
          </div>
        ))}
      </div>
      <div className="flex gap-[6px] text-[10px] text-text-dim">
        {buckets.map((b) => (
          <span key={b.lo} className="flex-1 text-center">
            {b.lo}
          </span>
        ))}
      </div>
    </div>
  );
}

const STRUCTURE_COLUMNS: ColumnDef<StructureStat>[] = [
  {
    key: 'key',
    label: 'Structure',
    width: '420px',
    render: (s) =>
      s.labels ? (
        <IxLabelsDisplay labels={s.labels} maxHeight="4.5rem" copyJson />
      ) : (
        <span className="font-mono text-[11px]">{s.key}</span>
      ),
    searchValue: (s) => s.key,
  },
  {
    key: 'present',
    label: 'In windows',
    tooltip: 'Entries whose window holds at least one print of this structure.',
    render: (s) => s.present,
    sortValue: (s) => s.present,
    searchValue: () => '',
    filterNumber: (s) => s.present,
  },
  {
    key: 'top',
    label: 'Largest in',
    tooltip: 'Entries where this structure made the most buy transactions in the window.',
    render: (s) => s.top,
    sortValue: (s) => s.top,
    searchValue: () => '',
    filterNumber: (s) => s.top,
  },
  {
    key: 'mean_tx',
    label: 'Mean tx %',
    tooltip: 'Its buy transactions over every buy transaction, averaged over all entries read (absent = 0%).',
    render: (s) => `${s.meanTxShare.toFixed(1)}%`,
    sortValue: (s) => s.meanTxShare,
    searchValue: () => '',
    filterNumber: (s) => s.meanTxShare,
  },
  {
    key: 'mean_sol',
    label: 'Mean SOL %',
    tooltip: 'Its buy SOL over every buy SOL, averaged over all entries read (absent = 0%).',
    render: (s) => `${s.meanSolShare.toFixed(1)}%`,
    sortValue: (s) => s.meanSolShare,
    searchValue: () => '',
    filterNumber: (s) => s.meanSolShare,
  },
  {
    key: 'tagged',
    label: 'Tagged in',
    tooltip: 'Entries where at least one of its buys carried the target tag.',
    render: (s) => s.tagged,
    sortValue: (s) => s.tagged,
    searchValue: () => '',
    filterNumber: (s) => s.tagged,
  },
];

/**
 * The target-axis summary: how many entries clear the filter, how the target
 * share distributes against its control window, and which structures fill the
 * windows. Scope picks the filtered entries or all of them.
 */
export function EntrySummary({
  entries,
  passing,
  tokensPassing,
  windowSecs,
}: {
  entries: EntryRow[];
  passing: EntryRow[];
  tokensPassing: number;
  windowSecs: number;
}) {
  const [basis, setBasis] = useState<ShareBasis>('tx');
  const [scope, setScope] = useState<Scope>('filtered');
  const readable = useMemo(() => entries.filter((e) => !e.unknown_reason), [entries]);
  const scoped = scope === 'filtered' ? passing : readable;
  const tokens = useMemo(() => new Set(entries.map((e) => e.mint_address)).size, [entries]);

  const winAxis = AXIS_BY_KEY.get(basis === 'tx' ? 'tx_share' : 'sol_share')!;
  const ctlAxis = AXIS_BY_KEY.get(basis === 'tx' ? 'ctl_tx_share' : 'ctl_sol_share')!;
  const hist = useMemo(() => shareHistogram(scoped, winAxis, ctlAxis), [scoped, winAxis, ctlAxis]);
  const board = useMemo(() => structureBoard(scoped), [scoped]);

  return (
    <section className="mb-4 flex flex-col gap-3">
      <div className="flex flex-wrap gap-2">
        <Tile label="Entries" value={String(entries.length)} sub={`${tokens} tokens`} title="Every buy transaction of the wallet in the range." />
        <Tile
          label="Readable"
          value={String(readable.length)}
          sub={`${entries.length - readable.length} unknown`}
          title="Entries the tape can answer for. Unknown = past tape retention, or fee pins with no fee reading."
        />
        <Tile
          label="Pass filter"
          value={String(passing.length)}
          sub={`${pct(passing.length, readable.length)} of readable`}
          title="Readable entries that clear every filter line."
        />
        <Tile label="Tokens passing" value={String(tokensPassing)} sub={`of ${tokens}`} title="Tokens with at least one passing entry." />
      </div>
      <div className="flex flex-wrap items-center gap-3">
        <ToggleGroup
          aria-label="Summary scope"
          size="sm"
          tone="neutral"
          value={scope}
          onChange={setScope}
          options={[
            { value: 'filtered', label: 'Filtered', title: 'Read the entries that pass the filter' },
            { value: 'all', label: 'All readable', title: 'Read every readable entry' },
          ]}
        />
        <ToggleGroup
          aria-label="Share basis"
          size="sm"
          tone="neutral"
          value={basis}
          onChange={setBasis}
          options={[
            { value: 'tx', label: 'Tx share', title: winAxis.definition(windowSecs) },
            { value: 'sol', label: 'SOL share', title: AXIS_BY_KEY.get('sol_share')!.definition(windowSecs) },
          ]}
        />
        <span className="text-[11px] text-text-dim">
          {scoped.length} entries · no buys in window: {hist.noWindow} · in control: {hist.noControl}
        </span>
      </div>
      <div className="max-w-[720px]">
        <ShareHistogram buckets={hist.buckets} label={winAxis.label} />
      </div>
      <DataTable
        columns={STRUCTURE_COLUMNS}
        rows={board}
        rowKey={(s) => s.key}
        tableId="entry_context_structures"
        defaultSort={{ col: 'present', dir: 'desc' }}
        defaultPageSize={10}
        searchable
        colFilters
        hoverable
        emptyMessage="No structures in these windows"
      />
    </section>
  );
}
