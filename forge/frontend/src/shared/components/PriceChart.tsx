import { useMemo, useState } from 'react';
import clsx from 'clsx';
import type { TradePriced, TokenOverview } from '@shared/types';
import { formatSig } from '@shared/lib/format';
import { useWalletPoolQuery } from '@shared/store/endpoints';
import { TokenPriceChart } from './tokenChart';
import type {
  ChartMetric,
  ChartTrade,
  PriceUnit,
  ProfileWalletInfo,
} from './tokenChart/types';

/**
 * Forge adapter over the ported hunter token chart. Maps `trades_priced` rows
 * onto the chart's `ChartTrade` contract (which is in hunter's units — human SOL
 * + raw token base units — so the ported pump.fun constants line up), sources
 * wallet markers from the managed pool, and owns the SOL/USD + price/MC toggles.
 */

/** Managed-wallet role → theme CSS var; resolved to a real hex for canvas draws. */
const ROLE_VAR: Record<string, string> = {
  dev: '--color-role-dev',
  bundler: '--color-role-bundler',
  treasury: '--color-role-treasury',
  trading: '--color-role-trading',
};

function resolveRoleColor(role: string): string {
  const varName = ROLE_VAR[role.toLowerCase()] ?? '--color-accent';
  try {
    const v = getComputedStyle(document.documentElement).getPropertyValue(varName).trim();
    if (v) return v;
  } catch {
    /* ignore (SSR / detached) */
  }
  return '#7eb8ff';
}

/** `trades_priced` row → `ChartTrade`. Converts forge's raw quote/base ratios
 *  (lamport-scale) to human SOL per raw token so the chart's SOL-denominated
 *  constants (initial 30 SOL, migration price, FDV supply) apply unchanged. */
function toChartTrade(t: TradePriced): ChartTrade {
  const qd = t.quote_decimals;
  const scale = 10 ** qd;
  const spot = t.spot_price_quote ?? t.exec_price_quote ?? 0;
  return {
    block_time: t.block_time,
    price_per_token: spot / scale,
    trade_type: t.trade_type === 'buy' ? 'buy' : 'sell',
    amount_sol: t.amount_quote_display ?? t.amount_quote / scale,
    token_amount: t.amount_base,
    slot: t.slot,
    tx_index: t.tx_index,
    tx_signature: typeof t.tx_signature === 'string' ? t.tx_signature : formatSig(t.tx_signature),
    leg_index: t.leg_index,
    reserve_sol: t.reserve_quote == null ? null : t.reserve_quote / scale,
    reserve_token: t.reserve_base,
    venue: t.market_kind === 'amm' ? 'amm' : 'curve',
    wallet_address: t.wallet_address,
  };
}

export function PriceChart({
  trades,
  overview,
  mint,
  height = 340,
}: {
  trades: TradePriced[];
  overview?: TokenOverview;
  mint: string;
  height?: number;
}) {
  const [metric, setMetric] = useState<ChartMetric>('price');
  const [unit, setUnit] = useState<PriceUnit>('SOL');

  const quoteSymbol = overview?.quote_symbol ?? trades[0]?.quote_symbol ?? 'SOL';
  const usdRate = overview?.quote_usd_rate ?? trades[0]?.quote_usd_rate ?? null;
  const canUsd = usdRate != null && usdRate > 0;
  const effectiveUnit: PriceUnit = unit === 'USD' && canUsd ? 'USD' : 'SOL';

  const chartTrades = useMemo(() => trades.map(toChartTrade), [trades]);

  // Managed-pool wallets as role-colored markers. The pool is global; only
  // wallets that actually traded this mint surface (the marker builder filters).
  const { data: pool = [] } = useWalletPoolQuery(undefined);
  const profileWallets = useMemo<ProfileWalletInfo[]>(
    () =>
      pool.map((w) => ({
        address: w.address,
        label: w.role,
        profileName: w.role,
        color: resolveRoleColor(w.role),
        isMine: w.role === 'dev',
      })),
    [pool],
  );

  const toValue = useMemo(() => {
    if (effectiveUnit === 'USD' && usdRate != null) return (sol: number) => sol * usdRate;
    return (sol: number) => sol;
  }, [effectiveUnit, usdRate]);

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-end">
        <div className="inline-flex rounded-md border border-[var(--color-line)] overflow-hidden text-xs">
          {(['SOL', 'USD'] as PriceUnit[]).map((u) => (
            <button
              key={u}
              type="button"
              disabled={u === 'USD' && !canUsd}
              onClick={() => setUnit(u)}
              className={clsx(
                'px-2 py-0.5 transition-colors',
                effectiveUnit === u
                  ? 'bg-[var(--color-accent)] text-[var(--color-accent-ink)]'
                  : 'text-[var(--color-muted)] hover:text-[var(--color-ink)]',
                u === 'USD' && !canUsd && 'opacity-40 cursor-not-allowed',
              )}
              title={u === 'USD' && !canUsd ? 'No USD rate for this quote yet' : undefined}
            >
              {u === 'USD' ? '$ USD' : `◎ ${quoteSymbol}`}
            </button>
          ))}
        </div>
      </div>
      <TokenPriceChart
        id={mint}
        symbol={overview?.symbol ?? quoteSymbol}
        trades={chartTrades}
        height={height}
        metric={metric}
        onMetricChange={setMetric}
        priceUnit={effectiveUnit}
        priceLabel={effectiveUnit === 'USD' ? 'USD' : quoteSymbol}
        toValue={toValue}
        isMigrated={overview?.is_migrated ?? undefined}
        tokenCreatedAt={overview?.created_at}
        profileWallets={profileWallets}
      />
    </div>
  );
}
