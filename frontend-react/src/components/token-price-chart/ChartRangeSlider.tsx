import { useCallback, useEffect, useRef } from 'react';
import { CHART_COLORS } from './constants';

type DragMode = 'from' | 'to' | 'pan';

type ChartRangeSliderProps = {
  /** Full data span (first bar time / slot). */
  min: number;
  /** Full data span (last bar time / slot). */
  max: number;
  /** Left edge of the currently visible window. */
  from: number;
  /** Right edge of the currently visible window. */
  to: number;
  /** Called continuously while dragging with the requested window. */
  onChange: (from: number, to: number) => void;
};

/** Handle width in px; centered on its edge so it slightly overhangs the window. */
const HANDLE_W = 10;
/** Never let the window collapse below this fraction of the full span. */
const MIN_WINDOW_RATIO = 0.01;

/**
 * Thin time-range slider drawn under the chart. The track maps to the full data
 * span; the highlighted region is the visible window. Drag a handle to zoom one
 * edge, drag the middle to pan. It drives the chart via `onChange` and is kept
 * in sync by the parent re-feeding `from`/`to` from the chart's visible range.
 */
export function ChartRangeSlider({ min, max, from, to, onChange }: ChartRangeSliderProps) {
  const trackRef = useRef<HTMLDivElement | null>(null);
  const dragRef = useRef<{
    mode: DragMode;
    startX: number;
    startFrom: number;
    startTo: number;
  } | null>(null);

  const span = max - min;
  const clampedFrom = Math.max(min, Math.min(from, max));
  const clampedTo = Math.max(min, Math.min(to, max));
  const leftPct = span > 0 ? ((clampedFrom - min) / span) * 100 : 0;
  const rightPct = span > 0 ? ((clampedTo - min) / span) * 100 : 100;
  const widthPct = Math.max(0, rightPct - leftPct);

  const valueFromClientX = useCallback(
    (clientX: number) => {
      const rect = trackRef.current?.getBoundingClientRect();
      if (!rect || rect.width === 0) return min;
      const ratio = (clientX - rect.left) / rect.width;
      return min + Math.max(0, Math.min(1, ratio)) * span;
    },
    [min, span],
  );

  useEffect(() => {
    const onMove = (e: PointerEvent) => {
      const drag = dragRef.current;
      if (!drag) return;
      const minWindow = span * MIN_WINDOW_RATIO;

      if (drag.mode === 'pan') {
        const rect = trackRef.current?.getBoundingClientRect();
        if (!rect || rect.width === 0) return;
        const deltaValue = ((e.clientX - drag.startX) / rect.width) * span;
        const windowSize = drag.startTo - drag.startFrom;
        const nextFrom = Math.max(
          min,
          Math.min(drag.startFrom + deltaValue, max - windowSize),
        );
        onChange(nextFrom, nextFrom + windowSize);
        return;
      }

      const value = valueFromClientX(e.clientX);
      if (drag.mode === 'from') {
        const nextFrom = Math.max(min, Math.min(value, drag.startTo - minWindow));
        onChange(nextFrom, drag.startTo);
      } else {
        const nextTo = Math.min(max, Math.max(value, drag.startFrom + minWindow));
        onChange(drag.startFrom, nextTo);
      }
    };
    const onUp = () => {
      dragRef.current = null;
    };
    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
    return () => {
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
    };
  }, [min, max, span, onChange, valueFromClientX]);

  const startDrag = (mode: DragMode) => (e: React.PointerEvent) => {
    e.preventDefault();
    e.stopPropagation();
    dragRef.current = {
      mode,
      startX: e.clientX,
      startFrom: clampedFrom,
      startTo: clampedTo,
    };
  };

  if (span <= 0) return null;

  return (
    <div className="select-none px-2 pb-2 pt-1">
      <div
        ref={trackRef}
        className="relative h-4 rounded"
        style={{ backgroundColor: CHART_COLORS.grid }}
      >
        <div
          className="absolute bottom-0 top-0 cursor-grab active:cursor-grabbing"
          style={{
            left: `${leftPct}%`,
            width: `${widthPct}%`,
            backgroundColor: `${CHART_COLORS.line}33`,
            border: `1px solid ${CHART_COLORS.line}`,
            borderRadius: 4,
          }}
          onPointerDown={startDrag('pan')}
        />
        <div
          className="absolute bottom-0 top-0 cursor-ew-resize rounded"
          style={{
            left: `calc(${leftPct}% - ${HANDLE_W / 2}px)`,
            width: HANDLE_W,
            backgroundColor: CHART_COLORS.line,
          }}
          onPointerDown={startDrag('from')}
        />
        <div
          className="absolute bottom-0 top-0 cursor-ew-resize rounded"
          style={{
            left: `calc(${rightPct}% - ${HANDLE_W / 2}px)`,
            width: HANDLE_W,
            backgroundColor: CHART_COLORS.line,
          }}
          onPointerDown={startDrag('to')}
        />
      </div>
    </div>
  );
}
