import { usePriceUnit } from 'context/PriceUnitContext';
import { useLazyGetSolPriceQuery } from 'store/apiSlice';
import { ToggleGroup } from 'components/ui/ToggleGroup';

export function PriceUnitToggle() {
  const { unit, usdRate, setUnit, setUsdRate } = usePriceUnit();
  const [fetchSolPrice] = useLazyGetSolPriceQuery();

  const onChange = async (next: 'SOL' | 'USD') => {
    if (next === unit) return;
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
      <ToggleGroup
        aria-label="Price unit"
        tone="neutral"
        size="sm"
        value={unit}
        onChange={onChange}
        options={[
          {
            value: 'SOL',
            label: 'SOL',
            activeClassName: 'bg-primary/12 text-primary shadow-sm',
          },
          {
            value: 'USD',
            label: 'USD',
            activeClassName: 'bg-secondary/12 text-secondary shadow-sm',
          },
        ]}
      />
    </div>
  );
}
