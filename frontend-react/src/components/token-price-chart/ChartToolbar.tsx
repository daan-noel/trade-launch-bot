import { useMemo } from 'react';
import {
  CHART_COLORS,
  CHART_GROUP_MODES,
  CHART_GROUP_MODE_LABELS,
  CHART_INTERVAL_LABELS,
  CHART_STYLES,
  CHART_STYLE_LABELS,
  createChartPriceFormatter,
} from './constants';
import { getTimezoneSelectOptions } from './chartTimezone';
import { BarCrosshairFields } from './BarCrosshairFields';
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
  chartTimezone,
  onChartTimezoneChange,
}: ChartToolbarProps) {
  const intervalsDisabled = groupMode === 'slot';
  const timezoneOptions = useMemo(
    () => getTimezoneSelectOptions(chartTimezone),
    [chartTimezone],
  );
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

        <select
          value={chartTimezone}
          disabled={intervalsDisabled}
          onChange={(e) => onChartTimezoneChange(e.target.value)}
          title="Chart time axis timezone"
          className={cn(
            'max-w-[14rem] truncate rounded-md px-2 py-1 text-[11px] font-semibold',
            intervalsDisabled && 'cursor-not-allowed opacity-40',
          )}
          style={{
            backgroundColor: CHART_COLORS.grid,
            color: CHART_COLORS.panelTextDim,
            border: 'none',
          }}
        >
          {timezoneOptions.map((opt) => (
            <option key={opt.id} value={opt.id}>
              {opt.label}
            </option>
          ))}
        </select>

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
          <input
            type="checkbox"
            checked={showAthLine}
            disabled={!athLineAvailable}
            onChange={(e) => onShowAthLineChange(e.target.checked)}
            className="size-3 accent-[#f0b429]"
          />
          <span style={showAthLine && athLineAvailable ? { color: CHART_COLORS.athLine } : undefined}>
            ATH line
          </span>
        </label>

        <label
          className="flex cursor-pointer items-center gap-1.5 rounded-md px-2 py-1 text-[11px] font-semibold"
          style={{ backgroundColor: CHART_COLORS.grid, color: CHART_COLORS.panelTextDim }}
          title="Show pump.fun bonding-curve graduation price"
        >
          <input
            type="checkbox"
            checked={showMigrationLine}
            onChange={(e) => onShowMigrationLineChange(e.target.checked)}
            className="size-3 accent-[#5dade2]"
          />
          <span style={showMigrationLine ? { color: CHART_COLORS.migrationLine } : undefined}>
            Migration line
          </span>
        </label>

        <label
          className={cn(
            'flex cursor-pointer items-center gap-1.5 rounded-md px-2 py-1 text-[11px] font-semibold',
            !swingOverlayAvailable && 'cursor-not-allowed opacity-40',
          )}
          style={{ backgroundColor: CHART_COLORS.grid, color: CHART_COLORS.panelTextDim }}
          title={
            swingOverlayAvailable
              ? 'Show swing detection path on chart'
              : 'Run swing detection to overlay results'
          }
        >
          <input
            type="checkbox"
            checked={showSwingOverlay}
            disabled={!swingOverlayAvailable}
            onChange={(e) => onShowSwingOverlayChange(e.target.checked)}
            className="size-3 accent-[#e879f9]"
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
      </div>
    </div>
  );
}
