# Trader Analysis episode ledger - what is still open

Trader Analysis reads one row per (wallet, mint) over the window: an avg-cost rollup
(`kernel::wallet_mint_pnl`) on curve-side SOL less the 125 bps fee, with every
per-trade figure in `walletPnlStats.ts` (`WALLET_STATS`). Two things keep it from
matching what the wallet did:

- **the SOL is modeled**: curve-side `amount_lamports` less 125 bps misses the network
  fee, the priority fee, the tip, and the PumpSwap fee on a migrated coin;
- **a trade is one token**, however many times the wallet re-entered it, so the
  per-trade distribution, hold, loss streak and max drawdown fold every visit into one.

The target: every figure on the page equals what the wallet moved, trade by trade.

## 1. SOL basis - exact only

A trade's SOL is its transactions' **payer net flow** (`trades.payer_net_lamports`,
migration 0019): what the transaction took from or returned to the wallet, every fee,
tip and venue charge included - the figure an on-chain tracker reads. There is no
modeled fallback: a leg without an exact figure makes its episode **incomplete** (§3).

A leg's flow is exact when all of these hold:

- `payer_net_lamports` is not NULL. The column is forward-only, so nothing before
  0019 reaches the server can ever be exact (`raw_txs` keeps no payload to re-decode);
- the transaction's payer is the wallet (`payer_id = wallet_id`). A bot that pays from
  one keypair and trades from another moves the payer's SOL, not the wallet's;
- the transaction's legs for this wallet all sit on one mint. The flow is
  per-transaction, denormalized onto every leg (collapse by signature), and cannot be
  split across mints.

Rent a buy parks in the wallet's own token account counts as still the wallet's
(0019), so a close in a separate transaction moves only its 5,000-lamport fee, which
is not a trade and is not booked.

**Prerequisite:** migration 0019 and the live binary deployed to the server
([exact-pnl-plan.md](exact-pnl-plan.md) item 1). Until then every episode is
incomplete and the page shows none. The lake export gains the column before a window
older than PG's ~30-day retention can be exact.

## 2. The episode

An episode opens on a buy while the wallet holds none of that mint and closes on the
sell that brings its held amount to dust - at most 0.1 % of the tokens the episode
bought. The next buy opens a new one. Legs fold in the canonical trade order
`(slot, tx_index, leg_index)` - never `block_time`, which ties across a slot, and
never a `leg_index = 0` filter, which drops buys.

Closed episode PnL = Σ sell flow − Σ buy flow; PnL % = that over Σ buy flow. No cost
method enters a closed episode. A partial sell realizes against the episode's running
average buy cost, which only an open episode's split needs.

An episode whose opening buy predates the window is read from its opening buy: the
window selects episodes by close, and the fold looks back to the buy through PG and
the lake.

## 3. Incomplete episodes

An episode the trades table cannot fully see is **incomplete**, counted and shown on
the page, and left out of every PnL figure:

- a leg without an exact flow (§1);
- a sell of more tokens than the episode holds - tokens arrived without a buy
  (a transfer in);
- no opening buy within the data held.

## 4. Open episodes

An open episode has no exact value until it sells. It is shown apart, at the
kernel's mark (`current_price`, labelled an estimate), and stays out of Total, the
per-trade stats and the drawdown. Tokens that left by transfer read as open too: the
trades table carries no transfers, so the page cannot tell them from a bag still
held, and keeping open episodes out of every PnL figure keeps them from bending one.

## 5. Max drawdown - the most lost at once

The running Total adds each closed episode's exact net SOL at its close, ordered by
the closing sell's `(slot, tx_index, leg_index)`, starting from 0. Max drawdown is the
deepest fall of that running Total below its highest point so far - the largest loss
in one stretch, whatever came before it. This is the Equity chart's fold
(`buildEquityCurve` + `maxDrawdownSol`), moved from one point per token to one point
per episode.

## 6. Where it is computed

- `kernel::wallet_episodes(legs) -> Vec<WalletEpisode>`: the pure fold, next to
  `wallet_mint_pnl`, unit-tested on golden leg sequences (one episode, a re-entry, a
  partial sell, dust, an oversold sell, a missing flow, a payer that is not the
  wallet). Each episode: entry / exit `(slot, tx_index)` and time, buy / sell flow,
  net SOL and %, hold, status (closed / open / incomplete + reason).
- Legs come from one bounded PG read per page load: the wallet's legs on the page's
  mints, window plus look-back, with `payer_net_lamports`, `payer_id` and
  `signature`, same `wallet_dict` proxy exclusion as `traded_mints_agg`.
- The wire: `episodes: WalletEpisode[]` on each `WalletTokenRow`. A mint's row PnL is
  the sum of its closed episodes; a row with an incomplete episode shows the count.

## 7. Summary once episodes land

`tradeCount`, win rate, the per-trade %, expectancy, worst trade, hold, loss streak and
max drawdown switch to the episode grain on the exact basis; `WALLET_STATS` gains
`episodeCount`, `incompleteCount`, `exactShare`, each definition updated in the same
commit. Workstation only: `lab` bin, no Helius call.
