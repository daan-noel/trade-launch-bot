import { useCallback, useEffect, useMemo, useState } from 'react';
import { skipToken } from '@reduxjs/toolkit/query/react';
import { DataTable } from 'components/table/DataTable';
import type { ColumnDef } from 'components/table/types';
import { tokenColumns } from 'components/tokens/tokenColumns';
import { TokenTable } from 'components/tokens/TokenTable';
import { ALL_TOKEN_INFO_KEYS } from 'components/tokens/sharedTokenColumns';
import { Accordion } from 'components/ui/Accordion';
import { Input } from 'components/ui/Input';
import { IconButton } from 'components/ui/IconButton';
import { SearchIcon, SpinnerIcon } from 'components/ui/icons';
import { SectionDivider } from 'components/ui/SectionDivider';
import { Tabs, TabsList, TabsPanel, TabsTrigger } from 'components/ui/Tabs';
import { inspectFromMint } from 'components/strategy/inspectTarget';
import { FlowLensProvider } from 'context/FlowLensContext';
import { useTimezone } from 'context/TimezoneContext';
import { useDebouncedValue } from 'hooks/useDebouncedValue';
import { useLocalStorage } from 'hooks/useLocalStorage';
import { ACCORDION_IDS, getTablePrefs, setTablePrefs, STORAGE_KEYS } from 'lib/storage';
import { apiErrorMessage } from 'store/apiSlice';
import type { TraderTokenRow } from 'types';
import { FlowLensBar } from '@lab/components/analysis/FlowLensBar';
import { FIELD_LABEL, TraderQueryInputs } from '@lab/components/analysis/TraderQueryInputs';
import {
  clampInt,
  CUSTOM_PRESET,
  DAY_MS,
  DEFAULT_DAYS,
  MAX_DAYS,
  shortAddr,
  wallClockToUtcIso,
} from '@lab/components/analysis/traderQuery';
import {
  DEFAULT_PROBE_WINDOW_SLOTS,
  PROBE_SLOT_KNOB,
  type PreEntryShow,
  type ProbeControlsModel,
} from '@lab/components/analysis/usePreEntryProbe';
import { useTraderFlowLens } from '@lab/components/analysis/useTraderFlowLens';
import { tradeOfBuy } from '@lab/components/analysis/walletPnlStats';
import { EntryDetail } from '@lab/components/entry-context/EntryDetail';
import { EntrySummary } from '@lab/components/entry-context/EntrySummary';
import { IdeaFilters } from '@lab/components/entry-context/IdeaFilters';
import { MarketScan } from '@lab/components/entry-context/MarketScan';
import { type TokenView, TokenViewToggle } from '@lab/components/entry-context/TokenViewToggle';
import { entryColumns } from '@lab/components/entry-context/entryColumns';
import { LazyLabTokenInspectModal } from '@lab/components/strategy/LazyLabTokenInspectModal';
import { entryLogic } from '@lab/lib/entryContext/logic';
import { hisTokens } from '@lab/lib/entryContext/overlap';
import { atDecision, entryVerdict, rollupByToken } from '@lab/lib/entryContext/analysis';
import { AXIS_FAMILIES, entryGroupLabels, groupsHiddenIn } from '@lab/lib/entryContext/axes';
import {
  entryKey,
  type EntryRow,
  type EntryContextRequest,
  type EntryTargetTag,
} from '@lab/lib/entryContext/types';
import { probeStateCounts, probeSummary, type PreEntryVerdict } from '@lab/lib/preEntryProbeTypes';
import { useGetEntryContextQuery, useGetTraderTokensQuery } from '@lab/store/labEndpoints';

/** Analysis window W, seconds: the server's own ceiling. */
const MAX_WINDOW_SECS = 600;
/** How long a probe-window edit settles before it re-reads. */
const PROBE_DEBOUNCE_MS = 350;

const TOKEN_GROUP_LABELS: Record<string, string> = { entry_ctx: 'His buys here' };
/** The buys table's filter row is this page's filter: open, and starting from the
 *  page's question (target over half the window's buy transactions). */
const DEFAULT_BUY_FILTERS: Record<string, string> = { tx_share: '>50' };
const BUYS_TABLE_ID = 'entry_context_buys';
/** Columns the buys table opens with hidden (all stay in the Columns panel): the
 *  SOL twins and side reads, so the answer columns fit on screen. */
const BUY_COLS_HIDDEN: Readonly<Record<string, boolean>> = {
  pe_sol: false,
  tag_buy_sol: false,
  buy_sol: false,
  ctl_sol_share: false,
  top_tx_share: false,
};
const EMPTY_ENTRIES: EntryRow[] = [];
const EMPTY_ROWS: TraderTokenRow[] = [];

/** The main tabs stand out from the family sub-tabs inside them: larger, boxed,
 *  each with its count, over a framed panel. */
const MAIN_TAB = 'px-6 py-3 text-sm';
const MAIN_TAB_COUNT = 'ml-2 rounded bg-white/6 px-1.5 py-0.5 font-mono text-[11px] font-normal text-text-mid';
const MAIN_PANEL = 'rounded-b-md rounded-tr-md border border-t-0 border-white/8 bg-bg-card/40 p-3';

/** Persisted draft (`mt:form.entryContext`). Two windows with two jobs: the
 *  ANALYSIS window (`windowSecs`, the shares and breakdown, applied on Analyze) and
 *  the PROBE window (`probeSlots`, "did it land in the slots before him", live). */
interface EntryForm {
  wallet: string;
  days: string;
  from: string;
  to: string;
  windowSecs: string;
  probeOn: boolean;
  show: PreEntryShow;
  probeSlots: number;
  /** Slots before his buy, shared by both signal methods. */
  slotsBefore: number;
  /** Slots after his buy. Ix pick drops a shape that buys again there. */
  slotsAfter: number;
  /** Reserve-match slippages, percent, comma-separated. Ignored while `slippageMode` is `auto`. */
  slippage: string;
  /** `auto` reads the slippage set from definite entries. `manual` sends `slippage`. */
  slippageMode: 'auto' | 'manual';
  /** Lamports a quote may miss the ceiling's curve SOL by. */
  slackLamports: number;
  minHits: number;
  minSol: number;
  /** The buys table's filters while the Filters switch is off; `null` = on. */
  pausedFilters: Record<string, string> | null;
}

const DEFAULT_FORM: EntryForm = {
  wallet: '',
  days: String(DEFAULT_DAYS),
  from: '',
  to: '',
  // On: "did the target land before his buy" is this page's pool.
  windowSecs: '30',
  probeOn: true,
  // The pool: his buy is after the target's signal.
  show: 'matched',
  probeSlots: DEFAULT_PROBE_WINDOW_SLOTS,
  slotsBefore: 2,
  slotsAfter: 1,
  slippage: '',
  slippageMode: 'auto',
  slackLamports: 1,
  minHits: 1,
  minSol: 0,
  pausedFilters: null,
};

/** A committed query — set on Analyze only, so typing never refetches. Saved, so
 *  the page comes back as it was left; a preset range stays the one Analyze fixed. */
interface EntryQuery {
  wallet: string;
  from: string;
  to: string;
  windowSecs: number;
}

/**
 * **Entry Context** — what the tape looked like in the `W` seconds before EVERY buy
 * of a trader, read under one target tag (the flow lens' set, narrowed by its
 * chips), with the window before it as the control.
 *
 * Built from the Trader Analysis parts, so the same feature reads the same on both
 * pages: the query row (`TraderQueryInputs`), the flow lens with the pre-entry
 * probe strip inside it (each buy is a `PreEntryVerdict`, so the probe's Show,
 * summary and columns are the ones Trader Analysis uses), the table filter row as
 * the filter, the analytics card chrome for the summary, and the shared token
 * table + inspect modal for the tokens.
 *
 * The buys table's filters are the idea. The summary counts his buys against that
 * idea, for the target's signal and for every structure. The buys and tokens on
 * screen are the ones that pass. The market section is a second token table: every
 * buy of the target in the range whose window passes those filters.
 * Every per-buy number is an axis (`@lab/lib/entryContext/axes.ts`); the numbers
 * are the engine's `m_flow` reads over the window (`entry_context.rs`), his own
 * trades excluded.
 */
export function EntryContextPage() {
  const { timezone } = useTimezone();
  const [form, setForm] = useLocalStorage<EntryForm>(STORAGE_KEYS.entryContextConfig, DEFAULT_FORM);
  const f = { ...DEFAULT_FORM, ...form };
  const patch = useCallback(
    (p: Partial<EntryForm>) => setForm((prev) => ({ ...DEFAULT_FORM, ...prev, ...p })),
    [setForm],
  );
  const [query, setQuery] = useLocalStorage<EntryQuery | null>(STORAGE_KEYS.entryContextQuery, null);
  const [inspected, setInspected] = useState<{ mint: string; symbol?: string | null } | null>(null);
  // The buys table's filtered cohort (pre-pagination), reported by the table.
  const [tableRows, setTableRows] = useState<EntryRow[] | null>(null);
  // The buys table's filters in force: the logic the summary tests.
  // His entries | Market. Both panels stay mounted: the market reads the buys table's filters.
  const [tab, setTab] = useState<'his' | 'market'>('his');
  // Tokens passing the filters in the last market scan, for the Market tab's label.
  const [marketTokens, setMarketTokens] = useState<number | null>(null);
  // The buys table's sub-tab: which idea family's columns it shows.
  const [family, setFamily] = useState(AXIS_FAMILIES[0].key);
  const hiddenGroups = useMemo(() => groupsHiddenIn(family), [family]);
  const filtersOn = f.pausedFilters == null;
  const [buyFilters, setBuyFilters] = useState<Readonly<Record<string, string>>>(
    filtersOn ? DEFAULT_BUY_FILTERS : {},
  );
  // Bumped by an edit in the Filters section: the buys table remounts and reads the
  // edited filters from its saved prefs.
  const [buysTableVersion, setBuysTableVersion] = useState(0);
  const applyFilters = useCallback((next: Readonly<Record<string, string>>) => {
    setBuyFilters(next);
    setTablePrefs(BUYS_TABLE_ID, { ...getTablePrefs(BUYS_TABLE_ID), colFilters: { ...next } });
    setBuysTableVersion((v) => v + 1);
  }, []);
  // Off parks the filters in the form and clears the table's; on puts them back.
  const toggleFilters = (on: boolean) => {
    if (on === filtersOn) return;
    if (on) applyFilters(f.pausedFilters ?? {});
    else applyFilters({});
    patch({ pausedFilters: on ? null : { ...buyFilters } });
  };
  const editFilter = (key: string, text: string) => {
    const next = { ...(f.pausedFilters ?? buyFilters) };
    if (text) next[key] = text;
    else delete next[key];
    if (filtersOn) applyFilters(next);
    else patch({ pausedFilters: next });
  };
  // Pool / Pass filters: which of his tokens the token table lists.
  const [tokenView, setTokenView] = useState<TokenView>('pass');

  const lens = useTraderFlowLens(query?.wallet ?? null);
  const tag = lens.value.tag;
  // The selected IXs in words. A set's name is a label, never who made the buys.
  const setWords = tag ? `the pattern set "${tag.name}"` : 'no pattern set';

  const run = (walletOverride?: string) => {
    const wallet = (walletOverride ?? f.wallet).trim();
    if (!wallet) return;
    const isCustom = f.days === CUSTOM_PRESET;
    const days = clampInt(f.days, DEFAULT_DAYS, 1, MAX_DAYS);
    setInspected(null);
    setQuery({
      wallet,
      from:
        isCustom && f.from
          ? wallClockToUtcIso(f.from, timezone, 'lower')
          : new Date(Date.now() - days * DAY_MS).toISOString(),
      to: isCustom ? wallClockToUtcIso(f.to, timezone, 'upper') : '',
      windowSecs: Math.min(MAX_WINDOW_SECS, Math.max(1, Number(f.windowSecs) || 30)),
    });
  };

  const slotsBefore = useDebouncedValue(
    Math.min(50, Math.max(0, Math.round(f.slotsBefore))),
    PROBE_DEBOUNCE_MS,
  );
  const slotsAfter = useDebouncedValue(
    Math.min(50, Math.max(0, Math.round(f.slotsAfter))),
    PROBE_DEBOUNCE_MS,
  );
  const slackLamports = useDebouncedValue(Math.max(0, Math.round(f.slackLamports)), PROBE_DEBOUNCE_MS);
  const autoSlippage = f.slippageMode !== 'manual';
  const slippageRaw = useDebouncedValue(f.slippage, PROBE_DEBOUNCE_MS);
  const slippagePct = useMemo(() => parseSlippagePct(slippageRaw), [slippageRaw]);
  const probeSlots = useDebouncedValue(
    Math.min(PROBE_SLOT_KNOB.max, Math.max(PROBE_SLOT_KNOB.min, Math.round(f.probeSlots))),
    PROBE_DEBOUNCE_MS,
  );
  // No set picked still reads every window and its breakdown; only the target
  // columns need a tag.
  // The target as the server takes it, built once for the buys read and a buy's
  // range read. Absent, not null: the engine's tag parser refuses a null side.
  const targetTag = useMemo<EntryTargetTag | undefined>(
    () =>
      tag
        ? {
            match: tag.match,
            ...(tag.side ? { side: tag.side } : {}),
            ...(tag.sticky ? { sticky: true as const } : {}),
          }
        : undefined,
    [tag],
  );
  // The committed read: his buys. The market scan is its own section.
  const ctxRequest = useMemo<EntryContextRequest | null>(
    () =>
      query
        ? {
            wallet: query.wallet,
            from: query.from,
            to: query.to || null,
            window_secs: query.windowSecs,
            probe_slots: probeSlots,
            slots_before: slotsBefore,
            slots_after: slotsAfter,
            ...(autoSlippage || slippagePct.length === 0 ? {} : { slippage_pct: slippagePct }),
            slack_lamports: slackLamports,
            ...(targetTag ? { tag: targetTag } : {}),
          }
        : null,
    [query, probeSlots, slotsBefore, slotsAfter, autoSlippage, slippagePct, slackLamports, targetTag],
  );
  const ctx = useGetEntryContextQuery(ctxRequest ?? skipToken);
  const derivedPct = ctx.data?.slippage_pct;
  const resolvedSlippage = autoSlippage ? (derivedPct ?? []) : slippagePct;
  const tokens = useGetTraderTokensQuery(
    query ? { wallet: query.wallet, days: 1, limit: 0, from: query.from, to: query.to, with: [] } : skipToken,
  );
  // Pool buys are read behind their signal, as the market's buys are (`atDecision`).
  const entries = useMemo(
    () => atDecision(ctx.data?.entries ?? EMPTY_ENTRIES, { minHits: f.minHits, minSol: f.minSol }, f.probeOn),
    [ctx.data, f.minHits, f.minSol, f.probeOn],
  );
  const readWindow = ctx.data?.window_secs ?? query?.windowSecs ?? 30;

  // Each buy as the probe's verdict — the Trader Analysis probe's own shape.
  const verdicts = useMemo(() => {
    const t = { minHits: f.minHits, minSol: f.minSol };
    return new Map<EntryRow, PreEntryVerdict>(entries.map((e) => [e, entryVerdict(e, t)]));
  }, [entries, f.minHits, f.minSol]);

  // Show narrows the table's INPUT set; the probe summary counts every buy read.
  const probeRows = useMemo(
    () =>
      f.probeOn && f.show !== 'all' ? entries.filter((e) => verdicts.get(e)?.state === f.show) : entries,
    [entries, verdicts, f.probeOn, f.show],
  );
  // A new query starts from the unfiltered set. Nothing else resets it: the table
  // re-reports on every input change, and a parent reset would land after it.
  useEffect(() => setTableRows(null), [query]);
  const shown = tableRows ?? probeRows;
  const shownSet = useMemo(() => new Set(shown), [shown]);

  const probe: ProbeControlsModel = {
    on: f.probeOn,
    setOn: (probeOn) => patch({ probeOn }),
    show: f.show,
    setShow: (show) => patch({ show }),
    window: {
      ...PROBE_SLOT_KNOB,
      value: f.probeSlots,
      set: (probeSlots) => patch({ probeSlots }),
    },
    minHits: f.minHits,
    setMinHits: (minHits) => patch({ minHits }),
    minSol: f.minSol,
    setMinSol: (minSol) => patch({ minSol }),
    summary: ctx.data ? probeSummary([...verdicts.values()], 0) : null,
    counts: ctx.data ? probeStateCounts([...verdicts.values()], 0) : null,
    loading: ctx.isFetching,
    error: ctx.error ? apiErrorMessage(ctx.error, 'Failed to read the windows') : null,
    blocked: !query ? 'press Analyze' : !tag ? 'pick a pattern set above: it is the target' : null,
  };

  const tokenRows = tokens.data ?? EMPTY_ROWS;
  const symbolOf = useMemo(() => {
    const m = new Map(tokenRows.map((r) => [r.mint_address, r.symbol || r.name] as const));
    return (mint: string) => m.get(mint) || shortAddr(mint);
  }, [tokenRows]);

  const verdictOf = useMemo(
    () => (f.probeOn ? (e: EntryRow) => verdicts.get(e) : null),
    [f.probeOn, verdicts],
  );
  const signaled = useCallback((e: EntryRow) => verdicts.get(e)?.state === 'matched', [verdicts]);
  // His round trip each buy belongs to, read off the token rows already loaded.
  const tradeOf = useMemo(() => {
    if (!tokens.data) return null;
    const byMint = new Map(tokens.data.map((r) => [r.mint_address, r] as const));
    return (e: EntryRow) => {
      const r = byMint.get(e.mint_address);
      return r ? tradeOfBuy(r, e.slot, e.tx_index) : null;
    };
  }, [tokens.data]);
  const buyColumns = useMemo(
    () => entryColumns(readWindow, symbolOf, verdictOf, probeSlots, tradeOf),
    [readWindow, symbolOf, verdictOf, probeSlots, tradeOf],
  );
  const logic = useMemo(
    () => entryLogic(buyFilters, buyColumns, verdictOf, f.show),
    [buyFilters, buyColumns, verdictOf, f.show],
  );
  // Each token he bought, by how his buys on it sit against the pool and the filters.
  const his = useMemo(() => hisTokens(entries, logic, signaled, f.probeOn), [entries, logic, signaled, f.probeOn]);
  // The Filters section's chips: the parked filters while the switch is off.
  const chipFilters = f.pausedFilters ?? buyFilters;
  const chips = useMemo(
    () => entryLogic(chipFilters, buyColumns, verdictOf, f.show).conditions.filter((c) => c.key in chipFilters),
    [chipFilters, buyColumns, verdictOf, f.show],
  );

  const buyGroupLabels = useMemo(
    () => entryGroupLabels(readWindow, probeSlots, slotsBefore),
    [readWindow, probeSlots, slotsBefore],
  );

  const rollup = useMemo(() => rollupByToken(entries, shownSet), [entries, shownSet]);
  // Pool = a buy in the buys table's input (the probe's Show); Pass filters = a buy on screen.
  const poolMints = useMemo(() => new Set(probeRows.map((e) => e.mint_address)), [probeRows]);
  const poolTokenRows = useMemo(
    () => tokenRows.filter((r) => poolMints.has(r.mint_address)),
    [tokenRows, poolMints],
  );
  const passTokenRows = useMemo(
    () => tokenRows.filter((r) => (rollup.get(r.mint_address)?.shown ?? 0) > 0),
    [tokenRows, rollup],
  );
  const tokenTableRows = tokenView === 'pool' ? poolTokenRows : passTokenRows;

  const tokenCols = useMemo(() => {
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
        tooltip: 'His buys on this token.',
        render: (r) => at(r)?.entries ?? '-',
        sortValue: (r) => at(r)?.entries ?? null,
        searchValue: () => '',
        filterNumber: (r) => at(r)?.entries ?? null,
      },
      {
        key: 'ec_shown',
        label: 'On screen',
        group: 'entry_ctx',
        tooltip: 'Of those, buys left after the filters.',
        render: (r) => at(r)?.shown ?? '-',
        sortValue: (r) => at(r)?.shown ?? null,
        searchValue: () => '',
        filterNumber: (r) => at(r)?.shown ?? null,
      },
      {
        key: 'ec_best_tx',
        label: 'Best target tx %',
        group: 'entry_ctx',
        tooltip: "Highest Target tx % among this token's buys on screen.",
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

  const loading = ctx.isFetching || tokens.isFetching;
  const error = apiErrorMessage(ctx.error ?? tokens.error, 'Failed to load entry context');

  return (
    <FlowLensProvider value={lens.value}>
      <div className="p-4">
        <h2 className="text-lg font-extrabold text-text">Entry Context</h2>
        <p className="mt-0.5 text-xs text-text-dim">
          A token pool (an ix structure traded just before the buy) and filters on the window before
          it, read on his buys and on the whole market. His own trades are never counted.
        </p>

        <SectionDivider />

        {/* Inputs */}
        <div className="mb-3 flex flex-wrap items-end gap-3">
          <TraderQueryInputs
            wallet={f.wallet}
            onWallet={(wallet) => patch({ wallet })}
            onPickWallet={(wallet) => {
              patch({ wallet });
              run(wallet);
            }}
            days={f.days}
            from={f.from}
            to={f.to}
            onRange={(r) => patch(r)}
            onEnter={() => run()}
            timezone={timezone}
          >
            <label
              className={FIELD_LABEL}
              title="Seconds before each buy to analyze (shares, counts, structures). Earlier = the same length just before that. Applied on Analyze. The probe has its own window, in slots."
            >
              Analysis window (s)
              <Input
                type="number"
                min={1}
                max={MAX_WINDOW_SECS}
                value={f.windowSecs}
                onChange={(e) => patch({ windowSecs: e.target.value })}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') run();
                }}
                className="w-[90px] font-normal normal-case tracking-normal"
              />
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
          </TraderQueryInputs>
        </div>

        {error && <p className="mb-2 text-sm text-red">{error}</p>}

        <SectionTitle>Token pool</SectionTitle>
        <FlowLensBar lens={lens} wallet={query?.wallet ?? null} probe={probe} />

        <SectionTitle>Signal</SectionTitle>
        <div className="mb-3 flex flex-wrap items-end gap-x-4 gap-y-2">
          <label className={FIELD_LABEL} title="Slots before his buy. Both methods read this window.">
            Slots before
            <Input
              type="number"
              min={0}
              max={50}
              value={f.slotsBefore}
              onChange={(e) => patch({ slotsBefore: Number(e.target.value) })}
              className="w-[72px] font-normal normal-case tracking-normal"
            />
          </label>
          <label
            className={FIELD_LABEL}
            title="Ix pick drops a buy whose exact instruction list also appears on a buy in this many slots after him."
          >
            Slot after
            <Input
              type="number"
              min={0}
              max={50}
              value={f.slotsAfter}
              onChange={(e) => patch({ slotsAfter: Number(e.target.value) })}
              className="w-[72px] font-normal normal-case tracking-normal"
            />
          </label>
          <label
            className={FIELD_LABEL}
            title="Read from definite entries: one print before him, quiet for 5 seconds, ceiling stored. A setting that two entries share is kept. Type to override; clear the box to read the entries again."
          >
            Slippage %
            <Input
              value={autoSlippage ? (derivedPct ?? []).join(', ') : f.slippage}
              placeholder="from entries"
              onChange={(e) => {
                const v = e.target.value;
                if (!v.trim()) patch({ slippageMode: 'auto', slippage: '' });
                else patch({ slippageMode: 'manual', slippage: v });
              }}
              className="w-[120px] font-normal normal-case tracking-normal"
            />
          </label>
          <label
            className={FIELD_LABEL}
            title="A candidate quote may differ from the ceiling's curve SOL by this many lamports and still match. 1 covers the rounding of the ceiling."
          >
            Slack, lamports
            <Input
              type="number"
              min={0}
              value={f.slackLamports}
              onChange={(e) => patch({ slackLamports: Number(e.target.value) })}
              className="w-[88px] font-normal normal-case tracking-normal"
            />
          </label>
          <p className="max-w-xl text-[11px] leading-snug text-text-dim">
            Ix pick uses every other buy in the slots before him: drop racers when a plain buy is present, drop a shape
            that returns in the slot after him, take the closest. Slippage is read from definite entries and filled in
            here. Reserve match names the one print in those slots, or, when several sit there, the print whose quote
            is within the slack of his ceiling at one of these slippages. A crowded buy stays blank on reserve until
            his max_sol_cost is stored.
          </p>
        </div>

        <SectionTitle>Filters</SectionTitle>
        <IdeaFilters
          conditions={chips}
          onEdit={editFilter}
          on={filtersOn}
          onToggle={toggleFilters}
          family={family}
          onFamily={setFamily}
        />

        <Tabs value={tab} onValueChange={(v) => setTab(v as 'his' | 'market')} variant="contained" className="mt-4">
          <TabsList>
            <TabsTrigger value="his" className={MAIN_TAB}>
              His entries
              {tableRows && <span className={MAIN_TAB_COUNT}>{tableRows.length.toLocaleString()} buys</span>}
            </TabsTrigger>
            <TabsTrigger value="market" className={MAIN_TAB}>
              Market
              {marketTokens != null && (
                <span className={MAIN_TAB_COUNT}>{marketTokens.toLocaleString()} tokens</span>
              )}
            </TabsTrigger>
          </TabsList>

          <TabsPanel value="his" keepMounted className={MAIN_PANEL}>
            {query && ctx.data && (
              <EntrySummary
                entries={entries}
                logic={logic}
                signaled={signaled}
                probeOn={f.probeOn}
                pool={`${setWords}, within ${probeSlots} slots`}
                ixFilters={filtersOn ? chips.filter((c) => c.needsIxs).length : 0}
                anyFilters={filtersOn ? chips.filter((c) => !c.needsIxs).length : 0}
              />
            )}

            {query && entries.length > 0 && (
              <>
                <Tabs value={family} onValueChange={setFamily} className="mb-2">
                  <TabsList>
                    {AXIS_FAMILIES.map((fam) => (
                      <TabsTrigger key={fam.key} value={fam.key} className="px-3 py-1.5 text-xs">
                        {fam.label}
                      </TabsTrigger>
                    ))}
                  </TabsList>
                </Tabs>
                <DataTable
                  key={buysTableVersion}
                  columns={buyColumns}
                  rows={probeRows}
                  rowKey={entryKey}
                  rowDetail={(e) => (
                    <EntryDetail
                      entry={e}
                      query={{
                        wallet: query.wallet,
                        windowSecs: readWindow,
                        probeSlots,
                        slotsBefore,
                        slotsAfter,
                        slippagePct: resolvedSlippage,
                        slackLamports,
                        tag: targetTag,
                      }}
                    />
                  )}
                  tableId={BUYS_TABLE_ID}
                  defaultCols={BUY_COLS_HIDDEN}
                  defaultSort={{ col: 'at', dir: 'desc' }}
                  searchable
                  colFilters={filtersOn}
                  defaultColFilters={DEFAULT_BUY_FILTERS}
                  colToggle
                  hoverable
                  loading={ctx.isFetching}
                  groupLabels={buyGroupLabels}
                  hiddenGroups={hiddenGroups}
                  resetKey={`${f.probeOn}|${f.show}`}
                  onFilteredRowsChange={setTableRows}
                  onColFiltersChange={setBuyFilters}
                  emptyMessage={
                    tag
                      ? 'No buys match the filters'
                      : 'No buys match the filters. The target columns need a pattern set picked in Target IXs above.'
                  }
                />
              </>
            )}

            {query && tokenRows.length > 0 && entries.length > 0 && (
              <>
                <SectionDivider />
                <Accordion
                  toggleLabel="Tokens"
                  header={
                    <TokenViewToggle
                      value={tokenView}
                      onChange={setTokenView}
                      pool={poolTokenRows.length}
                      pass={passTokenRows.length}
                    />
                  }
                  padding="sm"
                  bordered={false}
                  storageKey={ACCORDION_IDS.entryContextTokens}
                >
                  <TokenTable
                    columns={tokenCols}
                    rows={tokenTableRows}
                    existingKeys={ALL_TOKEN_INFO_KEYS}
                    mintSetFilter
                    charts
                    chartsDefaultOn
                    searchable
                    colFilters
                    colToggle
                    hoverable
                    loading={loading}
                    groupLabels={TOKEN_GROUP_LABELS}
                    tableId="entry_context_tokens"
                    resetKey={`${shown.length}|${tokenView}`}
                    highlightWallet={query.wallet}
                    titleOf={(r) => r.symbol || r.name || shortAddr(r.mint_address)}
                    selectedKey={inspected?.mint ?? null}
                    onSelect={(mint) => {
                      const row = mint ? tokenTableRows.find((r) => r.mint_address === mint) : null;
                      setInspected(mint ? { mint, symbol: row?.symbol } : null);
                    }}
                    emptyMessage={tokenView === 'pool' ? 'No tokens in the pool' : 'No tokens with a buy on screen'}
                    flowPatternKeys={lens.keys}
                  />
                </Accordion>
              </>
            )}
          </TabsPanel>

          <TabsPanel value="market" keepMounted className={MAIN_PANEL}>
            {query && ctxRequest ? (
              <>
                <MarketScan
                  scanRequest={ctxRequest}
                  logic={logic}
                  wallet={query.wallet}
                  flowPatternKeys={lens.keys}
                  hasTarget={!!tag}
                  pool={setWords}
                  his={his}
                  onPassTokens={setMarketTokens}
                />
              </>
            ) : (
              <p className="text-xs text-text-dim">Analyze a wallet first: the market scan uses its range and window.</p>
            )}
          </TabsPanel>
        </Tabs>

        {inspected && (
          <LazyLabTokenInspectModal
            target={inspectFromMint(inspected.mint, inspected.symbol)}
            titleSuffix="Token inspect"
            flowPatternKeys={lens.keys}
            onClose={() => setInspected(null)}
          />
        )}
      </div>
    </FlowLensProvider>
  );
}

/** Slippage percents from a comma-separated box. Blank leaves the reading to the entries. */
function parseSlippagePct(raw: string): number[] {
  return raw
    .split(/[,\s]+/)
    .map((s) => Number(s))
    .filter((n) => Number.isFinite(n) && n >= 0 && n < 500);
}

function SectionTitle({ children }: { children: string }) {
  return <h3 className="mb-1.5 text-[11px] font-bold uppercase tracking-wider text-text-dim">{children}</h3>;
}
