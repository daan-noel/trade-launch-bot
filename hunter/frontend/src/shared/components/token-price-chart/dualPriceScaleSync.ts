import type { IChartApi } from 'lightweight-charts';

type PriceRange = { from: number; to: number };

function rangeEq(a: PriceRange, b: PriceRange): boolean {
  return Math.abs(a.from - b.from) < 1e-12 && Math.abs(a.to - b.to) < 1e-12;
}

function proportionalRange(srcPrev: PriceRange, srcNext: PriceRange, dstPrev: PriceRange): PriceRange {
  const srcSpan = srcPrev.to - srcPrev.from;
  if (!(srcSpan > 0) || !Number.isFinite(srcSpan)) return dstPrev;
  const nextSpan = srcNext.to - srcNext.from;
  if (!(nextSpan > 0) || !Number.isFinite(nextSpan)) return dstPrev;
  const spanRatio = nextSpan / srcSpan;
  const srcCenter = (srcPrev.from + srcPrev.to) / 2;
  const nextCenter = (srcNext.from + srcNext.to) / 2;
  const shiftNorm = (nextCenter - srcCenter) / srcSpan;
  const dstSpan = dstPrev.to - dstPrev.from;
  const dstCenter = (dstPrev.from + dstPrev.to) / 2;
  const newSpan = dstSpan * spanRatio;
  const newCenter = dstCenter + shiftNorm * dstSpan;
  return { from: newCenter - newSpan / 2, to: newCenter + newSpan / 2 };
}

function readPriceRange(chart: IChartApi, id: 'left' | 'right'): PriceRange | null {
  const range = chart.priceScale(id).getVisibleRange();
  if (range == null) return null;
  if (!Number.isFinite(range.from) || !Number.isFinite(range.to) || range.to <= range.from) {
    return null;
  }
  return { from: range.from, to: range.to };
}

export function rearmDualAutoScale(chart: IChartApi) {
  const left = chart.priceScale('left');
  const right = chart.priceScale('right');
  if (typeof left.setAutoScale === 'function') {
    left.setAutoScale(true);
    right.setAutoScale(true);
  } else {
    left.applyOptions({ autoScale: true });
    right.applyOptions({ autoScale: true });
  }
}

export type DualPriceScaleSync = {
  detach: () => void;
  /**
   * Re-arm autoScale on both scales UNLESS the user has taken manual control of
   * the Y axis by dragging a price axis. Every caller that re-fits the vertical
   * scale as a side effect of a data/overlay update must go through this, never
   * {@link rearmDualAutoScale} directly.
   */
  rearm: () => void;
  /** Hard reset — drops manual Y control and re-fits. For deliberate axis-meaning
   *  changes (flow overlay toggled, unit/basis switch), not for data updates. */
  reset: () => void;
};

/**
 * Keep left (flow) and right (price) Y-zoom in lockstep. Returns a handle.
 * Time pan/zoom refits both via autoScale; axis drag on one side mirrors the
 * relative zoom onto the other.
 *
 * Manual Y control is sticky, matching lightweight-charts' own semantics: once
 * the user drags a price axis, autoScale stays off until they double-click that
 * axis. Without this the chart re-armed autoScale on EVERY visible-range change
 * — and a live trade arriving means setData + a viewport restore, i.e. two range
 * changes — so a hand-set price zoom was wiped on the next trade.
 */
export function attachDualPriceScaleSync(
  chart: IChartApi,
  el: HTMLElement,
  opts?: { isPaused?: () => boolean },
): DualPriceScaleSync {
  let lastLeftRange: PriceRange | null = null;
  let lastRightRange: PriceRange | null = null;
  let syncingScales = false;
  let syncRaf = 0;
  /** The pointer went down on a price axis — this gesture scales Y, not time. */
  let axisDragging = false;
  /** The user has hand-set the Y zoom; hold it until an axis double-click. */
  let manualPriceZoom = false;

  /** Hit-test against the axis gutters. The gesture's ORIGIN is the only reliable
   *  signal of intent: inferring it from "only one scale's range moved" misfires
   *  whenever a body pan leaves the flow line flat, which would silently freeze
   *  the Y axis on an ordinary horizontal drag. */
  const isOverPriceAxis = (clientX: number): boolean => {
    const rect = el.getBoundingClientRect();
    if (!(rect.width > 0)) return false;
    const x = clientX - rect.left;
    const leftWidth = chart.priceScale('left').width();
    const rightWidth = chart.priceScale('right').width();
    return (leftWidth > 0 && x <= leftWidth) || (rightWidth > 0 && x >= rect.width - rightWidth);
  };

  const capturePriceRanges = () => {
    lastLeftRange = readPriceRange(chart, 'left');
    lastRightRange = readPriceRange(chart, 'right');
  };

  const syncPriceScales = () => {
    if (syncingScales || opts?.isPaused?.()) return;
    const left = readPriceRange(chart, 'left');
    const right = readPriceRange(chart, 'right');
    if (!left || !right) {
      capturePriceRanges();
      return;
    }
    if (!lastLeftRange || !lastRightRange) {
      lastLeftRange = left;
      lastRightRange = right;
      return;
    }
    const leftChanged = !rangeEq(left, lastLeftRange);
    const rightChanged = !rangeEq(right, lastRightRange);
    if (rightChanged === leftChanged) {
      lastLeftRange = left;
      lastRightRange = right;
      return;
    }
    syncingScales = true;
    try {
      if (rightChanged) {
        const next = proportionalRange(lastRightRange, right, lastLeftRange);
        chart.priceScale('left').setVisibleRange(next);
        lastLeftRange = next;
        lastRightRange = right;
      } else {
        const next = proportionalRange(lastLeftRange, left, lastRightRange);
        chart.priceScale('right').setVisibleRange(next);
        lastRightRange = next;
        lastLeftRange = left;
      }
    } finally {
      syncingScales = false;
    }
  };

  const scheduleScaleSync = () => {
    if (syncRaf) return;
    syncRaf = requestAnimationFrame(() => {
      syncRaf = 0;
      syncPriceScales();
    });
  };

  // Fires for a user pan/zoom AND for every programmatic range change — setData
  // and the post-setData viewport restore both land here — so it must not clobber
  // a hand-set Y zoom.
  const onTimeRangeChange = () => {
    if (!manualPriceZoom) rearmDualAutoScale(chart);
    requestAnimationFrame(capturePriceRanges);
  };
  chart.timeScale().subscribeVisibleLogicalRangeChange(onTimeRangeChange);

  const onWheelScale = () => scheduleScaleSync();
  const onPointerDownScale = (e: PointerEvent) => {
    axisDragging = e.button === 0 && isOverPriceAxis(e.clientX);
  };
  const onPointerUpScale = () => {
    axisDragging = false;
    scheduleScaleSync();
  };
  const onPointerMoveScale = (e: PointerEvent) => {
    if (e.buttons !== 1) return;
    if (axisDragging) manualPriceZoom = true;
    scheduleScaleSync();
  };
  // Double-click on a price axis is lightweight-charts' own "reset Y" gesture, so
  // it always resets. On the chart BODY it only refits when the user has not taken
  // manual control — otherwise resetting the time zoom would silently drop the Y
  // zoom too.
  const onDoubleClickReset = (e: MouseEvent) => {
    const overAxis = isOverPriceAxis(e.clientX);
    requestAnimationFrame(() => {
      if (overAxis || !manualPriceZoom) {
        manualPriceZoom = false;
        rearmDualAutoScale(chart);
      }
      capturePriceRanges();
    });
  };

  el.addEventListener('wheel', onWheelScale, { passive: true });
  el.addEventListener('pointerdown', onPointerDownScale);
  el.addEventListener('pointerup', onPointerUpScale);
  el.addEventListener('pointermove', onPointerMoveScale);
  el.addEventListener('dblclick', onDoubleClickReset);
  requestAnimationFrame(capturePriceRanges);

  return {
    detach: () => {
      chart.timeScale().unsubscribeVisibleLogicalRangeChange(onTimeRangeChange);
      if (syncRaf) cancelAnimationFrame(syncRaf);
      el.removeEventListener('wheel', onWheelScale);
      el.removeEventListener('pointerdown', onPointerDownScale);
      el.removeEventListener('pointerup', onPointerUpScale);
      el.removeEventListener('pointermove', onPointerMoveScale);
      el.removeEventListener('dblclick', onDoubleClickReset);
    },
    rearm: () => {
      if (manualPriceZoom) return;
      rearmDualAutoScale(chart);
      requestAnimationFrame(capturePriceRanges);
    },
    reset: () => {
      manualPriceZoom = false;
      rearmDualAutoScale(chart);
      requestAnimationFrame(capturePriceRanges);
    },
  };
}
