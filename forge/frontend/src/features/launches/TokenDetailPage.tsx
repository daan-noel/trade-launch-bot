import { useEffect, useMemo, useRef } from 'react';
import { Link, useParams } from 'react-router-dom';
import {
  useTokenOverviewQuery,
  useTokenPositionsQuery,
  useTokenTradesQuery,
} from '@shared/store/endpoints';
import { connectTradeStream } from '@shared/services/sse';
import { apiErrorMessage } from '@shared/store/baseApi';
import {
  AddressDisplay,
  Banner,
  Card,
  Column,
  DataTable,
  Icon,
  KV,
  KVRow,
  RolePill,
  StatCard,
  StatusPill,
  TradeTypePill,
} from '@shared/components/ui';
import { PriceChart } from '@shared/components/PriceChart';
import { ManagePanel } from './ManagePanel';
import { LadderPanel } from './LadderPanel';
import { VolumePanel } from './VolumePanel';
import { ageFromSecs, formatCount, formatSig, formatUsd, gmgnMint, quoteToHuman } from '@shared/lib/format';
import type { TokenOverview, TokenPosition, TradePriced } from '@shared/types';

// This is the live view (SSE-driven), not the cold full-history inspect path.
// Cap the fetch at the most recent N trades so a high-volume mint doesn't ship
// (and re-render, and re-chart) its entire history per refresh (audit C3).
const TRADES_LIMIT = 500;
// Min gap between SSE-triggered refetches. A hot mint pushes many trades/sec; we
// coalesce them into at most one `/trades` refetch per window rather than one per
// trade (still far fresher than the old 10s blind poll).
const REFETCH_THROTTLE_MS = 1500;

export function TokenDetailPage() {
  const { mint = '' } = useParams();
  const { data: overview, isLoading, error } = useTokenOverviewQuery(mint, { skip: !mint });
  const {
    data: trades = [],
    isFetching: tradesLoading,
    refetch: refetchTrades,
  } = useTokenTradesQuery(
    { mint, limit: TRADES_LIMIT },
    // 30s poll is now only a gap-heal fallback; freshness comes from the SSE push.
    { skip: !mint, pollingInterval: 30_000, skipPollingIfUnfocused: true },
  );

  // Refetch the trade page when a trade for THIS mint is pushed, throttled so a
  // burst of trades coalesces into one refetch. The shared stream is unfiltered,
  // so we filter by mint here.
  const lastRefetch = useRef(0);
  useEffect(() => {
    if (!mint) return;
    const handle = connectTradeStream((t) => {
      if (t.mint_address !== mint) return;
      const now = Date.now();
      if (now - lastRefetch.current < REFETCH_THROTTLE_MS) return;
      lastRefetch.current = now;
      refetchTrades();
    });
    return () => handle.close();
  }, [mint, refetchTrades]);
  const { data: positions = [], isFetching: positionsLoading } = useTokenPositionsQuery(mint, {
    skip: !mint,
  });

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-3">
        <Link
          to="/launches"
          className="inline-flex items-center gap-1 text-sm muted hover:text-[var(--color-ink)]"
        >
          <Icon name="chevron-left" size={14} />
          Launched Tokens
        </Link>
      </div>

      {error && <Banner tone="bad">{apiErrorMessage(error, 'Token not found — it may not be ingested yet.')}</Banner>}

      {overview && (
        <>
          <div className="flex items-baseline gap-3 flex-wrap">
            <h1 className="text-xl font-semibold">{overview.name}</h1>
            <span className="muted">{overview.symbol}</span>
            {overview.is_own_launch && <span className="badge badge-info">own launch</span>}
            {overview.is_migrated && <span className="badge badge-info">migrated</span>}
            {overview.is_dead && <span className="badge badge-bad">dead</span>}
            <AddressDisplay value={overview.mint_address} kind="token" lead={6} tail={6} />
            <a
              href={gmgnMint(overview.mint_address)}
              target="_blank"
              rel="noreferrer"
              className="text-xs"
            >
              GMGN ↗
            </a>
          </div>

          <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
            <StatCard label="Price (USD)" value={formatUsd(overview.price_usd)} />
            <StatCard label="Market cap (USD)" value={formatUsd(overview.market_cap_usd)} />
            <StatCard label="Trades" value={formatCount(overview.trade_count)} />
            <StatCard label="Age" value={ageFromSecs(overview.age_secs)} />
          </div>
        </>
      )}

      {isLoading && <Card title="Overview"><p className="muted">Loading…</p></Card>}

      <Card title="Spot price">
        {trades.length > 0 ? (
          <PriceChart trades={trades} overview={overview} mint={mint} />
        ) : (
          <p className="muted text-sm py-8 text-center">
            {tradesLoading ? 'Loading trades…' : 'No trades ingested for this mint yet.'}
          </p>
        )}
      </Card>

      {overview && (
        <Card title="Details">
          <KV cols={2}>
            <KVRow label="Mint"><AddressDisplay value={overview.mint_address} kind="token" lead={10} tail={10} /></KVRow>
            <KVRow label="Creator"><AddressDisplay value={overview.creator_wallet} /></KVRow>
            <KVRow label="Quote"><span className="mono">{overview.quote_symbol}</span></KVRow>
            <KVRow label="Volume (quote)"><span className="mono">{quoteToHuman(overview.volume_quote, overview.quote_decimals)}</span></KVRow>
            <KVRow label="Migrated">{overview.is_migrated ? 'yes' : 'no'}</KVRow>
            <KVRow label="Dead">{overview.is_dead ? 'yes' : 'no'}</KVRow>
          </KV>
        </Card>
      )}

      <Card title={`Our holdings (${positions.length})`}>
        <HoldingsTable positions={positions} loading={positionsLoading} overview={overview} />
      </Card>

      {positions.length > 0 && <ManagePanel mint={mint} overview={overview} />}

      {positions.length > 0 && <LadderPanel mint={mint} />}

      {positions.length > 0 && <VolumePanel mint={mint} />}

      <Card title={`Trades (${trades.length})`}>
        <TradesTable trades={trades} loading={tradesLoading} overview={overview} />
      </Card>
    </div>
  );
}

/** Per-wallet holdings + derived PnL. Value/PnL are computed here from the token's
 *  raw price + decimals (never stored server-side): all quote amounts are quote
 *  base units, `current_price_quote` is quote base units per token base unit. */
function HoldingsTable({
  positions,
  loading,
  overview,
}: {
  positions: TokenPosition[];
  loading: boolean;
  overview: TokenOverview | undefined;
}) {
  const qd = overview?.quote_decimals ?? 9;
  const td = overview?.decimals ?? 6;
  const price = overview?.current_price_quote ?? null; // quote base units / token base unit
  const usdRate = overview?.quote_usd_rate ?? null;
  const quoteSymbol = overview?.quote_symbol ?? 'quote';

  // Aggregate holdings across every open position: total tokens held + total cost
  // basis + total value, in the quote (SOL). Same math as the per-row cells, summed
  // once (audit H2: recomputes only when positions / price change). Open-only, to
  // match the launched-tokens list aggregate.
  const totals = useMemo(() => {
    const open = positions.filter((p) => p.status === 'open');
    const totalBase = open.reduce((s, p) => s + p.balance_base, 0);
    const totalCostQuote = open.reduce((s, p) => s + p.cost_quote, 0);
    const totalValueQuote = price == null ? null : totalBase * price;
    return { totalBase, totalCostQuote, totalValueQuote };
  }, [positions, price]);

  // Memoized so its identity is stable across polls (audit H2) — the value/PnL
  // closures fold in the overview-derived scalars, so they only rebuild when
  // those actually change, not on every positions refetch.
  const columns = useMemo<Column<TokenPosition>[]>(() => {
    // value_quote (quote base units) = balance_base * price; PnL folds realized in.
    const valueQuote = (p: TokenPosition) => (price == null ? null : p.balance_base * price);
    const pnlQuote = (p: TokenPosition) => {
      const v = valueQuote(p);
      return v == null ? null : p.realized_quote + v - p.cost_quote;
    };
    const usd = (quoteBase: number | null) =>
      quoteBase == null || usdRate == null ? null : (quoteBase / 10 ** qd) * usdRate;

    return [
      { header: 'Role', render: (p) => <RolePill role={p.role} /> },
      {
        header: 'Wallet',
        render: (p) => <AddressDisplay value={p.wallet_address} />,
      },
      {
        header: 'Balance',
        align: 'right',
        render: (p) => <span className="mono">{(p.balance_base / 10 ** td).toLocaleString(undefined, { maximumFractionDigits: 2 })}</span>,
      },
      {
        header: `Cost (${quoteSymbol})`,
        align: 'right',
        render: (p) => <span className="mono">{quoteToHuman(p.cost_quote, qd)}</span>,
      },
      {
        header: `Value (${quoteSymbol})`,
        align: 'right',
        render: (p) => <span className="mono">{quoteToHuman(valueQuote(p), qd)}</span>,
      },
      {
        header: 'PnL',
        align: 'right',
        render: (p) => {
          const pnl = pnlQuote(p);
          if (pnl == null) return <span className="muted">—</span>;
          const pct = p.cost_quote > 0 ? (pnl / p.cost_quote) * 100 : null;
          const tone = pnl >= 0 ? 'text-[var(--color-good)]' : 'text-[var(--color-bad)]';
          return (
            <span className={`mono ${tone}`}>
              {formatUsd(usd(pnl))}
              {pct != null && <span className="text-xs muted"> ({pct >= 0 ? '+' : ''}{pct.toFixed(0)}%)</span>}
            </span>
          );
        },
      },
      { header: 'Status', render: (p) => <StatusPill status={p.status} /> },
    ];
  }, [qd, td, price, usdRate, quoteSymbol]);

  return (
    <div className="space-y-3">
      {positions.length > 0 && (
        <div className="grid grid-cols-3 gap-3">
          <StatCard
            label="Total held"
            value={(totals.totalBase / 10 ** td).toLocaleString(undefined, { maximumFractionDigits: 2 })}
          />
          <StatCard
            label={`Total cost (${quoteSymbol})`}
            value={quoteToHuman(totals.totalCostQuote, qd)}
          />
          <StatCard
            label={`Total value (${quoteSymbol})`}
            value={quoteToHuman(totals.totalValueQuote, qd)}
          />
        </div>
      )}
      <DataTable
        columns={columns}
        rows={positions}
        rowKey={(p) => p.id}
        loading={loading}
        empty="No tracked holdings — this token isn't one of our launches, or its positions aren't seeded yet."
      />
    </div>
  );
}

function TradesTable({
  trades,
  loading,
  overview,
}: {
  trades: TradePriced[];
  loading: boolean;
  overview: TokenOverview | undefined;
}) {
  const qd = overview?.quote_decimals ?? 9;
  // Memoized so its identity is stable across the 10s trades poll (audit H2).
  const columns = useMemo<Column<TradePriced>[]>(() => [
    { header: 'Slot', align: 'right', render: (t) => <span className="mono">{t.slot}</span> },
    { header: 'Type', render: (t) => <TradeTypePill type={t.trade_type} /> },
    { header: 'Market', render: (t) => <span className="text-xs muted">{t.market_kind}</span> },
    { header: 'Quote', align: 'right', render: (t) => <span className="mono">{t.amount_quote_display?.toFixed(4) ?? quoteToHuman(t.amount_quote, qd)}</span> },
    { header: 'USD', align: 'right', render: (t) => <span className="mono">{formatUsd(t.amount_usd)}</span> },
    { header: 'Wallet', render: (t) => <AddressDisplay value={t.wallet_address} /> },
    {
      header: 'Tx',
      render: (t) => {
        const sig = formatSig(t.tx_signature);
        return sig !== '—' ? <AddressDisplay value={sig} kind="tx" /> : <span className="muted">—</span>;
      },
    },
  ], [qd]);
  return (
    <DataTable
      columns={columns}
      rows={trades}
      rowKey={(t, i) => `${t.slot}-${t.tx_index}-${t.leg_index}-${i}`}
      loading={loading}
      empty="No trades yet."
      maxHeight={480}
    />
  );
}
