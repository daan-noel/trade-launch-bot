import { useMemo, useState, type ReactNode } from 'react';
import {
  CHART_COLORS,
  CHART_GROUP_MODES,
  CHART_GROUP_MODE_LABELS,
  CHART_INTERVAL_LABELS,
  CHART_STYLES,
  CHART_STYLE_LABELS,
  createChartPriceFormatter,
} from './constants';
import { FLOW_NON_VOL_LINE_COLOR, FLOW_VOL_LINE_COLOR } from 'lib/flow/flowChartData';
import { BarCrosshairFields } from './BarCrosshairFields';
import { Checkbox } from 'components/ui/Checkbox';
import { cn } from 'lib/cn';
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
  mayhem: '#f43f5e',
  cashback: '#c084fc',
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
export function RangeSelectIcon() {
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

/** Two overlaid cumulative curves — vol (red) / non-vol (gold). */
export function FlowLinesIcon() {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden className="size-3.5">
      <path
        d="M3 15 C7 14 9 8 17 5"
        stroke={FLOW_VOL_LINE_COLOR}
        strokeWidth="1.6"
        strokeLinecap="round"
      />
      <path
        d="M3 16.5 C8 16 11 12 17 11"
        stroke={FLOW_NON_VOL_LINE_COLOR}
        strokeWidth="1.6"
        strokeLinecap="round"
      />
    </svg>
  );
}

/** Up (buy) + down (sell) arrows — per-bar buy/sell count markers. */
export function BuySellCountsIcon() {
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
export function TrimGapsIcon() {
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
export function CandlesIcon() {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden className="size-3.5">
      <path d="M7 2.5v3.5M7 13.5V17" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      <rect x="4.75" y="6" width="4.5" height="7.5" rx="1" fill="currentColor" />
      <path d="M14 5v2M14 13v2.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      <rect x="11.75" y="7" width="4.5" height="6" rx="1" fill="currentColor" />
    </svg>
  );
}

/** Map pin with a dot — tracked-wallet buy/sell markers. */
export function WalletMarkersIcon() {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden className="size-3.5">
      <path
        d="M10 2.5c-2.9 0-5.25 2.2-5.25 4.9 0 3.5 5.25 9.6 5.25 9.6s5.25-6.1 5.25-9.6c0-2.7-2.35-4.9-5.25-4.9Z"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinejoin="round"
      />
      <circle cx="10" cy="7.4" r="1.9" fill="currentColor" />
    </svg>
  );
}

/** Upward triangle with a "D" — the dev/creator marker silhouette. */
export function DevMarkersIcon() {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden className="size-3.5">
      <path
        d="M10 3.5 L17 16 L3 16 Z"
        fill="currentColor"
        opacity="0.25"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinejoin="round"
      />
      <text x="10" y="14.5" textAnchor="middle" fontSize="7" fontWeight="700" fill="currentColor">
        D
      </text>
    </svg>
  );
}

/** Two dashed horizontal lines with axis-label chips — the strategy entry/exit
 *  dashed fill-price lines. */
export function EntryExitLinesIcon() {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden className="size-3.5">
      <path
        d="M2 7h11"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeDasharray="2 2"
      />
      <rect x="14" y="5.4" width="4" height="3.2" rx="0.8" fill="currentColor" />
      <path
        d="M2 13h11"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeDasharray="2 2"
      />
      <rect x="14" y="11.4" width="4" height="3.2" rx="0.8" fill="currentColor" />
    </svg>
  );
}

/** Rising zigzag — line chart style. */
export function LineIcon() {
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
 * centred `whitespace-nowrap` tooltip would spill past the viewport's right
 * edge — and since it stays mounted (only faded via `opacity-0`), that spill
 * gives every chart page a permanent horizontal scrollbar even when nothing is
 * hovered. Right-anchoring keeps the whole tooltip on-screen with room to spare.
 */
export function HoverTooltip({ children }: { children: ReactNode }) {
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
 * lives in the tooltip since the button is icon-only. Mirrors the active-pill
 * styling of the other toolbar toggles.
 */
export function IconToggleButton({
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
  showWalletMarkers,
  showDevMarkers,
  devMarkersAvailable,
  devMarkersBoundariesOnly,
  showEventMarkers,
  eventMarkersAvailable,
  showAthLine,
  athLineAvailable,
  showMigrationLine,
  trimEmptyBars,
  showFlowLines,
  rangeSelectMode,
  crosshair,
  formatFlow,
  isMigrated,
  isMayhemMode,
  isCashbackEnabled,
  onGroupModeChange,
  onIntervalChange,
  onStyleChange,
  onMetricChange,
  onShowTradeMarkersChange,
  onShowWalletMarkersChange,
  onShowDevMarkersChange,
  onDevMarkersBoundariesOnlyChange,
  onShowEventMarkersChange,
  onShowAthLineChange,
  onShowMigrationLineChange,
  onTrimEmptyBarsChange,
  onShowFlowLinesChange,
  onRangeSelectModeChange,
}: ChartToolbarProps) {
  const [showMore, setShowMore] = useState(false);
  const intervalsDisabled = groupMode === 'slot';
  // Memoize so crosshair-move re-renders don't rebuild the formatters per pixel.
  const formatChartPrice = useMemo(() => createChartPriceFormatter(priceUnit), [priceUnit]);
  const formatVol = useMemo(() => createChartPriceFormatter('SOL'), []);

  const crosshairLine = crosshair ? (
    <>
      <BarCrosshairFields
        style={style}
        crosshair={crosshair}
        formatPrice={formatChartPrice}
        formatVol={formatVol}
        layout="inline"
      />
      {(crosshair.flowVol != null || crosshair.flowNonVol != null) && (
        <div>
          <span style={{ color: FLOW_VOL_LINE_COLOR }}>
            <span className="font-semibold">VolMk</span>{' '}
            {crosshair.flowVol != null ? formatFlow(crosshair.flowVol) : '—'}
          </span>{' '}
          <span style={{ color: FLOW_NON_VOL_LINE_COLOR }}>
            <span className="font-semibold">NonVol</span>{' '}
            {crosshair.flowNonVol != null ? formatFlow(crosshair.flowNonVol) : '—'}
          </span>
        </div>
      )}
    </>
  ) : null;

  const showStatusBadges = isMigrated != null;

  // Surface "More" as active when any advanced toggle is on.
  const moreActive =
    showMore ||
    trimEmptyBars ||
    showAthLine ||
    showMigrationLine ||
    showFlowLines ||
    rangeSelectMode ||
    (showDevMarkers && devMarkersBoundariesOnly);

  return (
    <div
      className="flex items-start gap-3 border-b px-3 py-2"
      style={{ borderColor: CHART_COLORS.border }}
    >
      {/* Left: title, status badges, crosshair OHLCV */}
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
              {isMayhemMode && <StatusBadge label="Mayhem" color={STATUS_BADGE_COLOR.mayhem} />}
              {isCashbackEnabled && (
                <StatusBadge label="Cashback" color={STATUS_BADGE_COLOR.cashback} />
              )}
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
              <div>—</div>
            </div>
          )}
        </div>
      </div>

      {/* Right: essentials always; overlays behind More */}
      <div className="flex shrink-0 flex-col items-end gap-1.5">
        <div className="flex flex-wrap items-center gap-2">
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
            className={cn(
              'flex rounded-md p-0.5',
              intervalsDisabled && 'opacity-40',
            )}
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
            active={showWalletMarkers}
            onClick={() => onShowWalletMarkersChange(!showWalletMarkers)}
            label="Toggle tracked-wallet markers"
            tooltip="Tracked-wallet buy/sell markers"
          >
            <WalletMarkersIcon />
          </IconToggleButton>

          <IconToggleButton
            active={showDevMarkers}
            onClick={() => onShowDevMarkersChange(!showDevMarkers)}
            disabled={!devMarkersAvailable}
            label="Toggle dev/creator markers"
            tooltip={
              devMarkersAvailable
                ? 'Dev/creator wallet markers — triangle silhouette with heavier first_buy (entry) and sell_all (full exit) markers'
                : 'No creator wallet known for this token'
            }
            activeColor={CHART_COLORS.dev}
          >
            <DevMarkersIcon />
          </IconToggleButton>

          {eventMarkersAvailable && (
            <IconToggleButton
              active={showEventMarkers}
              onClick={() => onShowEventMarkersChange(!showEventMarkers)}
              label="Toggle strategy entry/exit price lines"
              tooltip="Dashed entry/exit fill-price lines"
              activeColor={CHART_COLORS.entry}
            >
              <EntryExitLinesIcon />
            </IconToggleButton>
          )}

          <IconToggleButton
            active={trimEmptyBars}
            onClick={() => onTrimEmptyBarsChange(!trimEmptyBars)}
            label="Toggle trimming of empty candles"
            tooltip="Hide flat candles for intervals with no trades"
          >
            <TrimGapsIcon />
          </IconToggleButton>

          <IconToggleButton
            active={showFlowLines}
            onClick={() => onShowFlowLinesChange(!showFlowLines)}
            label="Toggle vol/non-vol flow lines"
            tooltip="Cumulative volume-maker (red) vs non-volume (gold) overlay — uses fingerprint volume_ix_patterns when available, else creator-seeded classification"
            activeColor={FLOW_VOL_LINE_COLOR}
          >
            <FlowLinesIcon />
          </IconToggleButton>

          <button
            type="button"
            onClick={() => setShowMore((v) => !v)}
            aria-expanded={showMore}
            className={cn(
              'rounded-md px-2 py-1 text-[11px] font-semibold transition-colors',
              moreActive ? 'text-[#0a0a0a]' : 'hover:text-white',
            )}
            style={
              moreActive
                ? { backgroundColor: CHART_COLORS.activePill }
                : { backgroundColor: CHART_COLORS.grid, color: CHART_COLORS.panelTextDim }
            }
          >
            More
          </button>
        </div>

        {showMore && (
          <div className="flex flex-wrap items-center justify-end gap-2">
            <label
              className={cn(
                'flex cursor-pointer items-center gap-1.5 rounded-md px-2 py-1 text-[11px] font-semibold',
                !athLineAvailable && 'cursor-not-allowed opacity-40',
              )}
              style={{ backgroundColor: CHART_COLORS.grid, color: CHART_COLORS.panelTextDim }}
              title={
                athLineAvailable
                  ? 'Show all-time high price line'
                  : 'No ATH price recorded for this token'
              }
            >
              <Checkbox
                boxSize="sm"
                checked={showAthLine}
                disabled={!athLineAvailable}
                onChange={(e) => onShowAthLineChange(e.target.checked)}
                className="accent-[#f0b429]"
              />
              <span style={showAthLine && athLineAvailable ? { color: CHART_COLORS.athLine } : undefined}>
                ATH
              </span>
            </label>

            <label
              className="flex cursor-pointer items-center gap-1.5 rounded-md px-2 py-1 text-[11px] font-semibold"
              style={{ backgroundColor: CHART_COLORS.grid, color: CHART_COLORS.panelTextDim }}
              title="Show pump.fun bonding-curve graduation price"
            >
              <Checkbox
                boxSize="sm"
                checked={showMigrationLine}
                onChange={(e) => onShowMigrationLineChange(e.target.checked)}
                className="accent-[#5dade2]"
              />
              <span style={showMigrationLine ? { color: CHART_COLORS.migrationLine } : undefined}>
                Migration
              </span>
            </label>

            <label
              className={cn(
                'flex cursor-pointer items-center gap-1.5 rounded-md px-2 py-1 text-[11px] font-semibold',
                (!showDevMarkers || !devMarkersAvailable) && 'cursor-not-allowed opacity-40',
              )}
              style={{ backgroundColor: CHART_COLORS.grid, color: CHART_COLORS.panelTextDim }}
              title={
                devMarkersAvailable
                  ? 'Show only the dev first_buy (entry) and sell_all (full exit) markers — hides the dev’s mid-position buys/sells. Enable the Dev toggle first.'
                  : 'No creator wallet known for this token'
              }
            >
              <Checkbox
                boxSize="sm"
                checked={devMarkersBoundariesOnly}
                disabled={!showDevMarkers || !devMarkersAvailable}
                onChange={(e) => onDevMarkersBoundariesOnlyChange(e.target.checked)}
              />
              <span
                style={
                  devMarkersBoundariesOnly && showDevMarkers ? { color: CHART_COLORS.dev } : undefined
                }
              >
                Dev boundaries only
              </span>
            </label>

            <IconToggleButton
              active={rangeSelectMode}
              onClick={() => onRangeSelectModeChange(!rangeSelectMode)}
              label="Toggle range-select mode"
              tooltip="Drag to select a time range; hover its label for totals"
            >
              <RangeSelectIcon />
            </IconToggleButton>
          </div>
        )}
      </div>
    </div>
  );
}
