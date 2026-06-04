import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  createChart,
  createSeriesMarkers,
  LineSeries,
  LineStyle,
  type IChartApi,
  type IPriceLine,
  type ISeriesApi,
  type LogicalRangeChangeEventHandler,
  type SeriesMarker,
  type UTCTimestamp,
} from 'lightweight-charts';
import {
  barsSignature,
  captureChartViewport,
  reapplyChartViewport,
  type ChartViewport,
} from './chartViewport';
import {
  aggregateTradesToBars,
  aggregateTradesToBarsBySlot,
  athChartValue,
  migrationChartValue,
  barsToCandleData,
  barsToLineData,
  tradeBarSlot,
  tradeBarTime,
} from './chartBars';
import { ChartToolbar } from './ChartToolbar';
import {
  createChartTimeFormatters,
  getDefaultChartTimezone,
  isValidTimezone,
} from './chartTimezone';
import { cn } from './cn';
import {
  CANDLE_SERIES_OPTIONS,
  CHART_COLORS,
  CHART_INTERVALS,
  createChartOptions,
  createChartPriceFormat,
  createChartPriceFormatter,
  DEFAULT_CHART_PREFS,
  LINE_SERIES_OPTIONS,
  LS_CHART_PREFS_KEY,
  SERIES_BY_STYLE,
  SWING_HIGH_OVERLAY_SERIES_OPTIONS,
  TOKEN_TOTAL_SUPPLY,
} from './constants';
import { BarCrosshairTooltip } from './BarCrosshairTooltip';
import { SwingCrosshairTooltip } from './SwingCrosshairTooltip';
import {
  buildLegSegment,
  findSwingLegIndexAtChartTime,
  swingsToColoredLineData,
} from './swingOverlay';
import type {
  ChartBarSelection,
  ChartCrosshairInfo,
  ChartGroupMode,
  ChartInterval,
  ChartStyle,
  ChartSwingLeg,
  ChartBarTooltipState,
  ChartSwingTooltipState,
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
  chartTimezone: string;
} {
  const defaultTimezone = getDefaultChartTimezone();
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
        chartTimezone?: string;
      };
      const tz = parsed.chartTimezone;
      return {
        groupMode: parsed.groupMode ?? DEFAULT_CHART_PREFS.groupMode,
        interval: parsed.interval ?? DEFAULT_CHART_PREFS.interval,
        style: parsed.style ?? DEFAULT_CHART_PREFS.style,
        showTradeMarkers: parsed.showTradeMarkers ?? DEFAULT_CHART_PREFS.showTradeMarkers,
        showAthLine: parsed.showAthLine ?? DEFAULT_CHART_PREFS.showAthLine,
        showMigrationLine:
          parsed.showMigrationLine ?? DEFAULT_CHART_PREFS.showMigrationLine,
        chartTimezone:
          typeof tz === 'string' && isValidTimezone(tz) ? tz : defaultTimezone,
      };
    }
  } catch {
    /* ignore */
  }
  return { ...DEFAULT_CHART_PREFS, chartTimezone: defaultTimezone };
}

function savePrefs(
  groupMode: ChartGroupMode,
  interval: ChartInterval,
  style: ChartStyle,
  showTradeMarkers: boolean,
  showAthLine: boolean,
  showMigrationLine: boolean,
  chartTimezone: string,
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
        chartTimezone,
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
  swingOverlay = null,
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
  const swingSeriesRefs = useRef<ISeriesApi<'Line'>[]>([]);
  const swingSeriesLegRef = useRef(new Map<ISeriesApi<'Line'>, ChartSwingLeg>());
  const swingOverlayMetaRef = useRef<{
    segmentMode: 'connected' | 'perLeg';
    groupMode: ChartGroupMode;
    legs: ChartSwingLeg[];
  } | null>(null);
  const showSwingOverlayRef = useRef(true);
  const sortedTradesRef = useRef<ChartTrade[]>([]);
  const markersPluginRef = useRef<MarkersPlugin | null>(null);
  const barsRef = useRef<OhlcBar[]>([]);

  const initialPrefs = loadPrefs();
  const [groupMode, setGroupMode] = useState<ChartGroupMode>(initialPrefs.groupMode);
  const [interval, setInterval] = useState<ChartInterval>(initialPrefs.interval);
  const [style, setStyle] = useState<ChartStyle>(initialPrefs.style);
  const [showTradeMarkers, setShowTradeMarkers] = useState(initialPrefs.showTradeMarkers);
  const [showAthLine, setShowAthLine] = useState(initialPrefs.showAthLine);
  const [showMigrationLine, setShowMigrationLine] = useState(initialPrefs.showMigrationLine);
  const [chartTimezone, setChartTimezone] = useState(initialPrefs.chartTimezone);
  const swingOverlayAvailable = (swingOverlay?.legs.length ?? 0) > 0;
  const [showSwingOverlay, setShowSwingOverlay] = useState(true);
  const [crosshair, setCrosshair] = useState<ChartCrosshairInfo | null>(null);
  const [barTooltip, setBarTooltip] = useState<ChartBarTooltipState | null>(null);
  const [swingTooltip, setSwingTooltip] = useState<ChartSwingTooltipState | null>(null);
  const styleRef = useRef(style);
  styleRef.current = style;
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
  const visibleViewportRef = useRef<ChartViewport | null>(null);
  const isRestoringViewportRef = useRef(false);
  const mountedSeriesStyleRef = useRef<ChartStyle | null>(null);
  const prevBarsSignatureRef = useRef<string | null>(null);
  const horzStepRef = useRef(intervalSec);
  horzStepRef.current = groupMode === 'slot' ? 1 : intervalSec;

  const snapshotVisibleViewport = useCallback(
    (chart: IChartApi, series: ISeriesApi<'Line' | 'Candlestick'>): ChartViewport | null => {
      if (shouldFitContentRef.current) return null;
      const logical = chart.timeScale().getVisibleLogicalRange();
      if (!logical) return null;
      return captureChartViewport(series, logical, horzStepRef.current);
    },
    [],
  );

  const sortedTrades = useMemo(
    () =>
      [...trades].sort(
        (a, b) => Date.parse(a.block_time) - Date.parse(b.block_time),
      ),
    [trades],
  );

  const bars = useMemo(
    () =>
      groupMode === 'slot'
        ? aggregateTradesToBarsBySlot(sortedTrades, toValue, metric)
        : aggregateTradesToBars(sortedTrades, intervalSec, toValue, metric),
    [sortedTrades, groupMode, intervalSec, toValue, metric],
  );
  barsRef.current = bars;
  sortedTradesRef.current = sortedTrades;
  showSwingOverlayRef.current = showSwingOverlay;

  const formatChartPrice = useMemo(
    () => createChartPriceFormatter(priceUnit),
    [priceUnit],
  );
  const formatSwingPrice = useCallback(
    (priceInSol: number) => {
      const chartY = metric === 'price' ? priceInSol : TOKEN_TOTAL_SUPPLY * priceInSol;
      return formatChartPrice(toValue(chartY));
    },
    [metric, toValue, formatChartPrice],
  );
  const formatSwingAmount = useCallback(
    (sol: number) => formatChartPrice(toValue(sol)),
    [toValue, formatChartPrice],
  );
  const formatBarTime = useCallback(
    (barTime: UTCTimestamp) => {
      if (groupMode === 'slot') return `Slot ${barTime}`;
      return createChartTimeFormatters(chartTimezone).timeFormatter(barTime);
    },
    [groupMode, chartTimezone],
  );
  const formatVol = useMemo(() => createChartPriceFormatter('SOL'), []);

  const athLineAvailable = athChartValue(athPriceInSol, metric, toValue) != null;

  useEffect(() => {
    if (swingOverlayAvailable) setShowSwingOverlay(true);
  }, [swingOverlayAvailable, swingOverlay?.legs]);

  const handleGroupModeChange = useCallback(
    (next: ChartGroupMode) => {
      setGroupMode(next);
      savePrefs(
        next,
        interval,
        style,
        showTradeMarkers,
        showAthLine,
        showMigrationLine,
        chartTimezone,
      );
      onBarClickRef.current?.(null);
    },
    [interval, style, showTradeMarkers, showAthLine, showMigrationLine, chartTimezone],
  );

  const handleIntervalChange = useCallback(
    (next: ChartInterval) => {
      setInterval(next);
      savePrefs(
        groupMode,
        next,
        style,
        showTradeMarkers,
        showAthLine,
        showMigrationLine,
        chartTimezone,
      );
      onBarClickRef.current?.(null);
    },
    [groupMode, style, showTradeMarkers, showAthLine, showMigrationLine, chartTimezone],
  );

  const handleStyleChange = useCallback(
    (next: ChartStyle) => {
      setStyle(next);
      savePrefs(
        groupMode,
        interval,
        next,
        showTradeMarkers,
        showAthLine,
        showMigrationLine,
        chartTimezone,
      );
    },
    [groupMode, interval, showTradeMarkers, showAthLine, showMigrationLine, chartTimezone],
  );

  const handleShowTradeMarkersChange = useCallback(
    (next: boolean) => {
      setShowTradeMarkers(next);
      savePrefs(
        groupMode,
        interval,
        style,
        next,
        showAthLine,
        showMigrationLine,
        chartTimezone,
      );
    },
    [groupMode, interval, style, showAthLine, showMigrationLine, chartTimezone],
  );

  const handleShowAthLineChange = useCallback(
    (next: boolean) => {
      setShowAthLine(next);
      savePrefs(
        groupMode,
        interval,
        style,
        showTradeMarkers,
        next,
        showMigrationLine,
        chartTimezone,
      );
    },
    [groupMode, interval, style, showTradeMarkers, showMigrationLine, chartTimezone],
  );

  const handleShowMigrationLineChange = useCallback(
    (next: boolean) => {
      setShowMigrationLine(next);
      savePrefs(
        groupMode,
        interval,
        style,
        showTradeMarkers,
        showAthLine,
        next,
        chartTimezone,
      );
    },
    [groupMode, interval, style, showTradeMarkers, showAthLine, chartTimezone],
  );

  const handleChartTimezoneChange = useCallback(
    (next: string) => {
      if (!isValidTimezone(next)) return;
      setChartTimezone(next);
      savePrefs(
        groupMode,
        interval,
        style,
        showTradeMarkers,
        showAthLine,
        showMigrationLine,
        next,
      );
    },
    [groupMode, interval, style, showTradeMarkers, showAthLine, showMigrationLine],
  );

  const showChart = Boolean(id) && !loading && !error && trades.length > 0 && bars.length > 0;

  useEffect(() => {
    if (prevIdRef.current !== id || prevGroupingKeyRef.current !== groupingKey) {
      shouldFitContentRef.current = true;
      visibleViewportRef.current = null;
      prevBarsSignatureRef.current = null;
      mountedSeriesStyleRef.current = null;
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
      createChartOptions(
        rect.width || el.clientWidth,
        height,
        groupMode,
        priceUnit,
        chartTimezone,
      ),
    );
    chartRef.current = chart;

    const ro = new ResizeObserver((entries) => {
      const { width, height: h } = entries[0]?.contentRect ?? { width: 0, height: 0 };
      if (width > 0 && h > 0) chartRef.current?.applyOptions({ width, height: h });
    });
    ro.observe(el);

    chart.subscribeCrosshairMove((param) => {
      const hovered =
        param.hoveredSeries ?? param.hoveredInfo?.series;
      const onSwingSeries =
        hovered != null &&
        swingSeriesRefs.current.includes(hovered as ISeriesApi<'Line'>);

      let activeSwingLeg: ChartSwingLeg | undefined;
      if (!onSwingSeries || !param.point || !showSwingOverlayRef.current) {
        setSwingTooltip(null);
      } else {
        let leg = swingSeriesLegRef.current.get(hovered as ISeriesApi<'Line'>);
        const meta = swingOverlayMetaRef.current;
        if (!leg && meta?.legs.length) {
          const seriesData = param.seriesData.get(hovered);
          const chartTime =
            seriesData && 'time' in seriesData
              ? (seriesData.time as number)
              : typeof param.time === 'number'
                ? param.time
                : undefined;
          if (chartTime != null) {
            const idx = findSwingLegIndexAtChartTime(
              meta.legs,
              chartTime,
              meta.groupMode,
              sortedTradesRef.current,
              meta.segmentMode,
            );
            if (idx != null) leg = meta.legs[idx];
          }
        }
        if (leg) {
          activeSwingLeg = leg;
          setSwingTooltip({ leg, point: param.point });
        } else {
          setSwingTooltip(null);
        }
      }

      if (!param.time) {
        setCrosshair(null);
        setBarTooltip(null);
        return;
      }
      const bar = barsRef.current.find((b) => b.time === param.time);
      if (!bar) {
        setCrosshair(null);
        setBarTooltip(null);
        return;
      }
      const info: ChartCrosshairInfo = {
        open: bar.open,
        high: bar.high,
        low: bar.low,
        close: bar.close,
        volume: bar.volume,
        liquiditySol: bar.liquiditySol,
      };
      setCrosshair(info);
      if (param.point && !activeSwingLeg) {
        setBarTooltip({
          ...info,
          barTime: bar.time,
          style: styleRef.current,
          point: param.point,
        });
      } else {
        setBarTooltip(null);
      }
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

    const onVisibleLogicalRangeChange: LogicalRangeChangeEventHandler = (logical) => {
      if (shouldFitContentRef.current || isRestoringViewportRef.current || logical == null) {
        return;
      }
      const series = seriesRef.current;
      if (!series) return;
      visibleViewportRef.current = captureChartViewport(
        series,
        logical,
        horzStepRef.current,
      );
    };
    chart.timeScale().subscribeVisibleLogicalRangeChange(onVisibleLogicalRangeChange);

    return () => {
      chart.timeScale().unsubscribeVisibleLogicalRangeChange(onVisibleLogicalRangeChange);
      ro.disconnect();
      markersPluginRef.current?.detach();
      markersPluginRef.current = null;
      seriesRef.current = null;
      swingSeriesRefs.current = [];
      chart.remove();
      chartRef.current = null;
      setCrosshair(null);
      setBarTooltip(null);
      setSwingTooltip(null);
    };
  }, [showChart, height, groupingKey, groupMode, priceUnit, chartTimezone]);

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
    if (!chart || !showChart || groupMode !== 'time') return;
    const { timeFormatter, tickMarkFormatter } = createChartTimeFormatters(chartTimezone);
    chart.applyOptions({
      localization: { timeFormatter },
      timeScale: { tickMarkFormatter },
    });
  }, [chartTimezone, groupMode, showChart]);

  useEffect(() => {
    const chart = chartRef.current;
    if (!chart || !showChart || bars.length === 0) return;

    const ts = chart.timeScale();
    const sig = barsSignature(bars);
    const barsShapeChanged =
      prevBarsSignatureRef.current != null && prevBarsSignatureRef.current !== sig;
    prevBarsSignatureRef.current = sig;

    const existing = seriesRef.current;
    const styleChanged = mountedSeriesStyleRef.current !== style;

    if (existing && !styleChanged) {
      const savedViewport = snapshotVisibleViewport(chart, existing);
      if (savedViewport) visibleViewportRef.current = savedViewport;

      if (style === 'line') {
        existing.setData(barsToLineData(bars));
      } else {
        existing.setData(barsToCandleData(bars));
      }

      if (savedViewport) {
        isRestoringViewportRef.current = true;
        reapplyChartViewport(ts, savedViewport, barsShapeChanged ? 'time' : 'logical');
        requestAnimationFrame(() => {
          isRestoringViewportRef.current = false;
        });
      }
      return;
    }

    const savedViewport =
      existing != null
        ? snapshotVisibleViewport(chart, existing)
        : shouldFitContentRef.current
          ? null
          : visibleViewportRef.current;
    if (savedViewport) visibleViewportRef.current = savedViewport;

    if (existing) {
      if (athLineRef.current) {
        existing.removePriceLine(athLineRef.current);
        athLineRef.current = null;
      }
      if (migrationLineRef.current) {
        existing.removePriceLine(migrationLineRef.current);
        migrationLineRef.current = null;
      }
      markersPluginRef.current?.detach();
      markersPluginRef.current = null;
      chart.removeSeries(existing);
      seriesRef.current = null;
    }

    const SeriesCtor = SERIES_BY_STYLE[style];
    const baseOptions = style === 'line' ? LINE_SERIES_OPTIONS : CANDLE_SERIES_OPTIONS;
    const series = chart.addSeries(SeriesCtor, {
      ...baseOptions,
      priceFormat: createChartPriceFormat(priceUnit),
    });
    seriesRef.current = series as ISeriesApi<'Line' | 'Candlestick'>;
    mountedSeriesStyleRef.current = style;

    if (style === 'line') {
      series.setData(barsToLineData(bars));
    } else {
      series.setData(barsToCandleData(bars));
    }

    if (shouldFitContentRef.current) {
      ts.fitContent();
      shouldFitContentRef.current = false;
      visibleViewportRef.current = null;
    } else if (savedViewport) {
      isRestoringViewportRef.current = true;
      reapplyChartViewport(ts, savedViewport, barsShapeChanged ? 'time' : 'logical');
      requestAnimationFrame(() => {
        isRestoringViewportRef.current = false;
      });
    }
  }, [bars, style, showChart, groupingKey, priceUnit, snapshotVisibleViewport]);

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
  }, [showTradeMarkers, trades, groupMode, intervalSec, showChart]);

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
  }, [showAthLine, athLineAvailable, athPriceInSol, metric, toValue, showChart, style]);

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
  }, [showMigrationLine, metric, toValue, showChart, style]);

  useEffect(() => {
    const chart = chartRef.current;
    if (!chart || !showChart) return;

    const mainSeries = seriesRef.current;
    const savedViewport =
      mainSeries != null ? snapshotVisibleViewport(chart, mainSeries) : null;
    if (savedViewport) visibleViewportRef.current = savedViewport;

    for (const series of swingSeriesRefs.current) {
      chart.removeSeries(series);
    }
    swingSeriesRefs.current = [];
    swingSeriesLegRef.current.clear();
    swingOverlayMetaRef.current = null;
    setSwingTooltip(null);

    if (!showSwingOverlay || !swingOverlay?.legs.length) {
      if (savedViewport) {
        isRestoringViewportRef.current = true;
        reapplyChartViewport(chart.timeScale(), savedViewport, 'logical');
        requestAnimationFrame(() => {
          isRestoringViewportRef.current = false;
        });
      }
      return;
    }

    const segmentMode = swingOverlay.segmentMode ?? 'connected';
    swingOverlayMetaRef.current = {
      segmentMode,
      groupMode,
      legs: swingOverlay.legs,
    };

    if (segmentMode === 'perLeg') {
      for (const leg of swingOverlay.legs) {
        const segment = buildLegSegment(leg, metric, toValue, groupMode, sortedTrades);
        if (!segment) continue;
        const series = chart.addSeries(LineSeries, {
          ...SWING_HIGH_OVERLAY_SERIES_OPTIONS,
          color: segment.color,
          priceFormat: createChartPriceFormat(priceUnit),
        });
        series.setData(segment.data);
        swingSeriesRefs.current.push(series);
        swingSeriesLegRef.current.set(series, leg);
      }
    } else {
      const data = swingsToColoredLineData(
        swingOverlay.legs,
        metric,
        toValue,
        groupMode,
        sortedTrades,
      );
      const pointCount = data.filter((p) => 'value' in p).length;
      if (pointCount < 2) return;

      const series = chart.addSeries(LineSeries, {
        ...SWING_HIGH_OVERLAY_SERIES_OPTIONS,
        priceFormat: createChartPriceFormat(priceUnit),
      });
      series.setData(data);
      swingSeriesRefs.current.push(series);
    }

    if (savedViewport) {
      isRestoringViewportRef.current = true;
      reapplyChartViewport(chart.timeScale(), savedViewport, 'logical');
      requestAnimationFrame(() => {
        isRestoringViewportRef.current = false;
      });
    }

    return () => {
      if (!chartRef.current) return;
      for (const series of swingSeriesRefs.current) {
        try {
          chartRef.current.removeSeries(series);
        } catch {
          /* chart may already be removed */
        }
      }
      swingSeriesRefs.current = [];
    };
  }, [
    showSwingOverlay,
    swingOverlay,
    metric,
    toValue,
    groupMode,
    sortedTrades,
    showChart,
    style,
    priceUnit,
    snapshotVisibleViewport,
  ]);

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
        swingOverlayAvailable={swingOverlayAvailable}
        showSwingOverlay={showSwingOverlay}
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
        onShowSwingOverlayChange={setShowSwingOverlay}
        chartTimezone={chartTimezone}
        onChartTimezoneChange={handleChartTimezoneChange}
      />
      <div className="relative" style={{ height, width: '100%' }}>
        <div ref={containerRef} style={{ height: '100%', width: '100%' }} />
        {barTooltip && !swingTooltip && (
          <BarCrosshairTooltip
            tooltip={barTooltip}
            formatPrice={formatChartPrice}
            formatVol={formatVol}
            formatTime={formatBarTime}
          />
        )}
        {swingTooltip && showSwingOverlay && (
          <SwingCrosshairTooltip
            tooltip={swingTooltip}
            formatPrice={formatSwingPrice}
            formatAmount={formatSwingAmount}
          />
        )}
      </div>
    </div>
  );
}
