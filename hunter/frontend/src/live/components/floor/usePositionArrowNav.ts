import { useEffect } from 'react';

/**
 * While a position modal is open, ArrowLeft / ArrowRight moves to the previous /
 * next key in `keys` (cockpit lane or History page). Ignores events from inputs
 * so TP/SL and mint fields keep working.
 */
export function usePositionArrowNav({
  enabled,
  keys,
  selectedKey,
  onSelect,
}: {
  enabled: boolean;
  keys: readonly string[];
  selectedKey: string | null;
  onSelect: (key: string) => void;
}): void {
  useEffect(() => {
    if (!enabled || !selectedKey || keys.length < 2) return;

    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
      if (e.altKey || e.ctrlKey || e.metaKey) return;
      const t = e.target as HTMLElement | null;
      if (
        t &&
        (t.tagName === 'INPUT' ||
          t.tagName === 'TEXTAREA' ||
          t.tagName === 'SELECT' ||
          t.isContentEditable)
      ) {
        return;
      }
      const idx = keys.indexOf(selectedKey);
      if (idx < 0) return;
      const next =
        e.key === 'ArrowRight'
          ? keys[(idx + 1) % keys.length]
          : keys[(idx - 1 + keys.length) % keys.length];
      if (!next || next === selectedKey) return;
      e.preventDefault();
      onSelect(next);
    };

    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [enabled, keys, selectedKey, onSelect]);
}
