# Entry Context: open work

The page (`/analysis/entry-context`, [frontend.md](../arch/frontend.md)) reads the `W` seconds
before each of a wallet's buys under one target tag. What it does not do yet:

1. **Same-stage market.** The market scan
   ([entry-context-scan.md](../plans/strategies/entry-context-scan.md)) keeps every target buy in
   the range; a cut to the token stage of his entries (age, curve depth) is open.
2. **Outcome axes on his buys.** The scan prices moments after; his buys carry no price-after
   column yet (peak return within X seconds, return at a fixed time), which would make the
   outcome filterable in the buys table.
3. **Engine share metric.** The target share is read as `@tag / (@tag + @!tag)` of `buy_tx_count`
   and `buy_sol`. A rule cannot say that today: shipping a finding as a rule needs a buy-side
   share metric in `m_flow` (tx and SOL), with its registry definition.
4. **More breakdown axes.** Fee preset (CU limit, CU price, tip) and wallet as `group_by` keys.
5. **Target from a breakdown row.** Clicking a structure in the breakdown to make it the target
   (write it into the lens set).
