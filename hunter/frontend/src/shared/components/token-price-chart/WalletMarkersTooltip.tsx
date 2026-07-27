import { CHART_COLORS } from './constants';
import { tooltipHorizontalStyle } from './tooltipPlacement';
import type { ChartWalletMarkersTooltipState } from './types';

/** Box width cap — also what the edge flip is measured against (see the twin
 *  constant in {@link ./BarCrosshairTooltip}). */
const MAX_W = 260;

function shortAddr(address: string) {
  return `${address.slice(0, 5)}…${address.slice(-4)}`;
}

function formatSol(sol: number) {
  if (sol === 0) return '—';
  if (sol < 0.001) return `${sol.toExponential(2)} SOL`;
  return `${sol.toFixed(3)} SOL`;
}

export function WalletMarkersTooltip({
  tooltip,
  containerWidth,
}: {
  tooltip: ChartWalletMarkersTooltipState;
  /** Width of the positioned chart panel; omit when unknown (no edge flip). */
  containerWidth?: number;
}) {
  const { point, wallets } = tooltip;

  return (
    <div
      className="pointer-events-none absolute z-30 min-w-[180px] rounded-md border px-2.5 py-2 font-mono text-[10px] leading-snug shadow-xl"
      style={{
        ...tooltipHorizontalStyle(point.x, MAX_W, containerWidth),
        maxWidth: MAX_W,
        top: point.y - 10,
        borderColor: `${CHART_COLORS.crosshair}`,
        backgroundColor: '#0d0d0df5',
        color: CHART_COLORS.panelText,
      }}
    >
      <div
        className="mb-1.5 text-[9px] font-bold uppercase tracking-wide"
        style={{ color: CHART_COLORS.panelTextDim }}
      >
        Tracked Wallets
      </div>
      <div className="flex flex-col gap-1.5">
        {wallets.map(({ wallet, buyCount, sellCount, buySol, sellSol }) => (
          <div key={wallet.address} className="flex flex-col gap-0.5">
            <div className="flex items-center gap-1.5">
              <span
                className="inline-block h-2 w-2 shrink-0 rounded-full"
                style={{ backgroundColor: wallet.color }}
              />
              <span className="font-semibold" style={{ color: wallet.color }}>
                {wallet.profileName ?? wallet.label ?? shortAddr(wallet.address)}
              </span>
            </div>
            {wallet.profileName && (
              <div className="ml-3.5" style={{ color: CHART_COLORS.panelTextDim }}>
                {wallet.label ?? shortAddr(wallet.address)}
              </div>
            )}
            {wallet.tags && wallet.tags.length > 0 && (
              <div className="ml-3.5 flex flex-wrap gap-1">
                {wallet.tags.map((tag) => (
                  <span
                    key={tag.name}
                    className="rounded px-1 py-px text-[8px] font-semibold uppercase tracking-wide"
                    style={{
                      color: tag.color,
                      backgroundColor: `${tag.color}22`,
                      border: `1px solid ${tag.color}55`,
                    }}
                  >
                    {tag.name}
                  </span>
                ))}
              </div>
            )}
            <div className="ml-3.5 flex flex-col gap-0.5">
              {buyCount > 0 && (
                <div className="flex items-center gap-1">
                  <span style={{ color: CHART_COLORS.buy }}>▲ Buy</span>
                  <span className="text-text-dim ml-1">
                    {formatSol(buySol)}
                    {buyCount > 1 && (
                      <span style={{ color: CHART_COLORS.panelTextDim }}> ×{buyCount}</span>
                    )}
                  </span>
                </div>
              )}
              {sellCount > 0 && (
                <div className="flex items-center gap-1">
                  <span style={{ color: CHART_COLORS.sell }}>▼ Sell</span>
                  <span className="text-text-dim ml-1">
                    {formatSol(sellSol)}
                    {sellCount > 1 && (
                      <span style={{ color: CHART_COLORS.panelTextDim }}> ×{sellCount}</span>
                    )}
                  </span>
                </div>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
