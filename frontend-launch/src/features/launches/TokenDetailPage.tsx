import { Link, useParams } from 'react-router-dom';
import { useTokenOverviewQuery, useTokenTradesQuery } from '@shared/store/endpoints';
import { apiErrorMessage } from '@shared/store/baseApi';
import {
  AddressDisplay,
  Banner,
  Card,
  Column,
  DataTable,
  KV,
  KVRow,
  StatCard,
  StatusPill,
} from '@shared/components/ui';
import { PriceChart } from '@shared/components/PriceChart';
import { ageFromSecs, formatCount, formatSig, formatUsd, quoteToHuman } from '@shared/lib/format';
import type { TokenOverview, TradePriced } from '@shared/types';

export function TokenDetailPage() {
  const { mint = '' } = useParams();
  const { data: overview, isLoading, error } = useTokenOverviewQuery(mint, { skip: !mint });
  const { data: trades = [], isFetching: tradesLoading } = useTokenTradesQuery(
    { mint },
    { skip: !mint, pollingInterval: 10_000 },
  );

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-3">
        <Link to="/launches" className="text-sm muted hover:text-[var(--color-ink)]">
          ← Launched Tokens
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
          <PriceChart trades={trades} />
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

      <Card title={`Trades (${trades.length})`}>
        <TradesTable trades={trades} loading={tradesLoading} overview={overview} />
      </Card>
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
  const columns: Column<TradePriced>[] = [
    { header: 'Slot', align: 'right', render: (t) => <span className="mono">{t.slot}</span> },
    { header: 'Type', render: (t) => <StatusPill status={t.trade_type} /> },
    { header: 'Market', render: (t) => <span className="text-xs muted">{t.market_kind}</span> },
    { header: 'Quote', align: 'right', render: (t) => <span className="mono">{t.amount_quote_display?.toFixed(4) ?? quoteToHuman(t.amount_quote, qd)}</span> },
    { header: 'USD', align: 'right', render: (t) => <span className="mono">{formatUsd(t.amount_usd)}</span> },
    { header: 'Wallet', render: (t) => <span className="mono text-xs muted">#{t.wallet_id}</span> },
    {
      header: 'Tx',
      render: (t) => {
        const sig = formatSig(t.tx_signature);
        return sig !== '—' ? <AddressDisplay value={sig} kind="tx" /> : <span className="muted">—</span>;
      },
    },
  ];
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
