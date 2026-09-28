import { useCallback, useMemo, useState } from 'react';
import { skipToken } from '@reduxjs/toolkit/query/react';
import type { ColumnDef } from 'components/table/types';
import { tokenColumns } from 'components/tokens/tokenColumns';
import { TokenTable } from 'components/tokens/TokenTable';
import { ALL_TOKEN_INFO_KEYS } from 'components/tokens/sharedTokenColumns';
import { Checkbox } from 'components/ui/Checkbox';
import { DateTimeRangePicker } from 'components/ui/DateTimeRangePicker';
import { IconButton } from 'components/ui/IconButton';
import { SearchIcon, SpinnerIcon } from 'components/ui/icons';
import { Input } from 'components/ui/Input';
import { SectionDivider } from 'components/ui/SectionDivider';
import { Select } from 'components/ui/Select';
import { FlowLensProvider } from 'context/FlowLensContext';
import { useTimezone } from 'context/TimezoneContext';
import { useLocalStorage } from 'hooks/useLocalStorage';
import { useProfileWallets } from 'hooks/useProfileWallets';
import { STORAGE_KEYS } from 'lib/storage';
import { apiErrorMessage } from 'store/apiSlice';
import { datetimeLocalToUtcWallClock, utcIsoToDatetimeLocal } from 'utils/date';
import type { TraderTokenRow } from 'types';
import { FlowLensBar } from '@lab/components/analysis/FlowLensBar';
import { useTraderFlowLens } from '@lab/components/analysis/useTraderFlowLens';
import { EntryFilterBar } from '@lab/components/entry-context/EntryFilterBar';
import { EntrySummary } from '@lab/components/entry-context/EntrySummary';
import { EntryTokenDetail } from '@lab/components/entry-context/EntryTokenDetail';
import { compileFilter, rollupByToken, type AxisCondition } from '@lab/lib/entryContext/analysis';
import { entryKey, type EntryGroupBy, type EntryRow } from '@lab/lib/entryContext/types';
import { useGetEntryContextQuery, useGetTraderTokensQuery } from '@lab/store/labEndpoints';

const DAY_MS = 86_400_000;
const CUSTOM_PRESET = 'custom';
const PRESETS = [
  { value: '1', label: '1 day' },
  { value: '3', label: '3 days' },
  { value: '7', label: '7 days' },
  { value: '14', label: '14 days' },
  { value: '30', label: '30 days' },
  { value: CUSTOM_PRESET, label: 'Custom', description: 'Exact from → to' },
] as const;

const GROUP_BY_OPTIONS: { value: EntryGroupBy; label: string; title: string }[] = [
  { value: 'exact', label: 'Exact ix shape', title: 'The full ordered ix_labels sequence' },
  { value: 'template', label: 'Template', title: 'The coarse grain program|CU|ATA|N|S|F' },
  { value: 'program', label: 'Program', title: "The transaction's main program" },
];

const COLUMN_GROUP_LABELS: Record<string, string> = { entry_ctx: 'Entry context' };
const EMPTY_ENTRIES: EntryRow[] = [];
const EMPTY_ROWS: TraderTokenRow[] = [];

const shortAddr = (a: string) => `${a.slice(0, 4)}…${a.slice(-4)}`;

/** Persisted draft (`mt:form.entryContext`). */
interface EntryForm {
  wallet: string;
  days: string;
  from: string;
  to: string;
  windowSecs: string;
  groupBy: EntryGroupBy;
  filters: AxisCondition[];
  onlyPassing: boolean;
}

const DEFAULT_FORM: EntryForm = {
  wallet: '',
  days: '7',
  from: '',
  to: '',
  windowSecs: '30',
  groupBy: 'exact',
  // The finding this page was built for: the target made over half the buys.
  filters: [{ axis: 'tx_share', cond: '>50' }],
  onlyPassing: true,
};

/** A committed query — set on Analyze only, so typing never refetches. */
interface EntryQuery {
  wallet: string;
  from: string;
  to: string;
  windowSecs: number;
  groupBy: EntryGroupBy;
}

/**
 * **Entry Context** — what the tape looked like in the `W` seconds before each of a
 * trader's buys, read under one target tag (the flow lens' set, narrowed by its
 * chips), with the same read one `W` earlier as the control.
 *
 * Two sections over the same filtered entries:
 * - **Summary**: how many entries clear the filter, the target share's
 *   distribution against its control window, and which structures fill the
 *   windows.
 * - **Tokens**: the tokens with passing entries; picking one opens its chart with
 *   every window drawn, his buys on it, and the picked window's breakdown.
 *
 * Every per-entry number is an axis in `@lab/lib/entryContext/axes.ts`; the filter,
 * columns and summary render from that list. The numbers are the engine's
 * `m_flow` reads over the window (`entry_context.rs`), his own trades excluded.
 */
export function EntryContextPage() {
  const { timezone } = useTimezone();
  const [form, setForm] = useLocalStorage<EntryForm>(STORAGE_KEYS.entryContextConfig, DEFAULT_FORM);
  const f = { ...DEFAULT_FORM, ...form };
  const patch = useCallback(
    (p: Partial<EntryForm>) => setForm((prev) => ({ ...DEFAULT_FORM, ...prev, ...p })),
    [setForm],
  );
  const [query, setQuery] = useState<EntryQuery | null>(null);
  const [inspected, setInspected] = useState<string | null>(null);

  const lens = useTraderFlowLens(query?.wallet ?? null);
  const profileWallets = useProfileWallets();
  const isCustom = f.days === CUSTOM_PRESET;

  const run = (walletOverride?: string) => {
    const wallet = (walletOverride ?? f.wallet).trim();
    if (!wallet) return;
    const days = Math.max(1, Number.parseInt(f.days, 10) || 7);
    const toUtc = (wall: string, bound: 'lower' | 'upper') => {
      const utc = datetimeLocalToUtcWallClock(wall, timezone, bound);
      return utc ? `${utc}Z` : '';
    };
    const from = isCustom && f.from ? toUtc(f.from, 'lower') : new Date(Date.now() - days * DAY_MS).toISOString();
    setInspected(null);
    setQuery({
      wallet,
      from,
      to: isCustom ? toUtc(f.to, 'upper') : '',
      windowSecs: Math.min(600, Math.max(1, Number(f.windowSecs) || 30)),
      groupBy: f.groupBy,
    });
  };

  // The target is the lens tag as it stands, so a chip click re-reads the page the
  // same way it re-tints the charts.
  const tag = lens.value.tag;
  const ctx = useGetEntryContextQuery(
    query && tag
      ? {
          wallet: query.wallet,
          from: query.from,
          to: query.to || null,
          window_secs: query.windowSecs,
          group_by: query.groupBy,
          // Absent, not null: the engine's tag parser reads a present `side` as a
          // side and refuses null.
          tag: {
            match: tag.match,
            ...(tag.side ? { side: tag.side } : {}),
            ...(tag.sticky ? { sticky: true } : {}),
          },
        }
      : skipToken,
  );
  const tokens = useGetTraderTokensQuery(
    query ? { wallet: query.wallet, days: 1, limit: 0, from: query.from, to: query.to, with: [] } : skipToken,
  );

  const entries = ctx.data?.entries ?? EMPTY_ENTRIES;
  const windowSecs = ctx.data?.window_secs ?? query?.windowSecs ?? 30;
  const filter = useMemo(() => compileFilter(f.filters), [f.filters]);
  const passingEntries = useMemo(() => entries.filter(filter.pass), [entries, filter]);
  const passing = useMemo(() => new Set(passingEntries.map(entryKey)), [passingEntries]);
  const rollup = useMemo(() => rollupByToken(entries, passing), [entries, passing]);
  const tokensPassing = useMemo(
    () => [...rollup.values()].filter((r) => r.passing > 0).length,
    [rollup],
  );

  const tokenRows = tokens.data ?? EMPTY_ROWS;
  const tableRows = useMemo(
    () =>
      tokenRows.filter((r) => {
        const roll = rollup.get(r.mint_address);
        return roll != null && (!f.onlyPassing || roll.passing > 0);
      }),
    [tokenRows, rollup, f.onlyPassing],
  );

  const columns = useMemo(() => {
    const base = tokenColumns() as unknown as ColumnDef<TraderTokenRow>[];
    let lastIdentity = -1;
    base.forEach((c, i) => {
      if (c.group === 'identity') lastIdentity = i;
    });
    const at = (r: TraderTokenRow) => rollup.get(r.mint_address);
    const ctxCols: ColumnDef<TraderTokenRow>[] = [
      {
        key: 'ec_entries',
        label: 'Buys',
        group: 'entry_ctx',
        tooltip: 'His buy transactions on this token in the range.',
        render: (r) => at(r)?.entries ?? '-',
        sortValue: (r) => at(r)?.entries ?? null,
        searchValue: () => '',
        filterNumber: (r) => at(r)?.entries ?? null,
      },
      {
        key: 'ec_passing',
        label: 'Passing',
        group: 'entry_ctx',
        tooltip: 'Of those, the buys that clear every filter line.',
        render: (r) => at(r)?.passing ?? '-',
        sortValue: (r) => at(r)?.passing ?? null,
        searchValue: () => '',
        filterNumber: (r) => at(r)?.passing ?? null,
      },
      {
        key: 'ec_best_tx',
        label: 'Best tag tx %',
        group: 'entry_ctx',
        tooltip: 'The highest target tx share among this token\'s passing buys.',
        render: (r) => {
          const v = at(r)?.bestTxShare;
          return v == null ? '-' : `${v.toFixed(0)}%`;
        },
        sortValue: (r) => at(r)?.bestTxShare ?? null,
        searchValue: () => '',
        filterNumber: (r) => at(r)?.bestTxShare ?? null,
      },
    ];
    return [...base.slice(0, lastIdentity + 1), ...ctxCols, ...base.slice(lastIdentity + 1)];
  }, [rollup]);

  const inspectedEntries = useMemo(
    () => (inspected ? entries.filter((e) => e.mint_address === inspected) : EMPTY_ENTRIES),
    [entries, inspected],
  );

  const loading = ctx.isFetching || tokens.isFetching;
  const error = apiErrorMessage(ctx.error ?? tokens.error, 'Failed to load entry context');

  return (
    <FlowLensProvider value={lens.value}>
      <div className="p-4">
        <h2 className="text-lg font-extrabold text-text">Entry Context</h2>
        <p className="mt-0.5 text-xs text-text-dim">
          What the tape looked like in the seconds before each of a trader&apos;s buys, read
          under one target tag (the lens below), with the window before it as the control. His
          own trades are left out of every number.
        </p>

        <SectionDivider />

        <div className="mb-3 flex flex-wrap items-end gap-3">
          <label className="flex flex-col gap-1 text-[10px] font-bold uppercase tracking-widest text-text-dim">
            Wallet address
            <Input
              value={f.wallet}
              onChange={(e) => patch({ wallet: e.target.value })}
              onKeyDown={(e) => {
                if (e.key === 'Enter') run();
              }}
              placeholder="Solana base58 address"
              className="min-w-[420px] font-mono font-normal normal-case tracking-normal"
            />
          </label>
          {profileWallets.length > 0 && (
            <label className="flex flex-col gap-1 text-[10px] font-bold uppercase tracking-widest text-text-dim">
              Tracked wallet
              <Select
                value={profileWallets.some((w) => w.address === f.wallet) ? f.wallet : ''}
                onChange={(e) => {
                  if (!e.target.value) return;
                  patch({ wallet: e.target.value });
                  run(e.target.value);
                }}
                className="min-w-[200px] font-normal normal-case tracking-normal"
              >
                <option value="">Pick a profile wallet…</option>
                {profileWallets.map((w) => (
                  <option key={w.address} value={w.address}>
                    {w.label} · {shortAddr(w.address)}
                  </option>
                ))}
              </Select>
            </label>
          )}
          <label className="flex flex-col gap-1 text-[10px] font-bold uppercase tracking-widest text-text-dim">
            His buys in
            <DateTimeRangePicker
              aria-label="Buy range"
              size="sm"
              timeZone={timezone}
              emptyLabel="Pick a range"
              customPreset={CUSTOM_PRESET}
              presets={[...PRESETS]}
              value={{
                preset: f.days,
                from: isCustom
                  ? f.from
                  : utcIsoToDatetimeLocal(
                      new Date(Date.now() - (Number.parseInt(f.days, 10) || 7) * DAY_MS).toISOString(),
                      timezone,
                    ),
                to: isCustom ? f.to : '',
              }}
              onChange={({ preset, from, to }) =>
                preset === CUSTOM_PRESET
                  ? patch({ days: CUSTOM_PRESET, from, to })
                  : patch({ days: preset, from: '', to: '' })
              }
            />
          </label>
          <label
            className="flex flex-col gap-1 text-[10px] font-bold uppercase tracking-widest text-text-dim"
            title="Window W in seconds: the tape read is [his buy - W, his buy), and the control is the W before that."
          >
            Window (s)
            <Input
              type="number"
              min={1}
              max={600}
              value={f.windowSecs}
              onChange={(e) => patch({ windowSecs: e.target.value })}
              onKeyDown={(e) => {
                if (e.key === 'Enter') run();
              }}
              className="w-[90px] font-normal normal-case tracking-normal"
            />
          </label>
          <label className="flex flex-col gap-1 text-[10px] font-bold uppercase tracking-widest text-text-dim">
            Break down by
            <Select
              value={f.groupBy}
              onChange={(e) => patch({ groupBy: e.target.value as EntryGroupBy })}
              className="min-w-[150px] font-normal normal-case tracking-normal"
            >
              {GROUP_BY_OPTIONS.map((o) => (
                <option key={o.value} value={o.value} title={o.title}>
                  {o.label}
                </option>
              ))}
            </Select>
          </label>
          <IconButton
            variant="primary"
            size="lg"
            onClick={() => run()}
            disabled={loading || !f.wallet.trim()}
            label={loading ? 'Loading…' : 'Analyze'}
            title={loading ? 'Loading…' : 'Analyze'}
          >
            {loading ? <SpinnerIcon /> : <SearchIcon />}
          </IconButton>
        </div>

        <FlowLensBar lens={lens} wallet={query?.wallet ?? null} />
        {!tag && (
          <p className="mb-2 text-xs text-warning">
            Pick a pattern set in the lens above: its narrowed structures are the target tag every
            share is read against.
          </p>
        )}

        <EntryFilterBar
          lines={f.filters}
          onChange={(filters) => patch({ filters })}
          errors={filter.errors}
          windowSecs={windowSecs}
        />

        {error && <p className="mb-2 text-sm text-red">{error}</p>}
        {ctx.data?.truncated && (
          <p className="mb-2 text-xs text-warning">
            More than {ctx.data.max_entries} buys in this range: only the most recent{' '}
            {ctx.data.max_entries} are read. Narrow the range to read them all.
          </p>
        )}

        {query && ctx.data && (
          <>
            <h3 className="mb-2 text-xs font-bold uppercase tracking-widest text-text-dim">Summary</h3>
            <EntrySummary
              entries={entries}
              passing={passingEntries}
              tokensPassing={tokensPassing}
              windowSecs={windowSecs}
            />

            <SectionDivider />

            <div className="mb-2 flex items-center gap-3">
              <h3 className="text-xs font-bold uppercase tracking-widest text-text-dim">Tokens</h3>
              <label className="flex items-center gap-2 text-xs text-text-dim">
                <Checkbox
                  boxSize="sm"
                  checked={f.onlyPassing}
                  onChange={(e) => patch({ onlyPassing: e.target.checked })}
                />
                Only tokens with a passing buy
              </label>
            </div>
            <TokenTable
              columns={columns}
              rows={tableRows}
              existingKeys={ALL_TOKEN_INFO_KEYS}
              mintSetFilter
              charts
              searchable
              colFilters
              colToggle
              hoverable
              loading={loading}
              groupLabels={COLUMN_GROUP_LABELS}
              tableId="entry_context_tokens"
              resetKey={`${f.onlyPassing}|${JSON.stringify(f.filters)}`}
              highlightWallet={query.wallet}
              titleOf={(r) => r.symbol || r.name || shortAddr(r.mint_address)}
              selectedKey={inspected}
              onSelect={setInspected}
              emptyMessage="No tokens with a passing buy"
              flowPatternKeys={lens.keys}
            />
            {inspected && (
              <EntryTokenDetail
                key={inspected}
                mint={inspected}
                entries={inspectedEntries}
                passing={passing}
                wallet={query.wallet}
                windowSecs={windowSecs}
                flowPatternKeys={lens.keys}
              />
            )}
          </>
        )}
      </div>
    </FlowLensProvider>
  );
}
