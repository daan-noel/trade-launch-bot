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

/**
 * Keep left (flow) and right (price) Y-zoom in lockstep. Returns a disposer.
 * Time pan/zoom refits both via autoScale; axis drag on one side mirrors the
 * relative zoom onto the other.
 */
export function attachDualPriceScaleSync(
  chart: IChartApi,
  el: HTMLElement,
  opts?: { isPaused?: () => boolean },
): () => void {
  let lastLeftRange: PriceRange | null = null;
  let lastRightRange: PriceRange | null = null;
  let syncingScales = false;
  let syncRaf = 0;

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

  const onTimeRangeChange = () => {
    rearmDualAutoScale(chart);
    requestAnimationFrame(capturePriceRanges);
  };
  chart.timeScale().subscribeVisibleLogicalRangeChange(onTimeRangeChange);

  const onWheelScale = () => scheduleScaleSync();
  const onPointerUpScale = () => scheduleScaleSync();
  const onPointerMoveScale = (e: PointerEvent) => {
    if (e.buttons !== 1) return;
    scheduleScaleSync();
  };
  const onDoubleClickReset = () => {
    requestAnimationFrame(() => {
      rearmDualAutoScale(chart);
      capturePriceRanges();
    });
  };

  el.addEventListener('wheel', onWheelScale, { passive: true });
  el.addEventListener('pointerup', onPointerUpScale);
  el.addEventListener('pointermove', onPointerMoveScale);
  el.addEventListener('dblclick', onDoubleClickReset);
  requestAnimationFrame(capturePriceRanges);

  return () => {
    chart.timeScale().unsubscribeVisibleLogicalRangeChange(onTimeRangeChange);
    if (syncRaf) cancelAnimationFrame(syncRaf);
    el.removeEventListener('wheel', onWheelScale);
    el.removeEventListener('pointerup', onPointerUpScale);
    el.removeEventListener('pointermove', onPointerMoveScale);
    el.removeEventListener('dblclick', onDoubleClickReset);
  };
}
