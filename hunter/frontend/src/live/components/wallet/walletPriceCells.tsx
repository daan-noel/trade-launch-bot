import { memo } from 'react';
import { useWalletPriceDisplay } from 'hooks/useWalletPriceDisplay';
import { cn } from 'lib/cn';
import { priceClass } from 'utils/format';

/**
 * Rate-aware wallet price cells — the USD-native counterpart to
 * {@link import('components/tokens/priceCells')}.
 *
 * These read the unit/USD-rate from {@link useWalletPriceDisplay} (i.e. the
 * `PriceUnitContext`) *themselves* rather than receiving a pre-baked formatter
 * through the column definition. That keeps the wallet column array
 * referentially stable across a USD-rate tick: only these cells re-render when
 * the rate changes, instead of rebuilding every column def and re-rendering the
 * whole visible grid.
 */

export const ValueCell = memo(function ValueCell({ usd }: { usd: number | null | undefined }) {
  const price = useWalletPriceDisplay();
  if (usd == null) return <span className="text-text-dim">—</span>;
  return (
    <span className="font-semibold tabular-nums text-text">{price.displayValue(usd)}</span>
  );
});

export const PriceCell = memo(function PriceCell({ usd }: { usd: number | null | undefined }) {
  const price = useWalletPriceDisplay();
  if (usd == null) return <span className="text-text-dim">—</span>;
  return <span className={cn('tabular-nums', priceClass(usd))}>{price.displayPrice(usd)}</span>;
});

export const LiquidityCell = memo(function LiquidityCell({
  usd,
}: {
  usd: number | null | undefined;
}) {
  const price = useWalletPriceDisplay();
  if (usd == null) return <span className="text-text-dim">—</span>;
  return <span className="tabular-nums text-text-mid">{price.displayLiquidity(usd)}</span>;
});
