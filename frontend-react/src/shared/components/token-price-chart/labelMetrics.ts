/** Cached offscreen 2D context for measuring chip text width in CSS px. */
let measureCtx: CanvasRenderingContext2D | null | undefined;

/**
 * Width (CSS px) of `text` rendered in `font`, using a cached offscreen canvas
 * context. Falls back to a rough per-character estimate when no 2D context is
 * available. Shared by the chart label-chip plugins.
 */
export function measureLabelWidth(text: string, font: string): number {
  if (measureCtx === undefined) {
    measureCtx = document.createElement('canvas').getContext('2d');
  }
  if (!measureCtx) return text.length * 6; // rough fallback if no 2D context
  measureCtx.font = font;
  return measureCtx.measureText(text).width;
}
