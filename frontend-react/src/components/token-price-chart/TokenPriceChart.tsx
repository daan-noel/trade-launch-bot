import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  createChart,
  createSeriesMarkers,
  LineStyle,
  type IChartApi,
  type IPriceLine,
  type ISeriesApi,
  type SeriesMarker,
  type UTCTimestamp,
} from 'lightweight-charts';
import {
  aggregateTradesToBars,
  aggregateTradesToBarsBySlot,
  athChartValue,
  migrationChartValue,
  barsToCandleData,
  barsToLineData,
  tradeBarSlot,
  tradeBarTime,
  tradesForChartMetric,
} from './chartBars';
import { ChartToolbar } from './ChartToolbar';
import { cn } from './cn';
import {
  CANDLE_SERIES_OPTIONS,
  CHART_COLORS,
  CHART_INTERVALS,
  createChartOptions,
  createChartPriceFormat,
  DEFAULT_CHART_PREFS,
  LINE_SERIES_OPTIONS,
  LS_CHART_PREFS_KEY,
  SERIES_BY_STYLE,
} from './constants';
import type {
  ChartBarSelection,
  ChartCrosshairInfo,
  ChartGroupMode,
  ChartInterval,
  ChartStyle,
  ChartTrade,
  OhlcBar,
  TokenPriceChartProps,
} from './types';

function loadPrefs(): {
  groupMode: ChartGroupMode;
  interval: ChartInterval;
  style: ChartStyle;
  showTradeMarkers: boolean;
  showAthLine: boolean;
  showMigrationLine: boolean;
} {
  try {
    const raw = localStorage.getItem(LS_CHART_PREFS_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as {
        groupMode?: ChartGroupMode;
        interval?: ChartInterval;
        style?: ChartStyle;
        showTradeMarkers?: boolean;
        showAthLine?: boolean;
        showMigrationLine?: boolean;
      };
      return {
        groupMode: parsed.groupMode ?? DEFAULT_CHART_PREFS.groupMode,
        interval: parsed.interval ?? DEFAULT_CHART_PREFS.interval,
        style: parsed.style ?? DEFAULT_CHART_PREFS.style,
        showTradeMarkers: parsed.showTradeMarkers ?? DEFAULT_CHART_PREFS.showTradeMarkers,
        showAthLine: parsed.showAthLine ?? DEFAULT_CHART_PREFS.showAthLine,
        showMigrationLine:
          parsed.showMigrationLine ?? DEFAULT_CHART_PREFS.showMigrationLine,
      };
    }
  } catch {
    /* ignore */
  }
  return DEFAULT_CHART_PREFS;
}

function savePrefs(
  groupMode: ChartGroupMode,
  interval: ChartInterval,
  style: ChartStyle,
  showTradeMarkers: boolean,
  showAthLine: boolean,
  showMigrationLine: boolean,
) {
  try {
    localStorage.setItem(
      LS_CHART_PREFS_KEY,
      JSON.stringify({
        groupMode,
        interval,
        style,
        showTradeMarkers,
        showAthLine,
        showMigrationLine,
      }),
    );
  } catch {
    /* ignore */
  }
}

function buildTradeMarkers(
  trades: ChartTrade[],
  groupMode: ChartGroupMode,
  intervalSec: number,
): SeriesMarker<UTCTimestamp>[] {
  const countsByBar = new Map<UTCTimestamp, { buy: number; sell: number }>();
  for (const trade of trades) {
    const time =
      groupMode === 'slot'
        ? tradeBarSlot(trade)
        : tradeBarTime(trade.block_time, intervalSec);
    if (time == null) continue;
    const bucket = countsByBar.get(time) ?? { buy: 0, sell: 0 };
    if (trade.trade_type === 'buy') bucket.buy += 1;
    else bucket.sell += 1;
    countsByBar.set(time, bucket);
  }

  const markers: SeriesMarker<UTCTimestamp>[] = [];
  for (const [time, { buy, sell }] of countsByBar) {
    if (buy === 0 && sell === 0) continue;

    const textParts: string[] = [];
    if (buy > 0) textParts.push(`↑${buy}`);
    if (sell > 0) textParts.push(`↓${sell}`);

    const onlyBuy = buy > 0 && sell === 0;
    const onlySell = sell > 0 && buy === 0;

    markers.push({
      time,
      position: onlyBuy ? 'belowBar' : onlySell ? 'aboveBar' : 'inBar',
      color: onlyBuy ? CHART_COLORS.up : onlySell ? CHART_COLORS.down : CHART_COLORS.text,
      shape: onlyBuy ? 'arrowUp' : onlySell ? 'arrowDown' : 'circle',
      text: textParts.join(' '),
    });
  }
  return markers;
}

type MarkersPlugin = {
  setMarkers: (markers: SeriesMarker<UTCTimestamp>[]) => void;
  detach: () => void;
};

function panelClass(className?: string) {
  return cn('rounded-lg border', className);
}

const panelStyle = {
  borderColor: CHART_COLORS.border,
  backgroundColor: CHART_COLORS.background,
};

function Placeholder({
  message,
  className,
  height = 320,
}: {
  message: string;
  className?: string;
  height?: number;
}) {
  return (
    <div
      className={panelClass(cn('flex items-center justify-center text-xs', className))}
      style={{ ...panelStyle, height, color: CHART_COLORS.panelTextDim }}
    >
      {message}
    </div>
  );
}

export function TokenPriceChart({
  symbol,
  id = '',
  trades,
  loading = false,
  error = null,
  toValue: toValueProp,
  priceLabel = 'SOL',
  priceUnit = 'SOL',
  metric = 'price',
  onMetricChange,
  className,
  height = 320,
  onBarClick,
  athPriceInSol = null,
  isMigrated,
  isMayhemMode,
  isCashbackEnabled,
}: TokenPriceChartProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const onBarClickRef = useRef(onBarClick);
  onBarClickRef.current = onBarClick;
  const seriesRef = useRef<ISeriesApi<'Line' | 'Candlestick'> | null>(null);
  const markersPluginRef = useRef<MarkersPlugin | null>(null);
  const barsRef = useRef<OhlcBar[]>([]);

  const initialPrefs = loadPrefs();
  const [groupMode, setGroupMode] = useState<ChartGroupMode>(initialPrefs.groupMode);
  const [interval, setInterval] = useState<ChartInterval>(initialPrefs.interval);
  const [style, setStyle] = useState<ChartStyle>(initialPrefs.style);
  const [showTradeMarkers, setShowTradeMarkers] = useState(initialPrefs.showTradeMarkers);
  const [showAthLine, setShowAthLine] = useState(initialPrefs.showAthLine);
  const [showMigrationLine, setShowMigrationLine] = useState(initialPrefs.showMigrationLine);
  const [crosshair, setCrosshair] = useState<ChartCrosshairInfo | null>(null);
  const athLineRef = useRef<IPriceLine | null>(null);
  const migrationLineRef = useRef<IPriceLine | null>(null);

  const toValue = useCallback(
    (sol: number) => (toValueProp ? toValueProp(sol) : sol),
    [toValueProp],
  );

  const intervalSec = CHART_INTERVALS[interval];
  const groupingKey = groupMode === 'slot' ? 'slot' : intervalSec;

  const shouldFitContentRef = useRef(true);
  const prevIdRef = useRef(id);
  const prevGroupingKeyRef = useRef(groupingKey);

  const chartTrades = useMemo(
    () => tradesForChartMetric(trades, metric),
    [trades, metric],
  );

  const bars = useMemo(
    () =>
      groupMode === 'slot'
        ? aggregateTradesToBarsBySlot(chartTrades, toValue)
        : aggregateTradesToBars(chartTrades, intervalSec, toValue),
    [chartTrades, groupMode, intervalSec, toValue],
  );
  barsRef.current = bars;

  const athLineAvailable = athChartValue(athPriceInSol, metric, toValue) != null;

  const handleGroupModeChange = useCallback(
    (next: ChartGroupMode) => {
      setGroupMode(next);
      savePrefs(next, interval, style, showTradeMarkers, showAthLine, showMigrationLine);
      onBarClickRef.current?.(null);
    },
    [interval, style, showTradeMarkers, showAthLine, showMigrationLine],
  );

  const handleIntervalChange = useCallback(
    (next: ChartInterval) => {
      setInterval(next);
      savePrefs(groupMode, next, style, showTradeMarkers, showAthLine, showMigrationLine);
      onBarClickRef.current?.(null);
    },
    [groupMode, style, showTradeMarkers, showAthLine, showMigrationLine],
  );

  const handleStyleChange = useCallback(
    (next: ChartStyle) => {
      setStyle(next);
      savePrefs(groupMode, interval, next, showTradeMarkers, showAthLine, showMigrationLine);
    },
    [groupMode, interval, showTradeMarkers, showAthLine, showMigrationLine],
  );

  const handleShowTradeMarkersChange = useCallback(
    (next: boolean) => {
      setShowTradeMarkers(next);
      savePrefs(groupMode, interval, style, next, showAthLine, showMigrationLine);
    },
    [groupMode, interval, style, showAthLine, showMigrationLine],
  );

  const handleShowAthLineChange = useCallback(
    (next: boolean) => {
      setShowAthLine(next);
      savePrefs(groupMode, interval, style, showTradeMarkers, next, showMigrationLine);
    },
    [groupMode, interval, style, showTradeMarkers, showMigrationLine],
  );

  const handleShowMigrationLineChange = useCallback(
    (next: boolean) => {
      setShowMigrationLine(next);
      savePrefs(groupMode, interval, style, showTradeMarkers, showAthLine, next);
    },
    [groupMode, interval, style, showTradeMarkers, showAthLine],
  );

  const showChart = Boolean(id) && !loading && !error && trades.length > 0 && bars.length > 0;

  useEffect(() => {
    if (prevIdRef.current !== id || prevGroupingKeyRef.current !== groupingKey) {
      shouldFitContentRef.current = true;
      prevIdRef.current = id;
      prevGroupingKeyRef.current = groupingKey;
    }
  }, [id, groupingKey]);

  useEffect(() => {
    if (!showChart) return;

    const el = containerRef.current;
    if (!el) return;

    const rect = el.getBoundingClientRect();
    const chart = createChart(
      el,
      createChartOptions(rect.width || el.clientWidth, height, groupMode, priceUnit),
    );
    chartRef.current = chart;

    const ro = new ResizeObserver((entries) => {
      const { width, height: h } = entries[0]?.contentRect ?? { width: 0, height: 0 };
      if (width > 0 && h > 0) chartRef.current?.applyOptions({ width, height: h });
    });
    ro.observe(el);

    chart.subscribeCrosshairMove((param) => {
      if (!param.time) {
        setCrosshair(null);
        return;
      }
      const bar = barsRef.current.find((b) => b.time === param.time);
      if (!bar) {
        setCrosshair(null);
        return;
      }
      setCrosshair({
        open: bar.open,
        high: bar.high,
        low: bar.low,
        close: bar.close,
        volume: bar.volume,
      });
    });

    const groupModeAtMount = groupMode;
    const intervalAtMount = intervalSec;
    chart.subscribeClick((param) => {
      if (!param.time) {
        onBarClickRef.current?.(null);
        return;
      }
      const selection: ChartBarSelection =
        groupModeAtMount === 'slot'
          ? {
              barTime: param.time as UTCTimestamp,
              groupMode: 'slot',
              slot: param.time as number,
            }
          : {
              barTime: param.time as UTCTimestamp,
              groupMode: 'time',
              intervalSec: intervalAtMount,
            };
      onBarClickRef.current?.(selection);
    });

    return () => {
      ro.disconnect();
      markersPluginRef.current?.detach();
      markersPluginRef.current = null;
      seriesRef.current = null;
      chart.remove();
      chartRef.current = null;
      setCrosshair(null);
    };
  }, [showChart, height, groupingKey, groupMode, priceUnit]);

  useEffect(() => {
    const chart = chartRef.current;
    const series = seriesRef.current;
    if (!chart || !series || !showChart) return;

    const priceFormat = createChartPriceFormat(priceUnit);
    chart.applyOptions({
      localization: { priceFormatter: priceFormat.formatter },
    });
    series.applyOptions({ priceFormat });
  }, [priceUnit, showChart]);

  useEffect(() => {
    const chart = chartRef.current;
    if (!chart || !showChart || bars.length === 0) return;

    const ts = chart.timeScale();
    const savedRange = shouldFitContentRef.current ? null : ts.getVisibleLogicalRange();

    if (seriesRef.current) {
      if (athLineRef.current) {
        seriesRef.current.removePriceLine(athLineRef.current);
        athLineRef.current = null;
      }
      if (migrationLineRef.current) {
        seriesRef.current.removePriceLine(migrationLineRef.current);
        migrationLineRef.current = null;
      }
      markersPluginRef.current?.detach();
      markersPluginRef.current = null;
      chart.removeSeries(seriesRef.current);
      seriesRef.current = null;
    }

    const SeriesCtor = SERIES_BY_STYLE[style];
    const baseOptions = style === 'line' ? LINE_SERIES_OPTIONS : CANDLE_SERIES_OPTIONS;
    const series = chart.addSeries(SeriesCtor, {
      ...baseOptions,
      priceFormat: createChartPriceFormat(priceUnit),
    });
    seriesRef.current = series as ISeriesApi<'Line' | 'Candlestick'>;

    if (style === 'line') {
      series.setData(barsToLineData(bars));
    } else {
      series.setData(barsToCandleData(bars));
    }

    if (shouldFitContentRef.current) {
      ts.fitContent();
      shouldFitContentRef.current = false;
    } else if (savedRange) {
      ts.setVisibleLogicalRange(savedRange);
    }
  }, [bars, style, showChart, groupingKey, priceUnit]);

  useEffect(() => {
    const series = seriesRef.current;
    if (!series || !showChart) return;

    if (!showTradeMarkers) {
      markersPluginRef.current?.setMarkers([]);
      return;
    }

    const markers = buildTradeMarkers(trades, groupMode, intervalSec);
    if (markersPluginRef.current) {
      markersPluginRef.current.setMarkers(markers);
    } else {
      markersPluginRef.current = createSeriesMarkers(series, markers) as MarkersPlugin;
    }
  }, [showTradeMarkers, trades, groupMode, intervalSec, showChart, bars]);

  useEffect(() => {
    const series = seriesRef.current;
    if (!series || !showChart) return;

    if (athLineRef.current) {
      series.removePriceLine(athLineRef.current);
      athLineRef.current = null;
    }

    if (!showAthLine || !athLineAvailable) return;

    const price = athChartValue(athPriceInSol, metric, toValue);
    if (price == null) return;

    athLineRef.current = series.createPriceLine({
      price,
      color: CHART_COLORS.athLine,
      lineWidth: 1,
      lineStyle: LineStyle.Dashed,
      axisLabelVisible: true,
      title: 'ATH',
    });
  }, [
    showAthLine,
    athLineAvailable,
    athPriceInSol,
    metric,
    toValue,
    showChart,
    bars,
    style,
  ]);

  useEffect(() => {
    const series = seriesRef.current;
    if (!series || !showChart) return;

    if (migrationLineRef.current) {
      series.removePriceLine(migrationLineRef.current);
      migrationLineRef.current = null;
    }

    if (!showMigrationLine) return;

    const price = migrationChartValue(metric, toValue);

    migrationLineRef.current = series.createPriceLine({
      price,
      color: CHART_COLORS.migrationLine,
      lineWidth: 1,
      lineStyle: LineStyle.Dashed,
      axisLabelVisible: true,
      title: 'Migration',
    });
  }, [showMigrationLine, metric, toValue, showChart, bars, style]);

  if (!id) {
    return (
      <Placeholder
        message="Select a token row to view price history."
        className={className}
        height={height}
      />
    );
  }

  if (loading) {
    return (
      <Placeholder
        message={`Loading trades for ${symbol}…`}
        className={className}
        height={height}
      />
    );
  }

  if (error) {
    return (
      <div
        className={panelClass(cn('flex items-center justify-center p-4 text-xs text-red', className))}
        style={{ ...panelStyle, height, borderColor: '#f2364544' }}
      >
        {error}
      </div>
    );
  }

  if (trades.length === 0) {
    return (
      <Placeholder
        message={`No trades recorded for ${symbol}.`}
        className={className}
        height={height}
      />
    );
  }

  if (bars.length === 0) {
    return (
      <Placeholder
        message={`No chart data for ${symbol} at ${groupMode === 'slot' ? 'slot' : interval} grouping.`}
        className={className}
        height={height}
      />
    );
  }

  return (
    <div className={panelClass(className)} style={panelStyle}>
      <ChartToolbar
        symbol={symbol}
        groupMode={groupMode}
        interval={interval}
        style={style}
        priceLabel={priceLabel}
        priceUnit={priceUnit}
        metric={metric}
        tradeCount={trades.length}
        showTradeMarkers={showTradeMarkers}
        showAthLine={showAthLine}
        athLineAvailable={athLineAvailable}
        showMigrationLine={showMigrationLine}
        crosshair={crosshair}
        isMigrated={isMigrated}
        isMayhemMode={isMayhemMode}
        isCashbackEnabled={isCashbackEnabled}
        onGroupModeChange={handleGroupModeChange}
        onIntervalChange={handleIntervalChange}
        onStyleChange={handleStyleChange}
        onMetricChange={onMetricChange}
        onShowTradeMarkersChange={handleShowTradeMarkersChange}
        onShowAthLineChange={handleShowAthLineChange}
        onShowMigrationLineChange={handleShowMigrationLineChange}
      />
      <div ref={containerRef} style={{ height, width: '100%' }} />
    </div>
  );
}
