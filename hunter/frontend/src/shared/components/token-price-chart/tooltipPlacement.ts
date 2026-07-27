/** Gap between the crosshair point and the tooltip box. */
const CURSOR_GAP = 14;
/** Keep the box off the panel's border. */
const EDGE_PAD = 4;

/**
 * Horizontal placement for a crosshair tooltip: beside the cursor, flipped to the
 * cursor's LEFT when the box would otherwise run past the chart panel's right edge.
 *
 * A spill is not merely cosmetic. The box is `absolute`, so overflowing the panel
 * grows the page's scrollable width and summons a horizontal scrollbar that then
 * appears and disappears as the pointer moves. It bites on any narrow panel — the
 * Console's 380px manual-trade column is narrower than two tooltip widths.
 *
 * The flipped case anchors by `right` so the box hugs the cursor regardless of how
 * much of `maxWidth` it actually uses. When neither side has room (panel narrower
 * than the box) it clamps flush to the right edge.
 *
 * `containerWidth` unknown (0/undefined) ⇒ plain right-side placement, as before.
 */
export function tooltipHorizontalStyle(
  pointX: number,
  maxWidth: number,
  containerWidth?: number,
): { left: number } | { right: number } {
  if (!containerWidth || containerWidth <= 0) return { left: pointX + CURSOR_GAP };

  const rightOfCursor = pointX + CURSOR_GAP;
  if (rightOfCursor + maxWidth <= containerWidth - EDGE_PAD) return { left: rightOfCursor };

  if (pointX - CURSOR_GAP - maxWidth >= EDGE_PAD) {
    return { right: containerWidth - (pointX - CURSOR_GAP) };
  }

  return { left: Math.max(EDGE_PAD, containerWidth - EDGE_PAD - maxWidth) };
}
