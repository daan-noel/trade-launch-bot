import { usePriceUnit } from 'context/PriceUnitContext';
import { useLazyGetSolPriceQuery } from 'store/apiSlice';
import { cn } from 'lib/cn';

export function PriceUnitToggle() {
  const { unit, usdRate, setUnit, setUsdRate } = usePriceUnit();
  const [fetchSolPrice] = useLazyGetSolPriceQuery();

  const toggleUnit = async () => {
    const next = unit === 'SOL' ? 'USD' : 'SOL';
    setUnit(next);
    if (next === 'USD') {
      try {
        // `preferCacheValue` reuses the header's already-fetched rate instead of
        // hitting the network again on every SOL→USD toggle.
        const rate = await fetchSolPrice(undefined, true).unwrap();
        setUsdRate(rate);
      } catch {
        /* ignore */
      }
    }
  };

  return (
    <div className="flex items-center gap-2">
      {unit === 'USD' && (
        <span className="hidden text-[11px] tabular-nums text-text-dim lg:inline">
          {usdRate != null ? `$${usdRate.toFixed(2)}` : '—'}
        </span>
      )}
      <div className="flex rounded-lg border border-white/6 bg-white/3 p-0.5">
        <button
          type="button"
          onClick={() => unit !== 'SOL' && toggleUnit()}
          className={cn(
            'rounded-md px-2.5 py-1 text-[11px] font-semibold tracking-wide transition-all duration-150',
            unit === 'SOL'
              ? 'bg-primary/12 text-primary shadow-sm'
              : 'text-text-dim hover:text-text',
          )}
        >
          SOL
        </button>
        <button
          type="button"
          onClick={() => unit !== 'USD' && toggleUnit()}
          className={cn(
            'rounded-md px-2.5 py-1 text-[11px] font-semibold tracking-wide transition-all duration-150',
            unit === 'USD'
              ? 'bg-secondary/12 text-secondary shadow-sm'
              : 'text-text-dim hover:text-text',
          )}
        >
          USD
        </button>
      </div>
    </div>
  );
}
