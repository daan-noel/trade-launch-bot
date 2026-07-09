import { memo } from 'react';
import { usePriceDisplay } from 'hooks/usePriceDisplay';
import { priceClass } from 'utils/format';

/**
 * Rate-aware price cells.
 *
 * These read the unit/USD-rate from {@link usePriceDisplay} (i.e. the
 * `PriceUnitContext`) *themselves* rather than receiving a pre-baked formatter
 * through the column definition. That keeps the column array referentially
 * stable across a USD-rate tick: previously a new rate rebuilt every column def,
 * handed the table a fresh `visCols` identity, and re-rendered the whole visible
 * grid. Now only these cells — the ones whose output actually depends on the
 * rate — re-render when it changes, via the context subscription.
 */

export const PriceCell = memo(function PriceCell({ sol }: { sol: number | null | undefined }) {
  const price = usePriceDisplay();
  return <>{sol != null ? price.displayPrice(sol) : '-'}</>;
});

export const CurrentPriceCell = memo(function CurrentPriceCell({
  sol,
}: {
  sol: number | null | undefined;
}) {
  const price = usePriceDisplay();
  return (
    <span className={priceClass(sol ?? undefined)}>
      {sol != null ? price.displayPrice(sol) : '-'}
    </span>
  );
});

export const AmountCell = memo(function AmountCell({ sol }: { sol: number | null | undefined }) {
  const price = usePriceDisplay();
  return <>{sol != null ? price.displayAmount(sol) : '-'}</>;
});

export const CompactCell = memo(function CompactCell({
  sol,
  digits,
}: {
  sol: number | null | undefined;
  digits: number;
}) {
  const price = usePriceDisplay();
  return <>{sol != null ? price.displayCompact(sol, digits) : '-'}</>;
});
