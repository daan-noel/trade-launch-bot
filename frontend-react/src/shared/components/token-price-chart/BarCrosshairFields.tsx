import { CHART_OHLC_COLORS } from './constants';
import type { ChartCrosshairInfo, ChartStyle } from './types';

type BarCrosshairFieldsProps = {
  style: ChartStyle;
  crosshair: ChartCrosshairInfo;
  formatPrice: (value: number) => string;
  formatVol: (value: number) => string;
  layout: 'grid' | 'inline';
};

function FieldPair({
  label,
  value,
  color,
  layout,
}: {
  label: string;
  value: string;
  color: string;
  layout: 'grid' | 'inline';
}) {
  if (layout === 'grid') {
    return (
      <>
        <span className="font-semibold" style={{ color }}>
          {label}
        </span>
        <span style={{ color }}>{value}</span>
      </>
    );
  }
  return (
    <span style={{ color }}>
      <span className="font-semibold">{label}</span> {value}
    </span>
  );
}

export function BarCrosshairFields({
  style,
  crosshair,
  formatPrice,
  formatVol,
  layout,
}: BarCrosshairFieldsProps) {
  const liqLabel =
    crosshair.liquiditySol != null ? formatVol(crosshair.liquiditySol) : '—';
  const gap = layout === 'inline' ? ' ' : undefined;

  const volLiq = (
    <>
      <FieldPair
        label="Vol"
        value={formatVol(crosshair.volume)}
        color={CHART_OHLC_COLORS.volume}
        layout={layout}
      />
      {gap}
      <FieldPair
        label="Liq"
        value={liqLabel}
        color={CHART_OHLC_COLORS.liquidity}
        layout={layout}
      />
    </>
  );

  if (style === 'candles') {
    if (layout === 'inline') {
      return (
        <>
          <div>
            <FieldPair label="O" value={formatPrice(crosshair.open)} color={CHART_OHLC_COLORS.open} layout={layout} />
            {' '}
            <FieldPair label="C" value={formatPrice(crosshair.close)} color={CHART_OHLC_COLORS.close} layout={layout} />
          </div>
          <div>
            <FieldPair label="H" value={formatPrice(crosshair.high)} color={CHART_OHLC_COLORS.high} layout={layout} />
            {' '}
            <FieldPair label="L" value={formatPrice(crosshair.low)} color={CHART_OHLC_COLORS.low} layout={layout} />
          </div>
          <div>{volLiq}</div>
        </>
      );
    }
    return (
      <div className="grid grid-cols-[auto_1fr] gap-x-2 gap-y-0.5">
        <FieldPair
          label="O"
          value={formatPrice(crosshair.open)}
          color={CHART_OHLC_COLORS.open}
          layout={layout}
        />
        <FieldPair
          label="H"
          value={formatPrice(crosshair.high)}
          color={CHART_OHLC_COLORS.high}
          layout={layout}
        />
        <FieldPair
          label="L"
          value={formatPrice(crosshair.low)}
          color={CHART_OHLC_COLORS.low}
          layout={layout}
        />
        <FieldPair
          label="C"
          value={formatPrice(crosshair.close)}
          color={CHART_OHLC_COLORS.close}
          layout={layout}
        />
        <FieldPair
          label="Vol"
          value={formatVol(crosshair.volume)}
          color={CHART_OHLC_COLORS.volume}
          layout={layout}
        />
        <FieldPair
          label="Liq"
          value={liqLabel}
          color={CHART_OHLC_COLORS.liquidity}
          layout={layout}
        />
      </div>
    );
  }

  if (layout === 'inline') {
    return (
      <>
        <div>
          <FieldPair label="Price" value={formatPrice(crosshair.close)} color={CHART_OHLC_COLORS.price} layout={layout} />
        </div>
        <div>{volLiq}</div>
      </>
    );
  }
  return (
    <div className="grid grid-cols-[auto_1fr] gap-x-2 gap-y-0.5">
      <FieldPair
        label="Price"
        value={formatPrice(crosshair.close)}
        color={CHART_OHLC_COLORS.price}
        layout={layout}
      />
      <FieldPair
        label="Vol"
        value={formatVol(crosshair.volume)}
        color={CHART_OHLC_COLORS.volume}
        layout={layout}
      />
      <FieldPair
        label="Liq"
        value={liqLabel}
        color={CHART_OHLC_COLORS.liquidity}
        layout={layout}
      />
    </div>
  );
}
