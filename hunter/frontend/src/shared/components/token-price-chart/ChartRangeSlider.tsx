import { useCallback, useEffect, useMemo, useRef } from 'react';
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
  /** Highlight-lane rows mirrored onto the track: one thin band per row, a tick at
   *  every bar key (same units as `min`/`max`) the row hits — so where each
   *  highlight sits in the whole token stays visible while the chart is zoomed in. */
  marks?: readonly SliderMarkRow[] | null;
};

export interface SliderMarkRow {
  color: string;
  times: readonly number[];
}

/** Track resolution for ticks: hits closer than 1/1000 of the span share a tick. */
const MARK_BUCKETS = 1000;
/** Track height with no highlight rows. */
const BASE_TRACK_H = 16;
/** Each highlight row's band on the track — the track grows to give every row this. */
const MARK_ROW_H = 7;

/** Track height for `rows` highlight rows: the base, or taller so each row keeps its band. */
function trackHeight(rows: number): number {
  return Math.max(BASE_TRACK_H, rows * MARK_ROW_H);
}

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
export function ChartRangeSlider({ min, max, from, to, onChange, marks = null }: ChartRangeSliderProps) {
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

  const markRects = useMemo(() => {
    if (!marks?.length || span <= 0) return null;
    const rowH = trackHeight(marks.length) / marks.length;
    return marks.flatMap((row, r) => {
      const buckets = new Set<number>();
      for (const t of row.times) {
        buckets.add(Math.round(((t - min) / span) * MARK_BUCKETS));
      }
      return [...buckets].map((b) => ({ key: `${r}:${b}`, x: b, y: r * rowH, h: rowH, color: row.color }));
    });
  }, [marks, min, span]);

  if (span <= 0) return null;
  const trackH = trackHeight(marks?.length ?? 0);

  return (
    <div className="select-none px-2 pb-2 pt-1">
      <div
        ref={trackRef}
        className="relative rounded"
        style={{ height: trackH, backgroundColor: CHART_COLORS.grid }}
      >
        {markRects && (
          <svg
            className="pointer-events-none absolute inset-0 size-full"
            viewBox={`0 0 ${MARK_BUCKETS} ${trackH}`}
            preserveAspectRatio="none"
            aria-hidden
          >
            {markRects.map((m) => (
              <rect key={m.key} x={m.x - 1.5} y={m.y} width={3} height={m.h} fill={m.color} />
            ))}
          </svg>
        )}
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
