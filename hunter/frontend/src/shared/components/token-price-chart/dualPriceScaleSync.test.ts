import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { IChartApi } from 'lightweight-charts';
import { attachDualPriceScaleSync } from './dualPriceScaleSync';

/**
 * The regression this locks: a live trade means `setData` + a viewport restore, and BOTH
 * fire `subscribeVisibleLogicalRangeChange`. Re-arming autoScale from that callback threw
 * away a hand-set price zoom on every incoming trade. A drag that STARTED on a price axis
 * must make the Y zoom sticky until the axis is double-clicked; a body pan must not.
 */

const PANEL_WIDTH = 600;
const AXIS_WIDTH = 50;
/** Inside the right-hand axis gutter. */
const AXIS_X = PANEL_WIDTH - 10;
/** Well inside the plot area. */
const BODY_X = 300;

type Listener = (e: unknown) => void;

function fakeElement() {
  const listeners = new Map<string, Set<Listener>>();
  const el = {
    addEventListener(type: string, fn: Listener) {
      const set = listeners.get(type) ?? new Set<Listener>();
      set.add(fn);
      listeners.set(type, set);
    },
    removeEventListener(type: string, fn: Listener) {
      listeners.get(type)?.delete(fn);
    },
    getBoundingClientRect: () => ({ left: 0, top: 0, width: PANEL_WIDTH, height: 300 }),
  };
  const fire = (type: string, e: Record<string, unknown>) => {
    for (const fn of listeners.get(type) ?? []) fn(e);
  };
  return { el: el as unknown as HTMLElement, fire };
}

function fakeChart() {
  const setAutoScale = vi.fn();
  let onLogicalRangeChange: (() => void) | null = null;
  const scale = {
    width: () => AXIS_WIDTH,
    // Static range: syncPriceScales sees no one-sided change and early-outs, so the
    // manual flag can only come from the gesture hit-test — which is the point.
    getVisibleRange: () => ({ from: 1, to: 2 }),
    setVisibleRange: vi.fn(),
    setAutoScale,
    applyOptions: vi.fn(),
  };
  const chart = {
    priceScale: () => scale,
    timeScale: () => ({
      subscribeVisibleLogicalRangeChange: (fn: () => void) => {
        onLogicalRangeChange = fn;
      },
      unsubscribeVisibleLogicalRangeChange: () => {
        onLogicalRangeChange = null;
      },
    }),
  };
  return {
    chart: chart as unknown as IChartApi,
    setAutoScale,
    /** Stand-in for `setData` / the post-setData viewport restore. */
    emitDataDrivenRangeChange: () => onLogicalRangeChange?.(),
  };
}

function dragPriceAxis(fire: ReturnType<typeof fakeElement>['fire'], clientX: number) {
  fire('pointerdown', { button: 0, clientX });
  fire('pointermove', { buttons: 1, clientX });
  fire('pointerup', { clientX });
}

describe('attachDualPriceScaleSync — manual Y zoom is sticky', () => {
  const realRaf = globalThis.requestAnimationFrame;
  const realCancelRaf = globalThis.cancelAnimationFrame;

  beforeEach(() => {
    globalThis.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      cb(0);
      return 1;
    }) as typeof globalThis.requestAnimationFrame;
    globalThis.cancelAnimationFrame = (() => {}) as typeof globalThis.cancelAnimationFrame;
  });

  afterEach(() => {
    globalThis.requestAnimationFrame = realRaf;
    globalThis.cancelAnimationFrame = realCancelRaf;
  });

  it('re-fits on a data-driven range change while the user has not touched the axis', () => {
    const { el } = fakeElement();
    const { chart, setAutoScale, emitDataDrivenRangeChange } = fakeChart();
    attachDualPriceScaleSync(chart, el);

    setAutoScale.mockClear();
    emitDataDrivenRangeChange();

    expect(setAutoScale).toHaveBeenCalledWith(true);
  });

  it('holds the Y zoom through data-driven range changes after a price-axis drag', () => {
    const { el, fire } = fakeElement();
    const { chart, setAutoScale, emitDataDrivenRangeChange } = fakeChart();
    attachDualPriceScaleSync(chart, el);

    dragPriceAxis(fire, AXIS_X);
    setAutoScale.mockClear();
    emitDataDrivenRangeChange();
    emitDataDrivenRangeChange();

    expect(setAutoScale).not.toHaveBeenCalled();
  });

  it('does not treat a drag that started in the plot area as manual Y control', () => {
    const { el, fire } = fakeElement();
    const { chart, setAutoScale, emitDataDrivenRangeChange } = fakeChart();
    attachDualPriceScaleSync(chart, el);

    dragPriceAxis(fire, BODY_X);
    setAutoScale.mockClear();
    emitDataDrivenRangeChange();

    expect(setAutoScale).toHaveBeenCalledWith(true);
  });

  it('releases manual control on an axis double-click, not a body double-click', () => {
    const { el, fire } = fakeElement();
    const { chart, setAutoScale, emitDataDrivenRangeChange } = fakeChart();
    attachDualPriceScaleSync(chart, el);

    dragPriceAxis(fire, AXIS_X);

    // Body double-click resets the TIME zoom — it must not drop the price zoom.
    setAutoScale.mockClear();
    fire('dblclick', { clientX: BODY_X });
    emitDataDrivenRangeChange();
    expect(setAutoScale).not.toHaveBeenCalled();

    // Axis double-click is the library's native "reset Y" gesture.
    fire('dblclick', { clientX: AXIS_X });
    expect(setAutoScale).toHaveBeenCalledWith(true);

    setAutoScale.mockClear();
    emitDataDrivenRangeChange();
    expect(setAutoScale).toHaveBeenCalledWith(true);
  });

  it('rearm() respects manual control; reset() overrides it', () => {
    const { el, fire } = fakeElement();
    const { chart, setAutoScale } = fakeChart();
    const sync = attachDualPriceScaleSync(chart, el);

    dragPriceAxis(fire, AXIS_X);

    setAutoScale.mockClear();
    sync.rearm();
    expect(setAutoScale).not.toHaveBeenCalled();

    sync.reset();
    expect(setAutoScale).toHaveBeenCalledWith(true);

    // reset() dropped manual control, so ordinary re-fits resume.
    setAutoScale.mockClear();
    sync.rearm();
    expect(setAutoScale).toHaveBeenCalledWith(true);
  });
});
