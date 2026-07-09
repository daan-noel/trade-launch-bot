import { useEffect, useRef } from 'react';

/**
 * Run `callback` on an `intervalMs` timer, but only while the tab is visible.
 *
 * When the tab is hidden the timer is suspended so background tabs stop hitting
 * the API; on re-show it fires once immediately (to catch up) and resumes. Pass
 * `enabled = false` to stop polling entirely — e.g. nothing is selected, or the
 * selected entity is settled and can no longer change.
 *
 * The `leading` flag controls whether `callback` runs once on mount/enable. Set
 * it false when the caller does its own (non-silent) initial load and only wants
 * this hook for the recurring silent refresh.
 *
 * `callback` is read through a ref, so passing a fresh closure each render does
 * NOT restart the timer — only `intervalMs`, `enabled`, and `leading` do.
 */
export function useVisiblePolling(
  callback: () => void,
  intervalMs: number,
  enabled = true,
  leading = true,
): void {
  const cbRef = useRef(callback);
  cbRef.current = callback;

  useEffect(() => {
    if (!enabled) return;
    let timer: ReturnType<typeof setInterval> | null = null;

    const start = () => {
      if (timer === null) timer = setInterval(() => cbRef.current(), intervalMs);
    };
    const stop = () => {
      if (timer !== null) {
        clearInterval(timer);
        timer = null;
      }
    };

    const onVisibility = () => {
      if (document.hidden) {
        stop();
      } else {
        cbRef.current(); // catch up on whatever changed while hidden
        start();
      }
    };

    if (!document.hidden) {
      if (leading) cbRef.current();
      start();
    }
    document.addEventListener('visibilitychange', onVisibility);

    return () => {
      stop();
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [intervalMs, enabled, leading]);
}
