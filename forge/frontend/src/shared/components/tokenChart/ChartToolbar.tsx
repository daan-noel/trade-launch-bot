import { useMemo, type ReactNode } from 'react';
import {
  CHART_COLORS,
  CHART_GROUP_MODES,
  CHART_GROUP_MODE_LABELS,
  CHART_INTERVAL_LABELS,
  CHART_STYLES,
  CHART_STYLE_LABELS,
  createChartPriceFormatter,
} from './constants';
import { BarCrosshairFields } from './BarCrosshairFields';
import { cn } from '@shared/lib/cn';
import type { ChartMetric, ChartStyle, ChartToolbarProps } from './types';

const CHART_METRICS: ChartMetric[] = ['price', 'mc'];

const CHART_METRIC_LABELS: Record<ChartMetric, string> = {
  price: 'Price',
  mc: 'MC',
};

/** High-contrast badge hues on the dark chart toolbar. */
const STATUS_BADGE_COLOR = {
  migrated: '#34d399',
  bonding: '#38bdf8',
} as const;

function statusBadgeStyle(color: string) {
  return {
    color,
    border: `1px solid ${color}99`,
    backgroundColor: `${color}30`,
  };
}

function StatusBadge({ label, color }: { label: string; color: string }) {
  return (
    <span
      className="rounded px-1.5 py-px text-[10px] font-bold tracking-wide"
      style={statusBadgeStyle(color)}
    >
      {label}
    </span>
  );
}

/** Brackets around a dashed span — drag-to-select a time range. */
function RangeSelectIcon() {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden className="size-3.5">
      <path
        d="M6 4H4v12h2M14 4h2v12h-2"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path
        d="M7.5 10h5"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeDasharray="1.5 2"
      />
    </svg>
  );
}

/** Up (buy) + down (sell) arrows — per-bar buy/sell count markers. */
function BuySellCountsIcon() {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden className="size-3.5">
      <path
        d="M6 14V5m0 0L3.5 7.5M6 5l2.5 2.5"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path
        d="M14 6v9m0 0 2.5-2.5M14 15l-2.5-2.5"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

/** Two edges with inward arrows — collapse the empty gaps between candles. */
function TrimGapsIcon() {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden className="size-3.5">
      <path d="M3 4.5v11M17 4.5v11" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      <path
        d="M6 10h3.5M9.5 10 7.8 8.3M9.5 10 7.8 11.7"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path
        d="M14 10h-3.5M10.5 10l1.7-1.7M10.5 10l1.7 1.7"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

/** Two filled candlesticks with wicks — candlestick chart style. */
function CandlesIcon() {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden className="size-3.5">
      <path d="M7 2.5v3.5M7 13.5V17" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      <rect x="4.75" y="6" width="4.5" height="7.5" rx="1" fill="currentColor" />
      <path d="M14 5v2M14 13v2.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      <rect x="11.75" y="7" width="4.5" height="6" rx="1" fill="currentColor" />
    </svg>
  );
}

/** Rising zigzag — line chart style. */
function LineIcon() {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden className="size-3.5">
      <path
        d="M3 13l4-4 3 3 7-7"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

const CHART_STYLE_ICONS: Record<ChartStyle, ReactNode> = {
  candles: <CandlesIcon />,
  line: <LineIcon />,
};

/**
 * Instant dark tooltip shown below its `group` parent on hover (icon-only
 * controls). Anchored to the right edge (opens leftward), NOT centred: every
 * control that uses this sits in the toolbar's right-aligned cluster, so a
 * centred `whitespace-nowrap` tooltip would spill past the viewport's right edge.
 */
function HoverTooltip({ children }: { children: ReactNode }) {
  return (
    <span
      role="tooltip"
      className="pointer-events-none absolute right-0 top-full z-50 mt-1.5 whitespace-nowrap rounded px-2 py-1 text-[10px] font-medium opacity-0 shadow-lg transition-opacity duration-100 group-hover:opacity-100"
      style={{
        backgroundColor: '#0a0a0a',
        color: CHART_COLORS.panelText,
        border: `1px solid ${CHART_COLORS.border}`,
      }}
    >
      {children}
    </span>
  );
}

/**
 * Square icon toggle for the toolbar with an instant hover tooltip — the label
 * lives in the tooltip since the button is icon-only.
 */
function IconToggleButton({
  active,
  onClick,
  label,
  tooltip,
  disabled = false,
  activeColor = CHART_COLORS.activePill,
  children,
}: {
  active: boolean;
  onClick: () => void;
  label: string;
  tooltip: string;
  disabled?: boolean;
  activeColor?: string;
  children: ReactNode;
}) {
  return (
    <div className="group relative inline-flex">
      <button
        type="button"
        onClick={onClick}
        disabled={disabled}
        aria-label={label}
        aria-pressed={active}
        className={cn(
          'flex size-7 items-center justify-center rounded-md transition-colors',
          active ? 'text-[#0a0a0a]' : 'hover:text-white',
          disabled && 'cursor-not-allowed opacity-40 hover:text-inherit',
        )}
        style={
          active
            ? { backgroundColor: activeColor }
            : { backgroundColor: CHART_COLORS.grid, color: CHART_COLORS.panelTextDim }
        }
      >
        {children}
      </button>
      <HoverTooltip>{tooltip}</HoverTooltip>
    </div>
  );
}

export function ChartToolbar({
  symbol,
  groupMode,
  interval,
  style,
  priceLabel,
  priceUnit = 'SOL',
  metric,
  tradeCount,
  showTradeMarkers,
  showMigrationLine,
  trimEmptyBars,
  rangeSelectMode,
  crosshair,
  isMigrated,
  onGroupModeChange,
  onIntervalChange,
  onStyleChange,
  onMetricChange,
  onShowTradeMarkersChange,
  onShowMigrationLineChange,
  onTrimEmptyBarsChange,
  onRangeSelectModeChange,
}: ChartToolbarProps) {
  const intervalsDisabled = groupMode === 'slot';
  // Memoize so crosshair-move re-renders don't rebuild the formatters per pixel.
  const formatChartPrice = useMemo(() => createChartPriceFormatter(priceUnit), [priceUnit]);
  const formatVol = useMemo(() => createChartPriceFormatter('SOL'), []);

  const crosshairLine = crosshair ? (
    <BarCrosshairFields
      style={style}
      crosshair={crosshair}
      formatPrice={formatChartPrice}
      formatVol={formatVol}
      layout="inline"
    />
  ) : null;

  const showStatusBadges = isMigrated != null;

  return (
    <div
      className="flex items-start gap-3 border-b px-3 py-2"
      style={{ borderColor: CHART_COLORS.border }}
    >
      {/* Left: title, status badge, crosshair OHLCV */}
      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <div
            className="min-w-0 truncate text-[13px] font-bold"
            style={{ color: CHART_COLORS.panelText }}
          >
            {symbol}{' '}
            <span className="font-normal" style={{ color: CHART_COLORS.panelTextDim }}>
              · {groupMode === 'slot' ? 'slot' : interval} · {CHART_STYLE_LABELS[style]} · {tradeCount}{' '}
              trades · {priceLabel}
            </span>
          </div>
          {showStatusBadges && (
            <div className="flex shrink-0 items-center gap-1.5">
              <StatusBadge
                label={isMigrated ? 'Migrated ✓' : 'Bonding Curve'}
                color={isMigrated ? STATUS_BADGE_COLOR.migrated : STATUS_BADGE_COLOR.bonding}
              />
            </div>
          )}
        </div>
        <div
          className="mt-0.5 overflow-hidden font-mono text-[11px] leading-[14px]"
          aria-live="polite"
        >
          {crosshairLine ?? (
            <div aria-hidden="true" className="invisible select-none">
              <div>—</div>
              <div>—</div>
              {style === 'candles' && <div>—</div>}
            </div>
          )}
        </div>
      </div>

      {/* Right: controls, right-aligned */}
      <div className="flex shrink-0 flex-col items-end gap-1.5">
        <div className="flex flex-wrap items-center justify-end gap-2">
          <div
            className="flex rounded-md p-0.5"
            style={{ backgroundColor: CHART_COLORS.grid }}
          >
            {CHART_GROUP_MODES.map((key) => (
              <button
                key={key}
                type="button"
                onClick={() => onGroupModeChange(key)}
                className={cn(
                  'rounded px-2 py-0.5 text-[11px] font-semibold transition-colors',
                  groupMode === key ? 'text-[#0a0a0a]' : 'hover:text-white',
                )}
                style={
                  groupMode === key
                    ? { backgroundColor: CHART_COLORS.activePill }
                    : { color: CHART_COLORS.panelTextDim }
                }
              >
                {CHART_GROUP_MODE_LABELS[key]}
              </button>
            ))}
          </div>

          <div
            className={cn('flex rounded-md p-0.5', intervalsDisabled && 'opacity-40')}
            style={{ backgroundColor: CHART_COLORS.grid }}
          >
            {CHART_INTERVAL_LABELS.map((key) => (
              <button
                key={key}
                type="button"
                disabled={intervalsDisabled}
                onClick={() => onIntervalChange(key)}
                className={cn(
                  'rounded px-2 py-0.5 text-[11px] font-semibold transition-colors',
                  interval === key ? 'text-[#0a0a0a]' : 'hover:text-white',
                  intervalsDisabled && 'cursor-not-allowed hover:text-inherit',
                )}
                style={
                  interval === key
                    ? { backgroundColor: CHART_COLORS.activePill }
                    : { color: CHART_COLORS.panelTextDim }
                }
              >
                {key}
              </button>
            ))}
          </div>

          <div
            className="flex rounded-md p-0.5"
            style={{ backgroundColor: CHART_COLORS.grid }}
          >
            {CHART_STYLES.map((key) => (
              <div key={key} className="group relative inline-flex">
                <button
                  type="button"
                  onClick={() => onStyleChange(key)}
                  aria-label={`${CHART_STYLE_LABELS[key]} chart`}
                  aria-pressed={style === key}
                  className={cn(
                    'flex items-center justify-center rounded px-2.5 py-0.5 transition-colors',
                    style === key ? 'text-[#0a0a0a]' : 'hover:text-white',
                  )}
                  style={
                    style === key
                      ? { backgroundColor: CHART_COLORS.activePill }
                      : { color: CHART_COLORS.panelTextDim }
                  }
                >
                  {CHART_STYLE_ICONS[key]}
                </button>
                <HoverTooltip>{CHART_STYLE_LABELS[key]}</HoverTooltip>
              </div>
            ))}
          </div>

          {onMetricChange && metric != null && (
            <div
              className="flex rounded-md p-0.5"
              style={{ backgroundColor: CHART_COLORS.grid }}
            >
              {CHART_METRICS.map((key) => (
                <button
                  key={key}
                  type="button"
                  onClick={() => onMetricChange(key)}
                  className={cn(
                    'rounded px-2 py-0.5 text-[11px] font-semibold transition-colors',
                    metric === key ? 'text-[#0a0a0a]' : 'hover:text-white',
                  )}
                  style={
                    metric === key
                      ? { backgroundColor: CHART_COLORS.activePill }
                      : { color: CHART_COLORS.panelTextDim }
                  }
                >
                  {CHART_METRIC_LABELS[key]}
                </button>
              ))}
            </div>
          )}

          <IconToggleButton
            active={showTradeMarkers}
            onClick={() => onShowTradeMarkersChange(!showTradeMarkers)}
            label="Toggle buy/sell counts per bar"
            tooltip="Buy/sell counts per bar"
          >
            <BuySellCountsIcon />
          </IconToggleButton>

          <IconToggleButton
            active={trimEmptyBars}
            onClick={() => onTrimEmptyBarsChange(!trimEmptyBars)}
            label="Toggle trimming of empty candles"
            tooltip="Hide flat candles for intervals with no trades"
          >
            <TrimGapsIcon />
          </IconToggleButton>

          <IconToggleButton
            active={rangeSelectMode}
            onClick={() => onRangeSelectModeChange(!rangeSelectMode)}
            label="Toggle range-select mode"
            tooltip="Drag to select a time range; hover its label for totals"
          >
            <RangeSelectIcon />
          </IconToggleButton>

          <label
            className="flex cursor-pointer items-center gap-1.5 rounded-md px-2 py-1 text-[11px] font-semibold"
            style={{ backgroundColor: CHART_COLORS.grid, color: CHART_COLORS.panelTextDim }}
            title="Show pump.fun bonding-curve graduation price"
          >
            <input
              type="checkbox"
              checked={showMigrationLine}
              onChange={(e) => onShowMigrationLineChange(e.target.checked)}
              className="h-3.5 w-3.5 accent-[#5dade2]"
            />
            <span style={showMigrationLine ? { color: CHART_COLORS.migrationLine } : undefined}>
              Migration
            </span>
          </label>
        </div>
      </div>
    </div>
  );
}
