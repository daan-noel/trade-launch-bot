// --- pullback / higher-low pairing pre-run check ----------------------------

/** Pre-run advisory mirroring the backend `tpsl2_higher_low_gate_sane` guard:
 *  `entry_higher_low_secs` is a sub-parameter of the pullback gate — the live
 *  entry gate only reads it while `entry_pullback_pct` is enabled (see
 *  `entry::scalp`), so a combo that sets higher-low seconds while every swept
 *  pullback candidate is disabled is a silent no-op.
 *
 *  The backend PRUNES any swept combo that violates this (so a "winner" you
 *  copy always saves), which makes this advisory, not a blocker: it just tells
 *  the user up front that part of their grid will be dropped. Pure; safe
 *  inside a `useMemo`. Returns a warning string, or `null` when every combo is
 *  gate-valid. */
export function tpsl2AxesGateWarning(
  spec: Record<string, (number | null)[]>,
): string | null {
  const higherLowOn = (spec.entry_higher_low_secs ?? []).some(
    (v) => v != null && v !== 0,
  );
  const pullbackOn = (spec.entry_pullback_pct ?? []).some(
    (v) => v != null && v !== 0,
  );

  if (higherLowOn && !pullbackOn) {
    return 'Entry higher-low (s) is set but every Entry pullback % candidate is off (disabled) — higher-low seconds has no effect without a pullback gate, so those combos are skipped. Add a non-zero Pullback % candidate to sweep it.';
  }
  return null;
}
