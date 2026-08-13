# A correct `nonvol_buy` exit that nothing on screen could explain (2026-08-13)

**Symptom.** Four positions across `8NsSEHLwfyiJDgt5UbXiNApvNwbbVmaEaUHbmkkUpump` and
`HX9PCYbAUKR6AYaRT6AWRrHEhEqu84c5WoD8uBxBpump` (rule `-- promoted g3 c145802`,
fingerprint `d5b5c6f3`) closed 0.1–2.2 s after entry on `nonvol_buy >= 0.9`. Summing the
non-volume trades off the position modal's chart gave ~**0.39 SOL**, and the modal's
chips read **1.6449775** on every empty candle — the same number, candle after candle.
Every value the engine produced was correct.

**Cause — one classification gap, three surfaces that hid it.**

The fired req is `m_flow_split_window.nonvol_buy >= 0.9` at `window_size_sec = 2`. At the
real position's exit (`11:59:55.675854`) the window held two organic buys — 0.7491 SOL at
`11:59:53.723` and 0.8959 SOL at `11:59:55.589`, summing to **1.644977**, matching the
engine's readout to seven digits. Both carry
`[SetComputeUnitPrice, SetComputeUnitLimit, Associated Token: CreateIdempotent,
Pump.Fun: Buy, System Program: Transfer]`. The fingerprint held that sequence *without*
the trailing tip transfer, and the `Associated Token: Create` variant *with* it — but not
this one. Matching is an exact ordered hash, so two bot buys booked as organic demand.
Replaying with the variant added drops the window to **0.000**: no exit at all.

Nothing on screen could show that:

1. **The chart classified with the browser's unsaved pattern draft.** Same slot, same
   trades: saved set → NonVol 1.8861, the UI → 0.3880. `useEffectiveFlowPatternKeys`
   layered an app-wide draft over the saved keys with nothing marking it, so a hand-sum
   could not reach the engine's number.
2. **The exit label named a metric that did not fire.** `nonvol_buy` is the registry name
   of BOTH `m_flow_split.nonvol_buy` (lifetime, monotone) and its windowed twin;
   `format_metric_exit_label` emitted the bare name, and `parse_metric_exit_label`
   resolved it back to the *lifetime* id.
3. **The crosshair could not resolve an empty slot bar**, so the strip's `?? data`
   fallback showed the pinned exit readout on every gap bar — hence one repeated number.
   And the only line drawn, `NonVol`, is a cumulative NET since creation, not a
   trailing-window buy sum.

**Fix.** Exit labels carry the window (`nonvol_buy(2s) >= 0.9`) and the qualifier selects
the `MetricId` on parse, legacy forms still reading; the sweep carries
`exit_metric_window` so drill-in prints the same label. `TokenTrack::ensure_flow` adopts
an edited pattern set on already-tracked tokens instead of keeping the first-seen one.
The position modal is a `VolumePatternScope locked` and says when a draft is ignored;
`buildBarWallEndSec` resolves every bar including empty slot bars; `seriesIndexAsOf`
replaces nearest-row lookup; an unresolvable hover says so instead of borrowing the pin;
the fired condition is drawn as a value line with its threshold; lanes render in slot
mode; the overlay is labelled `∑net`.

**The rule this produced.** A pattern list carries **variants or nothing** — the same bot
appears with and without a trailing tip transfer and with `Create` vs `CreateIdempotent`,
four sequences for one behaviour, and a partial list books the rest as organic demand.
Audit by variant, never by example. That rule now lives in
[metrics-reference.md](../plans/strategies/metrics-reference.md) *Classifier*. Corollary
for the surfaces: any view reporting a decision already taken must classify with the set
that decision was taken under ([frontend.md](../arch/frontend.md) *Console*), and when an
exit looks impossible, replay the saved `metric_config` against `trades` before reading
anything off a chart.
