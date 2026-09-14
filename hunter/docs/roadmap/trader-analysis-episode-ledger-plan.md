# Trader Analysis episode ledger - what is still open

Trader Analysis reads one row per (wallet, mint) over the window: an avg-cost rollup
(`kernel::wallet_mint_pnl`), net of fee, with every per-trade figure in
`walletPnlStats.ts` (`WALLET_STATS`). A "trade" there is one token, however many
times the wallet re-entered it, so these stay unanswerable:

- a per-trade PnL % distribution when a wallet re-enters a mint (the page folds
  every visit into one);
- the hold of one round trip (the page's hold spans every re-entry);
- how deep a trade went under water before it was sold (max adverse excursion).

## 1. The episode

An episode opens on a buy while the wallet holds no tokens of that mint and closes on
the sell that brings its held amount back to zero. The next buy opens a new one. Legs
fold in the canonical trade order `(slot, tx_index, leg_index)` - never `block_time`,
which ties across a slot, and never a `leg_index = 0` filter, which drops buys.

Cost basis is avg-cost **within the episode**: a partial sell realizes against the
episode's running average buy price. A mint traded as one episode then reproduces
`wallet_mint_pnl` exactly, which is the parity test.

An episode still holding at the window's end is open (marked at `current_price`, as
now). An episode whose first leg in the window is a sell is partial (its buy predates
the window) and carries the same `partial` flag the rollup does.

## 2. Where it is computed

- `kernel::wallet_episodes(legs) -> Vec<WalletEpisode>`: the pure fold, next to
  `wallet_mint_pnl`, unit-tested on golden leg sequences (one episode, a re-entry, a
  partial sell, an oversold window). Each episode: entry / exit `(slot, tx_index)` and
  time, buy / sell SOL, matched cost, gross and net realized, net %, hold.
- Legs come from one bounded PG read per page load: the wallet's legs on the page's
  mints in the window, same `wallet_dict` proxy exclusion as `traded_mints_agg`.
- The wire: `episodes: WalletEpisode[]` on each `WalletTokenRow`. Mint-grain columns
  and focus keep working; the summary's trade grain moves to episodes.

## 3. Max adverse / favorable excursion

Per episode: the lowest and highest pool spot (`TradeRow::chart_spot_price`, the
series the charts draw) over every print of the mint between entry and exit, against
the episode's average entry price, as a %. MAE % = (min spot / avg entry - 1) x 100.
Unit, basis and window go into `WALLET_STATS` with the metric.

This needs every print of each traded mint over each episode, not only the wallet's
own legs - the expensive part. Open choice: one PG query per page (min / max spot per
episode range, grouped) against the lake for sealed days plus PG for the tail
([lake-pg-read-paths.md](../plans/database/lake-pg-read-paths.md)).

## 4. SOL basis

The rollup and the episodes price curve-side `amount_lamports` with the 125 bps fee.
`trades.payer_net_lamports` is what each transaction moved in the payer's wallet, all
fees and tip included (NULL before migration 0019). Where every leg of an episode
carries it, it is the exact figure; the choice (exact when complete, curve-side
otherwise, the episode labelled with its basis) is open.

## 5. Summary once episodes land

`WALLET_STATS` gains `episodeCount`, `medianMaePct`, `worstMaePct`, `medianMfePct`;
`tradeCount`, the per-trade %, hold and loss streak switch to the episode grain, each
definition updated in the same commit. Workstation only: `lab` bin, no Helius call.
