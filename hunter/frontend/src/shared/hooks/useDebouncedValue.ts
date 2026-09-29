import { useEffect, useState } from 'react';

/**
 * `value` once it has held still for `ms`. A knob that drives a server read passes
 * through this, so dragging a number from 5 to 50 fires one read, not forty-five.
 */
export function useDebouncedValue<T>(value: T, ms: number): T {
  const [settled, setSettled] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setSettled(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return settled;
}
