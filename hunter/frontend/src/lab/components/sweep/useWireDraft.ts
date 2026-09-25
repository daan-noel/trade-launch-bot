import { useEffect, useRef, useState } from 'react';

/**
 * Editor state for a wire document the sweep form stores (tags, a stage plan). The
 * editor models carry per-item ids that key their rows, so the document is parsed
 * once and re-parsed only when the stored value changes from outside (a re-run, a
 * template, a fingerprint's tags): re-parsing on every keystroke would mint new ids and
 * remount the row being typed in.
 */
export function useWireDraft<T>(
  wire: unknown,
  parse: (wire: unknown) => T,
  write: (draft: T) => unknown,
  onWire: (wire: unknown) => void,
): [T, (draft: T) => void] {
  const [draft, setDraft] = useState<T>(() => parse(wire));
  const sent = useRef(JSON.stringify(wire ?? null));
  const parseRef = useRef(parse);
  parseRef.current = parse;
  useEffect(() => {
    const s = JSON.stringify(wire ?? null);
    if (s === sent.current) return;
    sent.current = s;
    setDraft(parseRef.current(wire));
  }, [wire]);
  const set = (next: T) => {
    setDraft(next);
    const w = write(next);
    sent.current = JSON.stringify(w ?? null);
    onWire(w);
  };
  return [draft, set];
}
