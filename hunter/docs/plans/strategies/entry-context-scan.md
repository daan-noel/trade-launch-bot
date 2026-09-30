# Entry Context: the market scan

The Entry Context page tests an idea (the buys table's filters, apart from the probe) on his
buys. The market scan finds the same idea on the rest of the market: every buy of the selected
structure whose analysis window passes those filters.

## Scan buys

`POST /api/wallets/{wallet}/entry-context/scan` (`entry_context.rs::read_entry_scan`) takes the
entry-context body. It requires a target. For every mint traded in the range
(`TradeRepo::traded_mint_spans`) it loads the tape from that mint's first trade in the range to
its last, and returns each buy whose own transaction matches the target (a `ScanMoment`).
Sticky is off for that test, so a later buy by a wallet that already traded the target is not
itself a scan buy. A sell is never one.

Each scan buy is read from the seat of a follower (`point_of` on an anchor at the tape position
right behind that buy: the last `W` seconds, the Earlier stretch, the probe), with the studied
wallet's own trades left out. The window and the probe hold the scan buy itself, the way his
window holds the target buy he follows. His entries carry the matching read: `at_signal` is his
entry read again from the seat behind its signal (`seat_behind` on the probe's nearest target
print, the one function the scan uses), and the page reads a pool buy from it
(`atDecision` in `lib/entryContext/analysis.ts`). One target buy therefore passes or fails the
filters the same on both sides, and a token where he bought at a point is a token the scan
passes. A buy outside the pool has no signal and keeps the read at his own buy. Scope filters
(token, time, his SOL) and the probe thresholds do not apply to a scan buy.

Each scan buy also carries:

- `price`: the last trade's price at the buy (`price_of`, SOL per token, his trades left out).
- `ret_pct`: the price change from `price` to the last trade price `AFTER_SECS` (30 s, 120 s)
  later, in percent. No trade in between = no move = 0.
- `next_buy_secs`: seconds from the buy to his next buy on the mint, if any.

The breakdown is cut to its top row (the Top structure axis reads it). Scan buys past
`MAX_SCAN_MOMENTS` (100 000, the most recent first) are dropped and flagged; a buy whose read
reaches past the tape floor is `tape-truncated`.

## Points and chances

The page runs the scan on demand (one button, on the Market tab). The
backend knows no filters; the page applies them (`EntryLogic.idea`: Target tx % and the other
non-probe filters).

- A **point** is a scan buy that passes the filters: it matches the structure AND the filters.
- A scan buy that fails the filters is not drawn. It only ends a chance.
- A scan buy the scan could not read is skipped.

A **chance** is one opportunity the market gave for the idea: a container of points
(`frontend/src/lab/lib/entryContext/chances.ts`). On one token, in tape order, a chance starts at
a point; each next point joins it as one more check moment; it ends at the first scan buy that
fails the filters, or when the next point comes more than **Max pause** seconds after the last
one. The analysis window only says what a buy reads; it never starts or ends a chance.

**A failing buy ends a chance** is a toggle, on by default. Off, a failing buy is passed over:
only a pause longer than Max pause ends a chance, and the pauses and the count row are read the
same way.

Buy times are stamped to the millisecond, so buys of one slot are milliseconds apart and always
join under any Max pause of 1 s or more.

- **Headline:** `N chances on M tokens (from K points)`.
- **Max pause:** a box on the page. Empty, it follows the suggestion: the smallest step of the
  row below whose chance count differs from the next step's by under 2 %, i.e. where the count
  stops moving. When it never settles, 30 s is used. Beside it: the min, median and max of the
  pauses between points in a row (no failing buy between).
- **Ended by:** how many chances a failing buy ended, how many a pause, and how many ran to the
  token's last point.
- **Chances at max pause:** the count at 1, 2, 5, 10, 20, 30, 60, 120 and 300 s.
- **Token table:** one row per token with a chance, a Chances column (the default sort) and a
  Points column.
- **Charts:** one marker per chance at its first point (`Chance · N pts`). Show points (off by
  default) adds a marker for each later point. His buys stay marked.

On the 8dtx 6Vo3 set with Target tx % > 50 and a 30 s window (09-27 12:00-13:00 UTC): 6 477 scan
buys, 1 053 points, 234 chances. The count settles at 30 s (248 at 20 s, 234 from 30 s up):
a scan buy more than 30 s after the last one has no target buy in its window, so it fails the
filter and ends the chance first. Max pause matters only for a filter that can stay true through
a silence longer than the window. With the toggle off, the same hour reads 367 chances at 5 s,
198 at 30 s and 110 at 300 s: the count keeps falling, so Max pause alone decides it.

His own token table is unchanged: it is the tokens of his that pass the filters. Checking a
chance against his buys (hit or miss, [entry-condition-measurement.md](entry-condition-measurement.md))
is not on the page yet.
