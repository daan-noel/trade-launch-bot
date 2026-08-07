import { memo } from 'react';
import { usePriceUnitSetting, useUsdRate } from 'context/PriceUnitContext';
import { formatCompact, formatDecimalTrim, formatPrice, formatUsd, priceClass } from 'utils/format';

/**
 * Rate-aware price cells.
 *
 * Unit and rate live in separate contexts. The outer cell only subscribes to
 * the unit, so a SOL/USD tick does not re-render the SOL-mode branch. The USD
 * branch is a child that alone subscribes to the rate.
 */

const SolPrice = memo(function SolPrice({ sol }: { sol: number | null | undefined }) {
  return <>{sol != null ? `◎${formatPrice(sol)}` : '-'}</>;
});

const UsdPrice = memo(function UsdPrice({ sol }: { sol: number | null | undefined }) {
  const { usdRate } = useUsdRate();
  if (sol == null) return <>-</>;
  return <>{usdRate != null ? formatUsd(sol * usdRate) : `◎${formatPrice(sol)}`}</>;
});

const SolAmount = memo(function SolAmount({ sol }: { sol: number | null | undefined }) {
  return <>{sol != null ? `◎${formatDecimalTrim(sol, 4)}` : '-'}</>;
});

const UsdAmount = memo(function UsdAmount({ sol }: { sol: number | null | undefined }) {
  const { usdRate } = useUsdRate();
  if (sol == null) return <>-</>;
  return <>{usdRate != null ? formatUsd(sol * usdRate) : `◎${formatDecimalTrim(sol, 4)}`}</>;
});

const SolCompact = memo(function SolCompact({
  sol,
  digits,
}: {
  sol: number | null | undefined;
  digits: number;
}) {
  return <>{sol != null ? `◎${formatCompact(sol, digits)}` : '-'}</>;
});

const UsdCompact = memo(function UsdCompact({
  sol,
  digits,
}: {
  sol: number | null | undefined;
  digits: number;
}) {
  const { usdRate } = useUsdRate();
  if (sol == null) return <>-</>;
  return (
    <>
      {usdRate != null
        ? `$${formatCompact(sol * usdRate, digits)}`
        : `◎${formatCompact(sol, digits)}`}
    </>
  );
});

export const PriceCell = memo(function PriceCell({ sol }: { sol: number | null | undefined }) {
  const { unit } = usePriceUnitSetting();
  return unit === 'SOL' ? <SolPrice sol={sol} /> : <UsdPrice sol={sol} />;
});

export const CurrentPriceCell = memo(function CurrentPriceCell({
  sol,
}: {
  sol: number | null | undefined;
}) {
  const { unit } = usePriceUnitSetting();
  return (
    <span className={priceClass(sol ?? undefined)}>
      {unit === 'SOL' ? <SolPrice sol={sol} /> : <UsdPrice sol={sol} />}
    </span>
  );
});

export const AmountCell = memo(function AmountCell({ sol }: { sol: number | null | undefined }) {
  const { unit } = usePriceUnitSetting();
  return unit === 'SOL' ? <SolAmount sol={sol} /> : <UsdAmount sol={sol} />;
});

/**
 * Network-fee amount. Fees live between ~5e-6 and ~5e-3 SOL, which the two
 * existing renderers both mangle: `AmountCell` truncates to 4 decimals (a base
 * fee shows as `◎0`) and `PriceCell` switches to engineering notation
 * (`5e-6`) — neither reads as money. This renders plain lamport-resolution
 * decimals with the trailing zeros trimmed, the way an explorer shows gas.
 *
 * `null` renders as an em-dash, never `◎0`: the fee is unknown on every trade
 * ingested before migration 0005 and cannot be backfilled, and a landed
 * transaction never actually pays zero.
 */
const SolFee = memo(function SolFee({ sol }: { sol: number | null | undefined }) {
  return <>{sol != null ? `◎${formatDecimalTrim(sol, 9)}` : '—'}</>;
});

const UsdFee = memo(function UsdFee({ sol }: { sol: number | null | undefined }) {
  const { usdRate } = useUsdRate();
  if (sol == null) return <>—</>;
  if (usdRate == null) return <>{`◎${formatDecimalTrim(sol, 9)}`}</>;
  // A typical fee is well under a cent, so `formatUsd` would round it to $0.00
  // or flip to engineering notation. Four decimals keeps it readable as money.
  return <>{`$${formatDecimalTrim(sol * usdRate, 4)}`}</>;
});

export const FeeCell = memo(function FeeCell({ sol }: { sol: number | null | undefined }) {
  const { unit } = usePriceUnitSetting();
  return unit === 'SOL' ? <SolFee sol={sol} /> : <UsdFee sol={sol} />;
});

export const CompactCell = memo(function CompactCell({
  sol,
  digits,
}: {
  sol: number | null | undefined;
  digits: number;
}) {
  const { unit } = usePriceUnitSetting();
  return unit === 'SOL' ? (
    <SolCompact sol={sol} digits={digits} />
  ) : (
    <UsdCompact sol={sol} digits={digits} />
  );
});
