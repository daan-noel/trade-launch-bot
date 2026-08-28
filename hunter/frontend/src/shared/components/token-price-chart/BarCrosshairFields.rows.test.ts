import { describe, expect, it } from 'vitest';
import { isValidElement, type ReactElement } from 'react';
import { BarCrosshairFields, crosshairInlineRows } from './BarCrosshairFields';
import type { ChartCrosshairInfo, ChartStyle } from './types';

const CROSSHAIR: ChartCrosshairInfo = {
  open: 1,
  high: 2,
  low: 0.5,
  close: 1.5,
  volume: 10,
  inflow: 6,
  outflow: 4,
  liquiditySol: 20,
  flowTagged: null,
  flowUntagged: null,
};

/** Count the top-level `<div>` rows the inline layout actually renders. */
function renderedInlineRows(style: ChartStyle): number {
  const tree = BarCrosshairFields({
    style,
    crosshair: CROSSHAIR,
    formatPrice: String,
    formatVol: String,
    layout: 'inline',
  }) as ReactElement<{ children?: unknown }>;
  const children = tree.props.children;
  const list = Array.isArray(children) ? children : [children];
  return list.filter((c) => isValidElement(c) && c.type === 'div').length;
}

describe('crosshairInlineRows', () => {
  // The toolbar pins its crosshair-readout block to
  // `crosshairInlineRows(style) + flowRow` rows so hovering can never resize it.
  // If this drifts, the block resizes on hover, the chart canvas moves out from
  // under the pointer, and the tooltip vibrates on/off (Console manual-trade panel).
  it.each<ChartStyle>(['candles', 'line'])('matches the rows rendered for %s', (style) => {
    expect(crosshairInlineRows(style)).toBe(renderedInlineRows(style));
  });
});
