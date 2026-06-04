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
import { Button } from '../ui/Button';
import { Checkbox } from '../ui/Checkbox';
import { cn } from './cn';
import type { ChartMetric, ChartToolbarProps } from './types';

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

/** Zigzag path through swing pivots; line omitted when legs are not connected. */
function ConnectSwingsIcon({ connected }: { connected: boolean }) {
  const nodes = [
    { cx: 4, cy: 14 },
    { cx: 10, cy: 5 },
    { cx: 16, cy: 11 },
  ];
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden className="size-3.5">
      {connected ? (
        <path
          d="M4 14 10 5 16 11"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      ) : null}
      {nodes.map(({ cx, cy }) => (
        <circle key={`${cx}-${cy}`} cx={cx} cy={cy} r="2" fill="currentColor" />
      ))}
    </svg>
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
  showAthLine,
  athLineAvailable,
  showMigrationLine,
  swingOverlayAvailable,
  showSwingOverlay,
  connectSwings,
  crosshair,
  isMigrated,
  isMayhemMode,
  isCashbackEnabled,
  onGroupModeChange,
  onIntervalChange,
  onStyleChange,
  onMetricChange,
  onShowTradeMarkersChange,
  onShowAthLineChange,
  onShowMigrationLineChange,
  onShowSwingOverlayChange,
  onConnectSwingsChange,
}: ChartToolbarProps) {
  const intervalsDisabled = groupMode === 'slot';
  const formatChartPrice = createChartPriceFormatter(priceUnit);
  const formatVol = createChartPriceFormatter('SOL');

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
      className="flex flex-wrap items-center justify-between gap-2 border-b px-3 py-2"
      style={{ borderColor: CHART_COLORS.border }}
    >
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
          className="mt-0.5 h-[14px] overflow-hidden font-mono text-[11px] leading-[14px]"
          aria-live="polite"
        >
          {crosshairLine ?? <span className="invisible select-none" aria-hidden="true">—</span>}
        </div>
      </div>

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
            <button
              key={key}
              type="button"
              onClick={() => onStyleChange(key)}
              className={cn(
                'rounded px-2 py-0.5 text-[11px] font-semibold transition-colors',
                style === key ? 'text-[#0a0a0a]' : 'hover:text-white',
              )}
              style={
                style === key
                  ? { backgroundColor: CHART_COLORS.activePill }
                  : { color: CHART_COLORS.panelTextDim }
              }
            >
              {CHART_STYLE_LABELS[key]}
            </button>
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

        <button
          type="button"
          onClick={() => onShowTradeMarkersChange(!showTradeMarkers)}
          title="Show buy/sell counts per bar"
          className={cn(
            'rounded-md px-2 py-1 text-[11px] font-semibold transition-colors',
            showTradeMarkers ? 'text-[#0a0a0a]' : 'hover:text-white',
          )}
          style={
            showTradeMarkers
              ? { backgroundColor: CHART_COLORS.activePill }
              : { backgroundColor: CHART_COLORS.grid, color: CHART_COLORS.panelTextDim }
          }
        >
          B/S counts
        </button>

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

        <div
          className={cn(
            'flex items-center gap-0.5 rounded-md py-1 pl-2 pr-1 text-[11px] font-semibold',
            !swingOverlayAvailable && 'cursor-not-allowed opacity-40',
          )}
          style={{ backgroundColor: CHART_COLORS.grid, color: CHART_COLORS.panelTextDim }}
        >
          <label
            className={cn(
              'flex cursor-pointer items-center gap-1.5',
              !swingOverlayAvailable && 'cursor-not-allowed',
            )}
            title={
              swingOverlayAvailable
                ? 'Show swing detection path on chart'
                : 'Run swing detection to overlay results'
            }
          >
            <Checkbox
              boxSize="sm"
              checked={showSwingOverlay}
              disabled={!swingOverlayAvailable}
              onChange={(e) => onShowSwingOverlayChange(e.target.checked)}
              className="accent-[#e879f9]"
            />
            <span
              style={
                showSwingOverlay && swingOverlayAvailable
                  ? { color: CHART_COLORS.swingOverlay }
                  : undefined
              }
            >
              Swings
            </span>
          </label>
          <Button
            variant="subtle"
            size="xs"
            active={connectSwings}
            disabled={!swingOverlayAvailable || !showSwingOverlay}
            className="!min-h-0 shrink-0 border-0 bg-transparent px-1.5 py-0.5 normal-case tracking-normal hover:bg-white/6"
            title={
              swingOverlayAvailable
                ? connectSwings
                  ? 'Disconnect swing legs on chart'
                  : 'Connect sequential swing legs on chart'
                : 'Run swing detection to connect swings'
            }
            aria-label={
              swingOverlayAvailable
                ? connectSwings
                  ? 'Disconnect swing legs on chart'
                  : 'Connect sequential swing legs on chart'
                : 'Run swing detection to connect swings'
            }
            onClick={() => onConnectSwingsChange(!connectSwings)}
          >
            <ConnectSwingsIcon connected={connectSwings} />
          </Button>
        </div>
      </div>
    </div>
  );
}
