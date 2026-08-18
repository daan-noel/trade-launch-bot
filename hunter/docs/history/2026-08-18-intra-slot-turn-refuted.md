# 2026-08-18 — Intra-slot "turn" rule refuted; exits must be priced at +1 slot

## What was claimed

A discriminative search over 4.05M decision points produced `recov >= 15` (price bounced
15%+ off its low inside the decision slot) as an entry signal, reported at **+6.03%/trade,
44.2% win, 7/7 days positive**, harvested with a 4% armed trailing stop. Three validations
were left open: full token universe, out-of-sample period, and a path-ordered exit.

## What the validations found

All three were closed. The rule earns nothing.

Universe was widened from wallet 662's 11,864 mints to all **345,869** mints (14.3M
slot-states, 08-01..08-16), and the period split into OOS 08-01..08-08 / IS 08-09..08-15.
The entry signal survived both — and then died on execution.

| correction | IS | OOS |
| --- | --- | --- |
| as reported (ideal exit fill, 3.0% cost) | +4.19 | +3.86 |
| exit capped at the breach slot's high | +0.59 | +0.20 |
| true 3.3% round trip | +0.29 | -0.10 |
| exit charged +1 slot, as the entry is | **-4.22** | **-3.12** |

Negative on 14 of 16 days. A matched control — random print, same tokens, same fill, same
exit, same cost — earned +0.29 IS / -0.11 OOS, so even at the ideal fill the signal was
worth only **+0.3pp** over doing nothing in particular. Fixed holds at 2/5/12/25/50/75 slots
returned -4.31 to -11.03%: the signal buys a local top and mean-reverts.

An extreme-move variant (`rise>=50 & pool>=15`) looked alive at +3.65 IS / +5.69 OOS with
all 16 days positive and ~145 trades/day, and collapsed to -4.22 / -3.12 under the same
latency correction.

## Root cause

The simulator filled the trailing stop **at the trail level in the slot the trail broke**.
That is not reachable. Two separate optimisms hid inside it:

1. **Gapping.** When the whole slot traded below the trail level, the fill was still booked
   at the trail level. Capping at the slot high cost 3.6pp.
2. **Reaction latency.** A trail only triggers once price is already falling, so reacting to
   the breach is adverse selection by construction. The entry was correctly charged +1 slot;
   the exit was charged zero.

## The generalizable result

The first correction was calibrated, not assumed. Across **7.09M real sells**, comparing each
seller's realized price to `plast` of the previous slot (what a +1-slot reactor sees when it
decides): all sellers -5.03% mean / -1.14% median; **wallet 662 -1.75% / -1.85%**, which is
roughly his own impact alone. A good executor realizes the decision price. Exit speed is not
the bottleneck — this bot runs at p50 1 slot, 55% next-slot, at the physical floor.

The real defect was **slot-granularity exit resolution**. Filling a trailing stop at the trail
level in the slot it breaks is unreachable: per-print data (14.0M individual trades over
206,846 entries) shows the first print at or below the trail has usually already gapped well
past it. Re-scored per print, at 64hP's own measured execution quality (decision print price
minus 0.3% own impact, zero adverse drift), the rule returns **-2.22% IS / -2.61% OOS,
negative on all 16 days**. It fails at the best execution any real wallet in the data achieves,
so execution is not what killed it — the signal buys a local top.

## What changed as a result

- Every exit is now priced at **+1 slot, symmetric with the entry**. A backtest that fills an
  exit at its trigger price is not a backtest.
- Tight reactive trails (<=4%) are off the table at current latency; use a wide trail or a
  non-reactive exit.
- Wallet 64hP's exit was re-read: winners retrace 5.7% at exit, losers 19.1%. A trailing stop
  fires at a constant retrace, so a 3x difference by outcome means he was never running one.
  The "armed trailing stop" description of him is withdrawn.
- `m_price_window.rise` with `window_size_sec: 0.4` was confirmed to express this signal with
  no new metric (`window_key` rounds to ms; `block_time` carries microseconds). The earlier
  claim that a new intra-slot metric was required was wrong. Moot for this rule, still true
  for future ones.
