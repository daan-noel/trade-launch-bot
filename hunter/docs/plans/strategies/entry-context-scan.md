# Entry Context: the market scan

The Entry Context page tests an idea (the buys table's filters, apart from the probe) on his
buys. The market scan finds the same idea on the rest of the market: every buy of the selected
structure whose analysis window passes those filters.

## Points

`POST /api/wallets/{wallet}/entry-context/scan` (`entry_context.rs::read_entry_scan`) takes the
entry-context body. It requires a target. For every mint traded in the range
(`TradeRepo::traded_mint_spans`) it loads the tape from that mint's first trade in the range to
its last, and keeps each buy whose own transaction matches the target. Sticky is off for that
test, so a later buy by a wallet that already traded the target is not itself a point. A sell
is not a point.

Each point is read as a buy is (`point_of` on an anchor at that buy: the last `W` seconds, the
Earlier stretch, the probe), with the studied wallet's own trades left out. The buy itself sits
outside its window, the same way his buy sits outside his. The page then keeps a point when the
idea holds (`EntryLogic.idea`: Target tx % and the other non-probe filters). Scope filters
(token, time, his SOL) and the probe thresholds do not apply to a point.

Each point also carries:

- `price`: the last trade's price at the buy (`price_of`, SOL per token, his trades left out).
- `ret_pct`: the price change from `price` to the last trade price `AFTER_SECS` (30 s, 120 s)
  later, in percent. No trade in between = no move = 0.
- `next_buy_secs`: seconds from the point to his next buy on the mint, if any.

The breakdown is cut to its top row (the Top structure axis reads it). Points past
`MAX_SCAN_MOMENTS` (100 000, the most recent first) are dropped and flagged; a point whose
read reaches past the tape floor is `tape-truncated`.

## What the page reads from it

The page runs the scan on demand (one button, its own section under his token table). The
section lists one row per token that has a point the idea holds for, through the shared token
table. That token's chart marks each such buy, and his buys stay marked. His own token table
is unchanged: it is the tokens of his that pass the filters.
