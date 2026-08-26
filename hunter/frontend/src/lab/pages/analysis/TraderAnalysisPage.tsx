import { useCallback, useEffect, useMemo, useState } from 'react';
import type { ColumnDef } from 'components/table/types';
import { tokenColumns } from 'components/tokens/tokenColumns';
import { TokenTable } from 'components/tokens/TokenTable';
import { ALL_TOKEN_INFO_KEYS } from 'components/tokens/sharedTokenColumns';
import { DateTimeRangePicker } from 'components/ui/DateTimeRangePicker';
import { IconButton } from 'components/ui/IconButton';
import { SearchIcon, SpinnerIcon } from 'components/ui/icons';
import { Input } from 'components/ui/Input';
import { CoTradeSummary } from '@lab/components/analysis/CoTradeSummary';
import { coTradeColumns } from '@lab/components/analysis/coTradeColumns';
import { FlowLensBar } from '@lab/components/analysis/FlowLensBar';
import { TraderChartCardExtra } from '@lab/components/analysis/TraderChartCardExtra';
import { useTraderFlowLens } from '@lab/components/analysis/useTraderFlowLens';
import { WalletAnalyticsPanel } from '@lab/components/analysis/WalletAnalyticsPanel';
import { filterTraderRowsByFocus } from '@lab/components/analysis/walletFocus';
import { walletTokenColumns } from '@lab/components/analysis/walletTokenColumns';
import { LazyLabTokenInspectModal } from '@lab/components/strategy/LazyLabTokenInspectModal';
import { inspectFromMint } from 'components/strategy/inspectTarget';
import type { PositionFocusLens } from 'lib/strategy/positionFocus';
import { Checkbox } from 'components/ui/Checkbox';
import { Select } from 'components/ui/Select';
import { SectionDivider } from 'components/ui/SectionDivider';
import { FlowLensProvider } from 'context/FlowLensContext';
import { useTimezone } from 'context/TimezoneContext';
import { datetimeLocalToUtcWallClock, utcIsoToDatetimeLocal } from 'utils/date';
import { apiErrorMessage } from 'store/apiSlice';
import { useGetTraderTokensQuery } from '@lab/store/labEndpoints';
import { useProfileWallets } from 'hooks/useProfileWallets';
import { useLocalStorage } from 'hooks/useLocalStorage';
import { STORAGE_KEYS } from 'lib/storage';
import type { ProfileWalletInfo } from 'components/token-price-chart/types';
import type { TraderTokenRow } from 'types';

// Look-back clamp mirrors the backend (`MAX_WINDOW_DAYS` in
// `lab/src/api/handlers/wallets.rs`), and bounds the custom range's SPAN too.
// Max tokens uses the zero-as-unbound sentinel (`0` / blank ⇒ every mint in the
// window) — same as mint-trades `limit<=0` and the rule editor's Max total.
const DEFAULT_DAYS = 7;
const DEFAULT_LIMIT = 0;
const MAX_DAYS = 90;
const DAY_MS = 86_400_000;

/** The picker's custom-range sentinel — `days` holds this instead of a day count
 *  while the window is an explicit `from`/`to` pair. */
const CUSTOM_PRESET = 'custom';

const TRADER_LOOKBACK_PRESETS = [
  { value: '1', label: '1 day' },
  { value: '3', label: '3 days' },
  { value: '7', label: '7 days' },
  { value: '14', label: '14 days' },
  { value: '30', label: '30 days' },
  { value: '60', label: '60 days' },
  { value: '90', label: '90 days' },
  {
    value: CUSTOM_PRESET,
    label: 'Custom',
    description: `Exact from → to, max ${MAX_DAYS}d span`,
  },
] as const;

/** A wall-clock `YYYY-MM-DDTHH:mm` in `tz` for an instant — the picker's wire
 *  shape. Seeds the popover draft from whatever window is active, so switching a
 *  day preset to Custom starts from that preset's bounds instead of blank. */
const msToWallClock = (ms: number, tz: string) =>
  utcIsoToDatetimeLocal(new Date(ms).toISOString(), tz);

/** The picker's wall-clock (project zone) → the UTC RFC3339 instant the API
 *  takes. `bound` keeps a DST-ambiguous hour inside the range (see
 *  `datetimeLocalToUtcWallClock`). Blank in ⇒ blank out (no bound). */
const wallClockToUtcIso = (wall: string, tz: string, bound: 'lower' | 'upper') => {
  const utc = datetimeLocalToUtcWallClock(wall, tz, bound);
  return utc ? `${utc}Z` : '';
};

/** Group header labels. Only the appended wallet groups are named — the shared
 *  token groups keep the blank header every other token table shows, so the two
 *  halves of the row read apart at a glance. */
const COLUMN_GROUP_LABELS: Record<string, string> = {
  wallet_pos: 'Position',
  wallet_curve: 'Bonding curve',
  co_trade: 'Co-trade',
};

/** Stable empty reference so derived memos don't recompute while loading. */
const EMPTY_ROWS: TraderTokenRow[] = [];

/** A committed query — only set on Analyze, so editing the inputs doesn't
 *  refetch mid-typing. */
interface TraderQuery {
  wallet: string;
  days: number;
  limit: number;
  /** Explicit window bounds as UTC RFC3339, `''` when the window is the rolling
   *  `days` one. `from` set ⇒ the backend ignores `days`; `to` alone anchors the
   *  rolling span to that instant instead of now. */
  from: string;
  to: string;
  /** Comparison wallets for the co-trade columns. Empty ⇒ the plain
   *  single-wallet page, and the backend skips its second query entirely. */
  with: string[];
}

/** What the row-count sentence says the numbers were read over. */
function windowLabel(q: TraderQuery, tz: string): string {
  const at = (iso: string) => utcIsoToDatetimeLocal(iso, tz).replace('T', ' ');
  if (q.from) return `${at(q.from)} → ${q.to ? at(q.to) : 'now'}`;
  if (q.to) return `the ${q.days}d up to ${at(q.to)}`;
  return `the last ${q.days}d`;
}

const shortAddr = (a: string) => `${a.slice(0, 4)}…${a.slice(-4)}`;

/** The tracked wallets grouped by their profile name, for the picker's optgroups.
 *  `mine`-profile wallets sort first so your own wallet is easy to reach. */
function groupByProfile(wallets: ProfileWalletInfo[]): { profileName: string; wallets: ProfileWalletInfo[] }[] {
  const order: string[] = [];
  const byName = new Map<string, ProfileWalletInfo[]>();
  for (const w of wallets) {
    const name = w.profileName ?? 'Untitled';
    let bucket = byName.get(name);
    if (!bucket) {
      bucket = [];
      byName.set(name, bucket);
      order.push(name);
    }
    bucket.push(w);
  }
  return order
    .map((profileName) => ({ profileName, wallets: byName.get(profileName)! }))
    .sort((a, b) => Number(b.wallets[0]?.isMine) - Number(a.wallets[0]?.isMine));
}

const clampInt = (raw: string, fallback: number, min: number, max: number) => {
  const n = parseInt(raw, 10);
  return Math.min(max, Math.max(min, Number.isFinite(n) ? n : fallback));
};

/** Parse Max tokens: blank / 0 / non-finite ⇒ 0 (unlimited); positive stays as asked. */
const parseLimit = (raw: string) => {
  const trimmed = raw.trim();
  if (!trimmed) return 0;
  const n = parseInt(trimmed, 10);
  if (!Number.isFinite(n) || n <= 0) return 0;
  return n;
};

/**
 * Trader Analysis — paste a wallet address and see every token it traded in the
 * look-back window as the standard full token table (client-side sort / filter /
 * search) EXTENDED with the wallet's own position and bonding-curve columns
 * (`@lab/components/analysis/walletTokenColumns.tsx` — entry/exit + their token
 * ages, hold, leg counts, SOL in/out, PnL, fee, and the curve progress it bought
 * into and sold at), a wallet-level PnL analytics deck (summary + interactive
 * charts with focus chips, re-derived from the table's current filtered cohort —
 * see `walletPnlStats.ts` / `walletFocus.ts`), and the shared `TokenTable` Charts
 * toggle for a per-token grid mirroring the current page. Each chart card repeats
 * the headline wallet stats so a card read on its own still says who did what.
 *
 * Every PnL figure is an avg-cost reconstruction over this per-mint grain, NOT a
 * true per-episode ledger (a wallet that re-entered a mint many times collapses
 * to one row) — see the backend `kernel::wallet_mint_pnl` doc comment.
 *
 * Scope caveat: only tokens this box ingests appear — a coin the wallet traded
 * that was never tracked won't show. Charts are lazily mounted when the toggle
 * is on.
 *
 * **Co-trade mode.** Naming comparison wallets ("Compare with") annotates every
 * row with which of them were also on that mint and where their entry sat
 * against the primary's — see `coTradeColumns.tsx` / `coTrade.ts`. The page stays
 * PRIMARY-shaped on purpose: the PnL deck, the flow lens (which excludes the
 * studied wallet from its own classification) and the wallet columns all answer
 * for one wallet, and the comparison set is purely additive on top. The mint set
 * is the primary's, so a comparison wallet's own tokens do not add rows.
 *
 * Ordering comes from the entry leg's `(slot, tx_index)`, never from a
 * timestamp: `block_time` is second-precision and ties across a whole slot,
 * which is exactly the resolution these wallets sit at. Read the summary
 * strip's coupling mix before the overlap count — two busy wallets share some
 * memecoins by chance alone.
 */
/** Persisted query knobs (`mt:form.traderAnalysis`). `days` is the look-back
 *  picker's preset — a day count as a string, or `CUSTOM_PRESET` when `from`/`to`
 *  (wall-clock in the project zone) drive the window instead; `limit` keeps the
 *  raw input so the blank / `0` "every mint" sentinel round-trips. */
interface TraderForm {
  wallet: string;
  days: string;
  limit: string;
  from: string;
  to: string;
  /** Comparison wallets for the co-trade read — the "which of this family was
   *  also here" set. Persisted with the rest of the draft: a family is studied
   *  over several sessions, and re-picking it every reload is the friction that
   *  stops the question being asked. */
  compare: string[];
  /** Show only tokens at least one comparison wallet also traded. A pure
   *  client-side narrowing of the same rows — toggling it never refetches. */
  coOnly: boolean;
}
const DEFAULT_FORM: TraderForm = {
  wallet: '',
  days: String(DEFAULT_DAYS),
  limit: String(DEFAULT_LIMIT),
  from: '',
  to: '',
  compare: [],
  coOnly: false,
};

export function TraderAnalysisPage() {
  const { timezone } = useTimezone();
  // One persisted draft for the query knobs: the same wallet is studied over the
  // same look-back across sessions, so a refresh must not clear the address or
  // silently reset the window / row cap the numbers were read under.
  const [form, setForm] = useLocalStorage<TraderForm>(
    STORAGE_KEYS.traderAnalysisConfig,
    DEFAULT_FORM,
  );
  // `from`/`to` default in-place: a draft persisted before the custom range
  // existed has neither field, and an `undefined` would reach the picker as a
  // controlled-input value.
  const {
    wallet: walletInput,
    days: daysInput,
    limit: limitInput,
    from: fromInput = '',
    to: toInput = '',
    compare = [],
    coOnly = false,
  } = form;
  const patch = useCallback(
    (p: Partial<TraderForm>) => setForm((prev) => ({ ...prev, ...p })),
    [setForm],
  );
  const [query, setQuery] = useState<TraderQuery | null>(null);
  // Table column/search cohort (pre-pagination). Pinned when focus activates so
  // the analytics deck's parent base doesn't collapse to the focused slice
  // (same pin pattern as Sweep drill-in).
  const [filteredRows, setFilteredRows] = useState<TraderTokenRow[]>(EMPTY_ROWS);
  const [focus, setFocus] = useState<PositionFocusLens[]>([]);
  const [cohortPinned, setCohortPinned] = useState<TraderTokenRow[] | null>(null);
  // Row / chart-card select → lab token detail modal (mint-only; no fill markers).
  const [inspected, setInspected] = useState<{ mint: string; symbol?: string | null } | null>(
    null,
  );

  // Tracked wallets from the shared profiles cache (same SSOT the chart markers
  // read) — a picker to fill the wallet input from a saved profile wallet.
  // The page-wide flow lens: an analysis-owned ix_labels pattern set standing in
  // for the fingerprint these tokens don't have, so every chart can draw its
  // vol/non-vol split and every candle's trades table gets its Vol column.
  // Follows the COMMITTED wallet, not the input box — the exclusion it applies
  // must match the wallet the rows on screen belong to.
  const lens = useTraderFlowLens(query?.wallet ?? null);

  const profileWallets = useProfileWallets();
  const profileGroups = useMemo(() => groupByProfile(profileWallets), [profileWallets]);
  // Reflect the picker's selection only while the input still holds a known
  // tracked address; typing a custom address falls back to the placeholder.
  const pickedWallet = profileWallets.some((w) => w.address === walletInput) ? walletInput : '';
  // Co-trade surfaces follow the COMMITTED query, never the draft: the columns
  // read `co_traders`, which only the rows fetched under that query carry.
  const comparisonActive = (query?.with.length ?? 0) > 0;
  // Everything tracked except the primary — the comparison picker's menu.
  const comparisonChoices = useMemo(
    () => profileWallets.filter((w) => w.address !== walletInput && !compare.includes(w.address)),
    [profileWallets, walletInput, compare],
  );
  const labelFor = useCallback(
    (address: string) =>
      profileWallets.find((w) => w.address === address)?.label ?? shortAddr(address),
    [profileWallets],
  );

  const {
    data,
    isFetching,
    error: rawError,
  } = useGetTraderTokensQuery(
    query ?? { wallet: '', days: DEFAULT_DAYS, limit: DEFAULT_LIMIT, from: '', to: '', with: [] },
    { skip: !query },
  );
  const error = apiErrorMessage(rawError, 'Failed to load trader tokens');
  const rows = data ?? EMPTY_ROWS;

  // New Analyze (wallet / days / limit) drops focus — a stale day lens from the
  // previous window would silently empty the table.
  useEffect(() => {
    setFocus([]);
    setCohortPinned(null);
    setFilteredRows(EMPTY_ROWS);
    setInspected(null);
  }, [query]);

  // The standard shared token columns (SSOT — unchanged, so every other token
  // table keeps the same layout) with the wallet's own position + bonding-curve
  // columns SPLICED IN directly after the identity block. The splice happens
  // HERE, never inside `tokenColumns()`: a wallet-only field must not leak into
  // All Tokens or the strategy tables.
  //
  // Position sits ahead of the token's own activity/price/market columns because
  // this page answers "what did this wallet do", and the answer should be
  // readable without scrolling past twenty token fields first. Rows arrive
  // recent-first from the backend, which is the default order.
  const columns = useMemo(() => {
    const base = tokenColumns() as unknown as ColumnDef<TraderTokenRow>[];
    // One past the LAST identity column (symbol / name / mint / creator /
    // create_tx). A `-1` from `lastIndexOf` becomes 0, putting the wallet block
    // first — the right fallback if the shared set ever drops that group.
    let lastIdentity = -1;
    base.forEach((c, i) => {
      if (c.group === 'identity') lastIdentity = i;
    });
    const at = lastIdentity + 1;
    // Co-trade columns ride directly behind the wallet's own block — the two
    // read as one question ("what did he do here, and who else was here") — and
    // only exist while the COMMITTED query names comparison wallets, so the
    // single-wallet page keeps exactly the layout it had.
    const co = comparisonActive ? coTradeColumns(profileWallets) : [];
    return [...base.slice(0, at), ...walletTokenColumns(), ...co, ...base.slice(at)];
  }, [comparisonActive, profileWallets]);

  const isCustomWindow = daysInput === CUSTOM_PRESET;
  // Seed instant for the day presets. Recomputed only when the preset (or zone)
  // changes rather than every render — it feeds a draft and a trigger hint, not
  // the query, which resolves its own `now` at Analyze time.
  const presetFromWallClock = useMemo(
    () =>
      isCustomWindow
        ? ''
        : msToWallClock(
            Date.now() - clampInt(daysInput, DEFAULT_DAYS, 1, MAX_DAYS) * DAY_MS,
            timezone,
          ),
    [isCustomWindow, daysInput, timezone],
  );

  const run = (walletOverride?: string) => {
    const wallet = (walletOverride ?? walletInput).trim();
    if (!wallet) return;
    setQuery({
      wallet,
      // Custom with both bounds blank degrades to the rolling default rather
      // than an empty window, so Analyze always answers something.
      days: isCustomWindow ? DEFAULT_DAYS : clampInt(daysInput, DEFAULT_DAYS, 1, MAX_DAYS),
      limit: parseLimit(limitInput),
      from: isCustomWindow ? wallClockToUtcIso(fromInput, timezone, 'lower') : '',
      to: isCustomWindow ? wallClockToUtcIso(toInput, timezone, 'upper') : '',
      // The primary can't compare against itself; dropping it here keeps the
      // wire list and the summary strip agreeing on who is being compared.
      with: compare.filter((a) => a !== wallet),
    });
  };

  // Picking a tracked wallet fills the input and analyzes it immediately (state
  // is async, so pass the address straight to `run`).
  const handlePickWallet = (address: string) => {
    if (!address) return;
    patch({ wallet: address });
    run(address);
  };

  // Stable identity so DataTable's memoized pageRows/processed effects don't churn.
  const handleFilteredRows = useCallback(
    (r: TraderTokenRow[]) => {
      // Only track column/search filters while unfocused — once focus is on, the
      // table's row set is already narrowed and would collapse the chart base.
      if (focus.length === 0) setFilteredRows(r);
    },
    [focus.length],
  );

  const onFocusChange = useCallback(
    (next: PositionFocusLens[]) => {
      if (next.length > 0 && focus.length === 0) {
        // Prefer the table's column/search cohort; fall back to the full API
        // set if DataTable hasn't reported filtered rows yet.
        setCohortPinned(filteredRows.length > 0 ? filteredRows : rows);
      }
      if (next.length === 0) setCohortPinned(null);
      setFocus(next);
    },
    [focus.length, filteredRows, rows],
  );

  // Prefer the live table-filter report; fall back to the full API set so the
  // deck paints before DataTable's first onFilteredRowsChange.
  const tableFilterCohort = cohortPinned ?? (filteredRows.length > 0 ? filteredRows : rows);
  const focusOpts = useMemo(() => ({ timeZone: timezone }), [timezone]);

  // Focus narrows the table's input set (client-side); column filters still
  // apply inside DataTable on top of this. When unfocused, pass the full API set.
  const rowsForTable = useMemo(() => {
    const focused = focus.length === 0 ? rows : filterTraderRowsByFocus(rows, focus, focusOpts);
    // Co-traded-only is the last narrowing, applied to whatever focus left. The
    // backend already answered for every token, so this is a `length` test — the
    // toggle is instant and costs no query.
    if (!coOnly || !comparisonActive) return focused;
    return focused.filter((r) => r.co_traders.length > 0);
  }, [rows, focus, focusOpts, coOnly, comparisonActive]);

  return (
    <FlowLensProvider value={lens.value}>
    <div className="p-4">
      <h2 className="text-lg font-extrabold text-text">Trader Analysis</h2>
      <p className="mt-0.5 text-xs text-text-dim">
        Every token a wallet traded in the window — full token table (sort / filter /
        search) plus a synced charts grid with its buys/sells spotlighted. Recent trade
        first. Only tokens this box ingests appear. Add comparison wallets to see which
        of them were on the same mints, and who entered first.
      </p>

      <SectionDivider />

      {/* Inputs */}
      <div className="mb-3 flex flex-wrap items-end gap-3">
        <label className="flex flex-col gap-1 text-[10px] font-bold uppercase tracking-widest text-text-dim">
          Wallet address
          <Input
            value={walletInput}
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
              value={pickedWallet}
              onChange={(e) => handlePickWallet(e.target.value)}
              className="min-w-[200px] font-normal normal-case tracking-normal"
            >
              <option value="">Pick a profile wallet…</option>
              {profileGroups.map((group) => (
                <optgroup key={group.profileName} label={group.profileName}>
                  {group.wallets.map((w) => (
                    <option key={w.address} value={w.address}>
                      {shortAddr(w.address)}
                    </option>
                  ))}
                </optgroup>
              ))}
            </Select>
          </label>
        )}
        <label className="flex flex-col gap-1 text-[10px] font-bold uppercase tracking-widest text-text-dim">
          Look-back
          <DateTimeRangePicker
            aria-label="Look-back window"
            size="sm"
            timeZone={timezone}
            emptyLabel="Pick a range"
            customPreset={CUSTOM_PRESET}
            presets={[...TRADER_LOOKBACK_PRESETS]}
            // A day preset still hands the picker its resolved bounds, so the
            // trigger reads "7 days · 08/18 → now" and switching to Custom opens
            // on that window instead of a blank calendar. Only `from` is seeded:
            // a rolling preset's upper bound IS now, which the trigger renders.
            value={{
              preset: daysInput,
              from: isCustomWindow ? fromInput : presetFromWallClock,
              to: isCustomWindow ? toInput : '',
            }}
            onChange={({ preset, from, to }) =>
              preset === CUSTOM_PRESET
                ? patch({ days: CUSTOM_PRESET, from, to })
                : patch({ days: preset, from: '', to: '' })
            }
          />
        </label>
        <label className="flex flex-col gap-1 text-[10px] font-bold uppercase tracking-widest text-text-dim">
          Max tokens
          <Input
            type="number"
            min={0}
            blankZero
            placeholder="∞"
            value={limitInput}
            onChange={(e) => patch({ limit: e.target.value })}
            onKeyDown={(e) => {
              if (e.key === 'Enter') run();
            }}
            className="w-[110px] font-normal normal-case tracking-normal"
            title="Blank or 0 = every token in the window"
          />
        </label>
        {profileWallets.length > 0 && (
          <label className="flex flex-col gap-1 text-[10px] font-bold uppercase tracking-widest text-text-dim">
            Compare with
            <Select
              value=""
              onChange={(e) => {
                const address = e.target.value;
                if (address) patch({ compare: [...compare, address] });
              }}
              disabled={comparisonChoices.length === 0}
              className="min-w-[200px] font-normal normal-case tracking-normal"
              title="Wallets to check against this one. Their entries are measured against the primary's on the tape, and each gets its own color on every chart."
            >
              <option value="">
                {comparisonChoices.length === 0 ? 'No other tracked wallets' : 'Add a wallet…'}
              </option>
              {comparisonChoices.map((w) => (
                <option key={w.address} value={w.address}>
                  {w.label} · {shortAddr(w.address)}
                </option>
              ))}
            </Select>
          </label>
        )}
        <IconButton
          variant="primary"
          size="lg"
          onClick={() => run()}
          disabled={isFetching || !walletInput.trim()}
          label={isFetching ? 'Loading…' : 'Analyze'}
          title={isFetching ? 'Loading…' : 'Analyze'}
        >
          {isFetching ? <SpinnerIcon /> : <SearchIcon />}
        </IconButton>
      </div>

      {/* The picked comparison set, still a DRAFT until Analyze — the chips sit
          under the inputs rather than in the summary strip, which reports what
          the committed query actually returned. */}
      {compare.length > 0 && (
        <div className="mb-3 flex flex-wrap items-center gap-2 text-[11px]">
          <span className="text-[10px] font-bold uppercase tracking-widest text-text-dim">
            Compare with
          </span>
          {compare.map((address) => {
            const info = profileWallets.find((w) => w.address === address);
            return (
              <span
                key={address}
                className="inline-flex items-center gap-1.5 rounded border border-white/10 bg-white/5 px-1.5 py-0.5"
                title={address}
              >
                <span
                  className="size-1.5 rounded-full"
                  style={{ background: info?.color ?? '#888' }}
                />
                <span className="text-text">{labelFor(address)}</span>
                <button
                  type="button"
                  className="text-text-dim hover:text-red"
                  onClick={() => patch({ compare: compare.filter((a) => a !== address) })}
                  aria-label={`Remove ${labelFor(address)} from the comparison`}
                >
                  ×
                </button>
              </span>
            );
          })}
          <button
            type="button"
            className="text-text-dim underline-offset-2 hover:text-text hover:underline"
            onClick={() => patch({ compare: [] })}
          >
            clear
          </button>
          <span className="text-text-dim">
            {comparisonActive ? '' : '— press Analyze to apply'}
          </span>
        </div>
      )}

      {error && <p className="mb-2 text-sm text-red">{error}</p>}

      <FlowLensBar lens={lens} wallet={query?.wallet ?? null} />

      {query && !isFetching && !error && (
        <p className="mb-3 text-xs text-text-dim">
          {rows.length === 0
            ? `No tracked tokens traded by this wallet in ${windowLabel(query, timezone)}.`
            : `${rows.length} token${rows.length === 1 ? '' : 's'} traded by ${shortAddr(query.wallet)} in ${windowLabel(query, timezone)}`}
        </p>
      )}

      {/* Co-trade headline for the committed query. Reads the table's current
          cohort, so a column filter re-scopes the mix with it. */}
      {comparisonActive && !isFetching && !error && rows.length > 0 && (
        <>
          <CoTradeSummary
            rows={tableFilterCohort}
            comparison={query!.with}
            profileWallets={profileWallets}
          />
          <label className="mb-3 flex w-fit items-center gap-2 text-xs text-text-dim">
            <Checkbox
              boxSize="sm"
              checked={coOnly}
              onChange={(e) => patch({ coOnly: e.target.checked })}
            />
            Co-traded only
            <span className="text-text-dim/70">
              — hide the tokens none of the comparison wallets touched
            </span>
          </label>
        </>
      )}

      {/* Wallet-level PnL analytics — summary + interactive chart deck. Driven by
          the table's FULL filtered cohort (pinned under focus), so a column
          filter/search here also re-scopes every chart. Lives above the table
          since this is the "is this wallet good" answer, not a per-token drill-down. */}
      {query && !isFetching && !error && (
        <WalletAnalyticsPanel
          rows={tableFilterCohort}
          timezone={timezone}
          focus={focus}
          onFocusChange={onFocusChange}
        />
      )}

      {/* Token table — shared `TokenTable` client mode + Charts toggle (defaults
          on; persisted per tableId). Wallet stats ride `renderChartCardExtra` so
          the trader dimension never duplicates the token columns. Focus narrows
          the input set; the analytics panel above is fed by the pinned
          table-filter cohort so timing charts keep their parent base. */}
      {query && rows.length > 0 && (
        <TokenTable
          columns={columns}
          rows={rowsForTable}
          existingKeys={ALL_TOKEN_INFO_KEYS}
          mintSetFilter
          charts
          chartsDefaultOn
          searchable
          colFilters
          colToggle
          hoverable
          loading={isFetching}
          groupLabels={COLUMN_GROUP_LABELS}
          tableId="trader_analysis_tokens"
          highlightWallet={query.wallet}
          titleOf={(r) => r.symbol || r.name || shortAddr(r.mint_address)}
          selectedKey={inspected?.mint ?? null}
          onSelect={(mint) => {
            const row = mint ? rowsForTable.find((r) => r.mint_address === mint) : null;
            setInspected(mint ? { mint, symbol: row?.symbol } : null);
          }}
          onFilteredRowsChange={handleFilteredRows}
          emptyMessage="No tokens match the filters"
          renderChartCardExtra={(row) => <TraderChartCardExtra row={row} timezone={timezone} />}
          flowPatternKeys={lens.keys}
        />
      )}

      {inspected && (
        <LazyLabTokenInspectModal
          target={inspectFromMint(inspected.mint, inspected.symbol)}
          titleSuffix="Token inspect"
          // Same lens the grid's cards draw under. Without it the modal has no
          // fingerprint to fall back on (`ruleOverride` is null here), so its
          // chart and Vol column classify with nothing.
          flowPatternKeys={lens.keys}
          onClose={() => setInspected(null)}
        />
      )}
    </div>
    </FlowLensProvider>
  );
}
