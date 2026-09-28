# Entry Context: open work

The page (`/analysis/entry-context`, [frontend.md](../arch/frontend.md)) reads the `W` seconds
before each of a wallet's buys under one target tag. What it does not do yet:

1. **Market scan.** The same condition over every token in the range, not only his: how often
   it fires on the market, and how many of those firings he bought. Needs a new moment source
   (first time the condition holds per token, episode-deduped) run as a lab background job on
   the workstation, restricted to the same token stage (age, curve depth) as his entries.
2. **Outcome axes.** What happened after the moment: peak return within X seconds, return at a
   fixed time. New axes in `lib/entryContext/axes.ts` fed by a server read after the anchor.
3. **Random-moment baseline.** The target share at random non-entry moments on the same tokens,
   beside the control window.
4. **Engine share metric.** The target share is read as `@tag / (@tag + @!tag)` of `buy_tx_count`
   and `buy_sol`. A rule cannot say that today: shipping a finding as a rule needs a buy-side
   share metric in `m_flow` (tx and SOL), with its registry definition.
5. **More breakdown axes.** Fee preset (CU limit, CU price, tip) and wallet as `group_by` keys.
6. **Target from a breakdown row.** Clicking a structure in the breakdown to make it the target
   (write it into the lens set).
