# Roster nodes: N4 and N5

Phase-3 instrument study per
[market-model-and-workflow.md](market-model-and-workflow.md), read under
[trader-study-contract.md](trader-study-contract.md): the roster locates the
node, never the rule.

Window 08-27 .. 09-03 (7.4 days). Tables `census.rb_ep2`, `rb_feat`, `rb_k`,
`rb_ctx`, `rb_cslot`. Same rows and same fields in
`hunter/_local/roster-nodes.xlsx` and the per-node CSVs beside it.

---

## 1. The logic the grouping comes from

A price on a bonding curve already contains every trade that has printed, so
observed flow is spent information. Profit can therefore only come from flow
that has not arrived yet, and that flow exists only because some actor still
intends to spend. Every entry is an inference about someone's remaining budget.

So the question that separates traders is: **whose money is still coming, and
what on the tape says so.** Each distinct answer is a decision node. Entry age,
tape density, pullback-versus-strength and repeat entry are the observable
shadows of that answer, which is why grouping on them recovers nodes rather
than styles.

Our seat decides which answers are reachable: decision to fill is p50 115 ms
against a ~400 ms slot, 52.6 % of real buys land in the trigger's own slot.
A node whose edge is consumed inside its own slot is closed to us however well
it pays its owner.

Grouping is k-means (k = 5, silhouette 0.345) over log entry age, log hold, log
tape density, pullback share, log silence-break share, repeat index, log vsol
and no-prior-tape share. Three of the five nodes are not listed here: two are
launch-slot nodes closed by construction, and the third harvests by tranching
out of one buy, so it holds two wallets under the scope rule below.

---

## 2. Scope

A **trade** is one round trip on one token: it opens on a buy from a flat
position and closes when the position returns to flat.

Listed wallets take **exactly one buy and one sell per trade on at least 95 %
of their trades**. Re-entry is unrestricted - a wallet may open and close the
same token many times. In scope, the worst wallet averages 1.03 buys and 1.02
sells per trade.

210 wallets fall outside and sit in `EXCLUDED-not-one-buy-one-sell.csv`
with the reason on each row: 58 scale
out, 56 scale in,
96 do neither consistently.

The scope rule is expensive, and its cost is monotone in purity:

| share of trades that are exactly 1 buy + 1 sell | wallets | median return on spend | median trades |
| --- | --- | --- | --- |
| under 50 % | 152 | 13.38 % | 167 |
| 50-80 % | 114 | 15.38 % | 149 |
| 80-95 % | 37 | 5.24 % | 209 |
| **95 % and over** | **79** | **1.79 %** | **2,100** |

One-in-one-out wallets trade ~13x more often for ~8x less margin per trade.
The rule selects the high-frequency, thin-margin end of the population, and
thin margin is what a late fill consumes first.

---

## 3. Fields

Every number is a per-wallet figure over the window, computed from that
wallet's own landed transactions. Medians, not means, wherever a distribution
is skewed.

| field | meaning |
| --- | --- |
| `grade` | A to D by quartile of a within-node score. The score is the mean of three percentile ranks: return on spend, days profitable, and trade count. All three are required - a high margin over 40 trades is not a habit, and a large book that loses on a third of its days is not effective |
| `known_as` | study nickname, where the wallet has one |
| `wallet_address` | on-chain address |
| `operator_id` | wallets whose ids are adjacent were first seen together, so they are one funding batch - one operator running several addresses. Rows sharing this id are not independent evidence |
| `operator_wallets` | how many addresses that operator runs inside this node |
| `pct_return_on_spend` | **the ranking column.** Net SOL kept per 100 SOL deployed: `(SOL received x 0.9875 - SOL paid x 1.0125) / SOL paid`, so 1.25 % is charged on each leg. No fill model and no latency assumption - this is what the wallet itself banked. Read it as the budget available to absorb the gap between their fill and ours |
| `net_sol` | total SOL kept over the window, same fee basis |
| `pct_days_profitable` | share of the wallet's active days that end net positive |
| `pct_trades_profitable` | share of individual trades that end net positive. Well under 50 % is normal - these books are carried by a tail |
| `worst_day_sol` | the single worst day, in SOL |
| `trades` | round trips completed |
| `tokens` | distinct tokens traded |
| `trades_per_token` | round trips per token - the re-entry rate. 1.0 means one shot and move on |
| `median_entry_token_age_s` | seconds between token creation and the entry buy - where in the token's life the wallet acts |
| `median_hold_s` | seconds from entry buy to closing sell |
| `median_buy_sol` | size of the entry buy |
| `median_entry_curve_vsol` | virtual SOL reserve at entry, i.e. how far up the curve. The curve starts at 30 and graduates at 114.9 |
| `median_tape_prints_60s` | trades printed on that token in the 60 s before entry - how busy the tape is when the wallet acts |
| `pct_entries_on_dip` | share of entries below 97 % of the token's 60 s running high. High means the wallet buys pullbacks; low means it buys strength |
| `pct_entries_after_silence` | share of entries that break a gap of 10 or more empty slots - the dev-campaign signature |

---

## 4. N4 - mid-tape generalist

54 wallets, 50 operators, 125,906 trades,
1221.4 SOL net. Median return on spend 1.88 % per wallet,
1.79 % per operator; the largest operator holds 14.9 % of node
net.

Enters ~71.0 s into a token's life on a moderate tape
(40.0 prints in the prior minute), split roughly evenly between
strength and pullback (46.0 % on a dip), takes about one shot per token
(1.07 trades per token) and holds ~19.5 s on a
0.67 SOL clip.

This node carries the roster's highest share of silence-break entries, so the
mid-curve dev-campaign decision lives here. It is also the least homogeneous of
the five - a phase of a token's life rather than one signature - which is why
its wallets spread widely on every field below. 11 of 54 re-enter
at 1.5+ trades per token.

| grade | known_as | wallet_address | operator_id | operator_wallets | pct_return_on_spend | net_sol | pct_days_profitable | pct_trades_profitable | worst_day_sol | trades | tokens | trades_per_token | median_entry_token_age_s | median_hold_s | median_buy_sol | median_entry_curve_vsol | median_tape_prints_60s | pct_entries_on_dip | pct_entries_after_silence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| A |  | GjKjTmzuoAMdxmKHJpQp9PTJLNuvN1SBXAEARr4GEB1c | 19 | 1 | 3.68 | 13.2 | 100 | 41.3 | 0.02 | 3633 | 2307 | 1.57 | 1041.1 | 145.3 | 0.1 | 52.4 | 7.5 | 24.7 | 18 |
| A | 8dtx | 8dtx2tr4TuJsYpri2suggFu1pg3DVjFLBBVmhtDy1MEF | 18 | 1 | 2.65 | 86.43 | 100 | 27.9 | 2.14 | 4429 | 3271 | 1.35 | 126 | 17.1 | 0.65 | 41.1 | 41 | 49.3 | 17.3 |
| A |  | ADkquSiH1zS6LmGoJxzwE6x44gpE6S3KWLALqNc3sTDs | 7 | 1 | 2.32 | 32.38 | 100 | 45.2 | 2.16 | 4043 | 4043 | 1 | 6.3 | 10 | 0.35 | 38.8 | 17.5 | 47.3 | 5.3 |
| A |  | BS5ZZpkieC2N8MJhSDuydRWmvX654F3cEMboz4JkJum7 | 23 | 1 | 3.62 | 22.76 | 100 | 38.9 | 0.5 | 2100 | 1350 | 1.56 | 60.6 | 13.5 | 0.31 | 38.5 | 46 | 71.3 | 10.7 |
| A |  | 6J1T22cKBTw11McuSteoEMpFfMfmWgG9NzsKs11mQYGD | 8 | 2 | 6.34 | 24.27 | 100 | 34.5 | 0.48 | 1460 | 1460 | 1 | 45.9 | 47.5 | 0.28 | 45.4 | 38.5 | 48.7 | 10 |
| A |  | 6cUt6WMbsT5xh8j59AD2ZzumVszcPMGey9VB7dHvz6En | 10 | 2 | 5.71 | 37.62 | 100 | 40.6 | 0.03 | 1642 | 1642 | 1 | 49.1 | 33.5 | 0.39 | 45.5 | 33 | 36.7 | 8.7 |
| A |  | 97jv9pjCf5D7VUZEyj6XnXBC5JF23Vsagc3bvSF6GRmL | 0 | 1 | 1.76 | 33.54 | 100 | 58.6 | 2.41 | 4633 | 4632 | 1 | 7 | 10.1 | 0.4 | 38.8 | 13 | 28.7 | 10.7 |
| A |  | 88887QrRZPZmsstEXsoXXB8E7nbmUDde4Gp5s7jG3ENu | 42 | 1 | 1.52 | 119.07 | 100 | 43.4 | 2.36 | 6484 | 4902 | 1.32 | 49.4 | 23.3 | 1.2 | 38.6 | 74 | 84 | 2 |
| A |  | 5XEj4qt6STm1dKvwTiqiuLRYJoMWUey4BqiwDPTuaUVG | 8 | 2 | 3.75 | 28.94 | 87.5 | 32.4 | -3.09 | 2469 | 2469 | 1 | 38 | 38.9 | 0.29 | 44.4 | 40 | 42.7 | 13.3 |
| A | 3Xk2 | 3Xk2EuuSwKgniGNA4XkB33YY4mnEvcMnTLNpkKgGa14X | 11 | 2 | 3.03 | 51.52 | 87.5 | 38.9 | -0.36 | 2384 | 2102 | 1.13 | 44.9 | 49.2 | 0.72 | 43.7 | 43.5 | 41.3 | 22 |
| A |  | GZmUDsn8W1rtopeTgwVgab9rsRGMLVVMfsXwWKD1VS9p | 1 | 1 | 2.05 | 39.57 | 100 | 41.5 | 0.21 | 2207 | 1911 | 1.15 | 232.4 | 15 | 0.88 | 40.6 | 13.5 | 20.7 | 50 |
| A |  | 9QAEGBhQcRGMS62w2Ktr69gFfRqswVT6Kg92cNBurgLL | 38 | 1 | 4.71 | 32.24 | 100 | 52.5 | 0.71 | 866 | 638 | 1.36 | 176.6 | 8 | 0.92 | 43.2 | 40 | 48.7 | 15.3 |
| A |  | 8aaRWuPGNP1qoBk7jE9ZRMaUhNnCn1cujjUkv6KoEzVL | 16 | 1 | 4.2 | 12.08 | 100 | 52 | 0.13 | 941 | 791 | 1.19 | 75.7 | 23.2 | 0.28 | 51 | 51.5 | 46.7 | 5.3 |
| A |  | 6fKGtgUw3kfEG3cxEGBN4RrwybvBv35zY49iCjwQfyka | 10 | 2 | 5.18 | 47.58 | 87.5 | 40.4 | -0.28 | 1847 | 1847 | 1 | 46.5 | 30.1 | 0.51 | 45.4 | 41.5 | 43.3 | 10.7 |
| B |  | BNqfkxRXdkH1EeM6ZiWtgcySKfxcBL58ENDo9KDZWYB | 5 | 1 | 2.54 | 16.24 | 87.5 | 38.3 | -0.05 | 2089 | 1299 | 1.61 | 65.6 | 13.6 | 0.32 | 38.6 | 40 | 60 | 14 |
| B |  | 7qaQFEd4xDW7u1E8ZozoKKWCRARmbqh9MCY4s1mUcDWw | 21 | 1 | 2.86 | 17.3 | 87.5 | 37.5 | -0.19 | 1997 | 1277 | 1.56 | 56.6 | 12.7 | 0.32 | 38.4 | 43.5 | 66.7 | 9.3 |
| B |  | 9EGEoLk1S947Fa29WbvYBvqJHK4WCYt2dc91CxKJGonz | 47 | 1 | 1.74 | 42.04 | 87.5 | 45.9 | -3.33 | 3063 | 2204 | 1.39 | 310.8 | 60 | 0.79 | 53.5 | 56.5 | 34 | 14 |
| B |  | D9UiteKBjbVp8UYGRSKi4fJRsXYp8Csk7zcrnaCvq6LS | 35 | 1 | 5.78 | 36.39 | 100 | 52.8 | 1.95 | 411 | 366 | 1.12 | 236.5 | 19.5 | 1.48 | 57.4 | 65.5 | 42 | 12.7 |
| B |  | ocBBRKb13iTq9UJUSMhWLKpDcwAvi3FWR3vHr7wH1qa | 15 | 1 | 0.88 | 40.55 | 87.5 | 13.7 | -0.3 | 5196 | 5196 | 1 | 10.8 | 60.1 | 0.89 | 30.9 | 16 | 95.3 | 20.7 |
| B |  | 3TB4EwTwSrkWgjubYYbd6JNmVcTSJ44EgMMwKm8HZABQ | 2 | 1 | 2.37 | 18.63 | 75 | 38.4 | -2.18 | 2596 | 1589 | 1.63 | 64.2 | 13.4 | 0.32 | 38.4 | 44.5 | 62 | 16 |
| B |  | ANQAWcduKeTYn3fVki7hdkTDRUWgB5DTLyissf6AU6Ty | 17 | 1 | 1.96 | 11.86 | 87.5 | 36.8 | -1.05 | 2008 | 1295 | 1.55 | 56.2 | 13.1 | 0.32 | 38.6 | 38 | 69.3 | 8 |
| B |  | 9999huSCf6QpepPHQVFLZ9smhsbwkV4XWPLboiy9qqRj | 11 | 2 | 1.33 | 130.16 | 75 | 42 | -8.74 | 7510 | 6331 | 1.19 | 21.9 | 26.5 | 1.23 | 42.2 | 75.5 | 70 | 0.7 |
| B |  | 9RNZnq4uvU3n3QGhAj2mH7MtLMRq13KYmJwAJP3bd4AG | 43 | 1 | 8.25 | 16.31 | 87.5 | 44.1 | -0.06 | 211 | 141 | 1.5 | 274.2 | 34.3 | 1 | 49.5 | 46 | 50 | 21.3 |
| B |  | EK2jmNwVtnv8cHuQKELbMKF8Y7t75aJ9jduayvpCbC6W | 32 | 1 | 6.51 | 27.74 | 75 | 44 | -1.51 | 521 | 510 | 1.02 | 320.8 | 19.3 | 0.77 | 40.4 | 8 | 24 | 47.3 |
| B |  | BUBBLEtr8BEJZ9PyC2DFYxG58r8tu5PiyAKMVm14hCg9 | 40 | 1 | 0.11 | 1.27 | 75 | 31.5 | -5.36 | 14667 | 12401 | 1.18 | 6.6 | 19.5 | 0.1 | 34.6 | 6.5 | 38.7 | 16 |
| B |  | 67LwNGrukVFFcA1XZQ9U8ddZGCBuPvPHgMxw81AeRtQ7 | 41 | 1 | 0.76 | 11.54 | 75 | 36 | -3.83 | 4868 | 4661 | 1.04 | 9.9 | 13.4 | 0.19 | 39.1 | 31.5 | 58.7 | 5.3 |
| B |  | 8GSDbEhMy3LbuLnqyb1ehQr1wwX3yiMN5pL98NnEM32a | 20 | 1 | 1.6 | 10.21 | 75 | 37.9 | -0.05 | 2121 | 1351 | 1.57 | 61.9 | 12 | 0.32 | 38.6 | 43 | 66 | 16 |
| B |  | pau23UpU2BFwF4JZrLxAnf4ZqgnD3xLnz6ESu7vPsao | 34 | 1 | 1.79 | 15.51 | 87.5 | 40.5 | -0.88 | 1049 | 1049 | 1 | 335.9 | 11.5 | 0.49 | 65.2 | 68.5 | 53.3 | 10 |
| C |  | d4uZpXsNtK7TR9BjysQPdkjMPVftoDS1DguZnBDfk3E | 12 | 1 | 1.79 | 10.35 | 75 | 37.5 | -1.08 | 1905 | 1213 | 1.57 | 66.3 | 13.2 | 0.32 | 38.3 | 44 | 66 | 11.3 |
| C |  | DhMmikTXrfk1bn87XSd9AiMYVNFpFMuDJG593eS4kuvZ | 25 | 1 | 4.5 | 18.31 | 75 | 40.8 | -1.28 | 505 | 489 | 1.03 | 284.8 | 19.6 | 0.77 | 41 | 8 | 22.7 | 40.7 |
| C |  | C6DDrhm8nxVewhZEmswpb1Wmcji3RPcHV7XS7uKFF5FU | 27 | 1 | 2.92 | 12.72 | 75 | 37.1 | -2.58 | 533 | 512 | 1.04 | 289.1 | 19.6 | 0.77 | 40.7 | 10 | 22 | 44 |
| C |  | H1as9cJbd6Uhu7Lv5drRCFKg7nbPnNk3ymeYkJt1dZzQ | 33 | 1 | 3.79 | 16.92 | 62.5 | 38.6 | -2 | 546 | 527 | 1.04 | 297.7 | 19.8 | 0.77 | 40.1 | 9 | 21.3 | 44.7 |
| C |  | HwLUyAPykbPgkotArMazDsNUn7uMyCSBMjm3drDHSBfw | 9 | 1 | 1.04 | 16.31 | 75 | 44.9 | -5.33 | 1979 | 1540 | 1.29 | 109.3 | 18 | 0.79 | 57.3 | 126 | 51.3 | 7.3 |
| C |  | BYXYdwiERyYtMWF7qHaVZTrVP4NCzfiDbzmGMPwPL6dP | 3 | 1 | 1.26 | 47.65 | 75 | 14.8 | -0.9 | 1914 | 1914 | 1 | 7.5 | 75.1 | 1.98 | 32 | 9 | 100 | 20.7 |
| C |  | ArAqZHZwDBFWbyRxqmSWVDYc2o2Zb4sctYMb4MqJmrUH | 48 | 1 | 1.28 | 47.85 | 75 | 15.7 | -1.76 | 1893 | 1893 | 1 | 7 | 120.1 | 1.98 | 32 | 10 | 100 | 17.3 |
| C |  | DyWz2KgbdcGPRDbBN15w9DaWKPvyfQSeLgA6m7NuAEpU | 49 | 1 | 5.57 | 3 | 66.7 | 61.5 | -0.53 | 52 | 51 | 1.02 | 112.8 | 43.7 | 0.98 | 47.9 | 56 | 40.4 | 1.9 |
| C |  | 3aEoBZFwEPB6LeSixoWucBWXFpjRxTESi4vkssZHB6JU | 46 | 1 | 0.68 | 7.95 | 75 | 25.6 | -2.41 | 1987 | 1794 | 1.11 | 127.4 | 18.5 | 0.59 | 36.4 | 15.5 | 42 | 29.3 |
| C |  | ApfmkSSHDssbfpovQwYApbFnLNZkWPTTAA9nfzDggFVY | 45 | 1 | 1.72 | 13.01 | 75 | 55.3 | -1.55 | 704 | 571 | 1.23 | 159 | 9.1 | 0.95 | 52.7 | 80.5 | 40 | 8.7 |
| C |  | GApPtVhB94qhwJ2KCDiSv69pZoaqs9znUMnjWEgCd3LW | 24 | 2 | 2.25 | 9.4 | 75 | 42.7 | -2.37 | 511 | 503 | 1.02 | 314.9 | 19.3 | 0.77 | 40.2 | 7.5 | 20.7 | 47.3 |
| C |  | 9Uq8GVrvSTxGxTYBXvmGJdvbJSfNTdUFgq1HwzNW2xT8 | 22 | 1 | 1.39 | 15.61 | 62.5 | 44.8 | -1.59 | 1929 | 1571 | 1.23 | 129.7 | 16 | 0.59 | 52 | 110.5 | 50 | 17.3 |
| C |  | Eo8ZQxX9pXoUmosc9MaJYMMSE8VPB6JK1M1fSrzdi6h5 | 37 | 1 | -0.19 | -8.46 | 42.9 | 15.7 | -5.4 | 7365 | 7365 | 1 | 12 | 50 | 0.59 | 31 | 41.5 | 100 | 14.7 |
| D |  | J2BA2Nkf9XYVpCUVJio7aaL4vCzxthzGYhFTxsAsDaUy | 29 | 1 | 2.34 | 10.07 | 62.5 | 44.2 | -2.17 | 530 | 516 | 1.03 | 310.2 | 19.6 | 0.77 | 40.7 | 10 | 18.7 | 50 |
| D |  | HV4ZT4QpEW54rywbnigQUadJ4aK1JgieQjXSZ6o7Jy1y | 36 | 1 | -0.21 | -2.98 | 50 | 42.7 | -4.23 | 3753 | 2629 | 1.43 | 64 | 11 | 0.39 | 49.6 | 61.5 | 36.7 | 13.3 |
| D |  | 4HgMCR7abFKHDPgYfMrtjvKniPXj4zHwuSNNRVx7QEzQ | 6 | 1 | 1.1 | 3.81 | 66.7 | 38.2 | -0.57 | 1181 | 1181 | 1 | 3.4 | 3.5 | 0.35 | 32.1 | 4 | 48 | 26 |
| D |  | 82s2hm9RFxesfpSLsLg8kwxKz5XWn1Xorf2GyFQey8Xy | 31 | 1 | 2.11 | 9.08 | 62.5 | 42.7 | -1.39 | 527 | 511 | 1.03 | 291.7 | 19.3 | 0.77 | 40.9 | 9 | 24 | 44 |
| D |  | 5HnzXhdADMB3gn2GphZATAwfrEzzMvG5hBwT36Wtd7JX | 13 | 1 | 0.76 | 4.71 | 50 | 36.3 | -1.3 | 2063 | 1299 | 1.59 | 60.5 | 12.4 | 0.32 | 38.5 | 45 | 66 | 13.3 |
| D |  | EtDKFxGaHZnKEAGEvSpEbW8QYXBV5so6sRrkzaWtcsXM | 44 | 1 | -0.1 | -2.61 | 40 | 16.6 | -2.9 | 3741 | 3741 | 1 | 17.2 | 27.2 | 0.69 | 30.8 | 48.5 | 100 | 18 |
| D |  | 3fdJGPWsoE1dhQNp1H1kwgYn6HFexSoWFAet7SCbTmTn | 4 | 1 | 2.49 | 10.01 | 50 | 37.1 | -3.21 | 493 | 477 | 1.03 | 324.9 | 19.8 | 0.77 | 41.3 | 13.5 | 32 | 44.7 |
| D |  | 7W8SEZv79hk4445o56Fd6RzhbUsozkzGXi8vkc7vNRr8 | 14 | 1 | 0.28 | 1.45 | 50 | 37.8 | -2.6 | 1754 | 1595 | 1.1 | 54.8 | 49.8 | 0.22 | 44.8 | 48 | 45.3 | 17.3 |
| D |  | Had6pusVmdPaXzoWCEAQJQ4dMttPx7YCvii1Udd4nnPT | 24 | 2 | 1.32 | 5.5 | 62.5 | 40.1 | -1.91 | 511 | 498 | 1.03 | 294.2 | 19.7 | 0.77 | 40.3 | 10 | 24.7 | 50.7 |
| D |  | 9a6x28w1qWLxJGs1FMi5MLMDzhgnWnm38SYzkKbrc7kR | 28 | 1 | -0.05 | -0.23 | 62.5 | 39.9 | -4.96 | 539 | 525 | 1.03 | 335.4 | 19.3 | 0.77 | 40.5 | 11 | 24.7 | 50 |
| D |  | 7Ut1gh36seBkUiYuPx3rNQbuLE7QLT53YJRo3wsGAZRZ | 30 | 1 | -0.04 | -0.15 | 62.5 | 40.4 | -2.96 | 527 | 509 | 1.04 | 303.1 | 18.9 | 0.77 | 40.3 | 10 | 20 | 46 |
| D |  | 8qXbAtKV6QNhnWaocgj2ymGL7KqxLuY1NUgRSo6mXd4h | 39 | 1 | 0.24 | 0.59 | 62.5 | 35 | -1.34 | 483 | 280 | 1.73 | 277.6 | 29.5 | 0.49 | 47.5 | 56.5 | 72 | 18 |
| D |  | BNhVytqZgi1Dj9cQR51c5Qc9Nz82jBgYCSd3GmfKAZd6 | 26 | 1 | -1.24 | -5.4 | 25 | 35.3 | -2.59 | 536 | 515 | 1.04 | 318 | 19.5 | 0.77 | 41.5 | 11.5 | 23.3 | 44.7 |

---

## 5. N5 - re-entry dip scalper

23 wallets, 22 operators, 226,140 trades,
1443.8 SOL net. Median return on spend 1.39 % per wallet,
1.48 % per operator; the largest operator holds 33.9 % of node
net.

Enters ~226.8 s in, buys pullbacks (60.0 % on a dip), and returns
to the same token repeatedly - median 2.86 trades per token, and **every**
wallet in this node re-enters - holding ~18.8 s on a 0.47 SOL
clip, the smallest in the roster.

The bet is that the token holds a live range that can be harvested again and
again. This is the one node whose shape is a one-buy-one-sell scalper with
re-entry, and it runs the thinnest margin of the roster at 1.39 %.

| grade | known_as | wallet_address | operator_id | operator_wallets | pct_return_on_spend | net_sol | pct_days_profitable | pct_trades_profitable | worst_day_sol | trades | tokens | trades_per_token | median_entry_token_age_s | median_hold_s | median_buy_sol | median_entry_curve_vsol | median_tape_prints_60s | pct_entries_on_dip | pct_entries_after_silence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| A |  | AbQcLHU2EfzkvBpXwccNxQdWoophiPzZWBLtsUjKaQvZ | 18 | 1 | 2.03 | 129.84 | 100 | 47.8 | 1.83 | 10171 | 3575 | 2.85 | 175.8 | 19.4 | 0.49 | 48.9 | 86 | 79.3 | 8 |
| A |  | 6sViQ1oyvmb6cqB3Emh5Zs9LwV3f6cvVdbg16LHJNvWV | 13 | 1 | 3.26 | 90.92 | 100 | 49 | 0.05 | 3263 | 1602 | 2.04 | 365 | 60.1 | 0.99 | 57.2 | 78.5 | 46 | 18.7 |
| A | 64hP | 64hP97Bwr5PubotcTeGgfhkFrGiLVVxT2kVo9M9b4AEz | 1 | 1 | 1.18 | 488.79 | 100 | 42.9 | 7.68 | 42034 | 13825 | 3.04 | 107.2 | 23.5 | 0.95 | 51 | 147 | 82 | 0 |
| A |  | 8fStGV461vNqwhmQkvYvTFEYkxT4dKqNsyepgtFFDcud | 16 | 1 | 1.76 | 16.76 | 100 | 68.3 | 0.53 | 4876 | 2518 | 1.94 | 99.1 | 8.8 | 0.1 | 71.5 | 183.5 | 82 | 0 |
| A |  | AvgzJYCgqQAHYMAezRtdQtCtmknTMz9rJnkdu3Np7sS1 | 5 | 1 | 3.1 | 18.78 | 100 | 38.1 | 1.06 | 2112 | 741 | 2.85 | 210.1 | 17 | 0.24 | 50.6 | 64 | 58 | 4 |
| A |  | 3vnqjU9PQxLayncdx7DtoJCqTizCDmnKBu5HLeNvrY9p | 4 | 1 | 3.54 | 24.27 | 87.5 | 37.8 | -0.02 | 2428 | 832 | 2.92 | 227.4 | 18.6 | 0.24 | 50 | 56 | 64.7 | 11.3 |
| B |  | ARARAw1AL4kKnuwFyU534yoGv5ytexxeVpbcuEvpqi89 | 12 | 1 | 3.07 | 4.55 | 100 | 83.8 | 0.18 | 148 | 49 | 3.02 | 7.7 | 0.4 | 0.99 | 59.7 | 130 | 98.6 | 0 |
| B |  | GVVP8N7jnxgr3QdtR461bsuCNNQmuNu4DBW2Ab4tzKVp | 3 | 1 | 1.25 | 115.74 | 87.5 | 52.2 | -6.15 | 13520 | 2821 | 4.79 | 660.4 | 38.3 | 0.81 | 64.6 | 64 | 70 | 0 |
| B |  | 49uohdddbmvWpKRPY1prvsYRtX1Bvc5WC5LvtQxYja6o | 20 | 1 | 1.26 | 84.13 | 87.5 | 42.4 | -0.68 | 9267 | 4054 | 2.29 | 169.5 | 25.3 | 0.49 | 47.7 | 84 | 58 | 6 |
| B |  | DjQ5U31K1XfGTrZNczTtmYG76tvn4FZu7VzunhUjhehC | 11 | 1 | 2.81 | 17.44 | 87.5 | 40 | -0.61 | 2215 | 775 | 2.86 | 215 | 19.2 | 0.24 | 50.2 | 53 | 58 | 6.7 |
| B | omego | omegoMAe1AMY5MFKQQr3JwXVy8F4eCvmBAfcpo8XAfq | 2 | 1 | 0.76 | 157.52 | 87.5 | 48 | -16.08 | 28725 | 4908 | 5.85 | 370.3 | 15.3 | 0.62 | 63.5 | 84.5 | 72.7 | 4 |
| B |  | BwRUYhUN18HHM5KdLmzTfrrqRbqP6g7QsHn8tz6g87DL | 14 | 1 | 1.56 | 46.91 | 87.5 | 46.5 | -3.7 | 3503 | 1724 | 2.03 | 355.8 | 60.1 | 0.99 | 57.4 | 59 | 41.3 | 10.7 |
| B |  | 2k2jJVet1SeAKvVSbvd8pZbqpaLcbMgRmYx8EhNTUJf9 | 19 | 1 | 0.99 | 60.52 | 87.5 | 51.1 | -4.47 | 13629 | 2855 | 4.77 | 730.5 | 38 | 0.5 | 60.8 | 63.5 | 66 | 0 |
| C |  | 7Q6RcQKsmpaRsaK17nuVyxDEWMRcPJUhboHvsqNRgTsR | 6 | 2 | 1.05 | 31.85 | 87.5 | 37.5 | -0.98 | 6405 | 2596 | 2.47 | 1083.3 | 52.1 | 0.47 | 46.2 | 10 | 31.3 | 31.3 |
| C |  | HcwFhksAgNWvXJrb1W2BdPCU8rP8vVWwJJ7EnqWNANwF | 21 | 1 | 2.5 | 11.79 | 87.5 | 82.3 | -1.22 | 475 | 163 | 2.91 | 7.6 | 0.8 | 0.99 | 55 | 118 | 98 | 0 |
| C |  | 4nDJMtnz35oWDF4EZJ8bxEeuQAFsAit8kTMKsi9BtZ4s | 0 | 1 | 1.96 | 14.24 | 75 | 38.1 | -0.46 | 2548 | 906 | 2.81 | 233.6 | 17.4 | 0.24 | 49.7 | 58 | 60 | 8.7 |
| C |  | ssssswdk4RR8HqkE3uwUWzDbd6mXFTTPjcXBKNzQ57E | 15 | 1 | 0.68 | 61.13 | 75 | 37.8 | -2.95 | 38866 | 5818 | 6.68 | 340.9 | 19.9 | 0.22 | 55.7 | 75 | 70 | 10 |
| C |  | 5MTFCNHteKNV7YBxSSkb7Bdp2pDy6wsEduC5dtDb7FAE | 7 | 1 | 1.9 | 11.3 | 75 | 37.4 | -0.97 | 2038 | 737 | 2.77 | 209.8 | 18.5 | 0.24 | 50.4 | 59 | 58 | 9.3 |
| D |  | H6Q4HhDmGzh81pnXCbRswT2xtdLJGLTBtHLMjo29HciE | 6 | 2 | 0.1 | 10.14 | 62.5 | 46.8 | -3.24 | 24944 | 6354 | 3.93 | 226.8 | 5.8 | 0.4 | 58.6 | 145.5 | 92 | 4 |
| D |  | 86ugEi2Fo5oZCHVHc9MzMDk7Tht8bzni694qpk3G5t95 | 17 | 1 | 0.56 | 25.63 | 75 | 35.8 | -2.89 | 8365 | 2128 | 3.93 | 1381.7 | 23.1 | 0.54 | 50.7 | 11 | 35.3 | 40 |
| D |  | 6PU45i6inSAwhPQt3jEDZyJfHnMTf3QnvbiFURhCmi7d | 8 | 1 | 1.39 | 8.54 | 62.5 | 37.1 | -2.08 | 2172 | 729 | 2.98 | 220.1 | 18.8 | 0.24 | 50.3 | 52.5 | 59.3 | 8 |
| D |  | FHKhfxRcGcVpoFcPaGkEQTTbfrZWzRGAjKwdL54xTBwb | 10 | 1 | 0.95 | 6.04 | 62.5 | 36.3 | -1.64 | 2237 | 788 | 2.84 | 208.8 | 17.6 | 0.24 | 50.3 | 54.5 | 57.3 | 11.3 |
| D |  | 8eNTaNgz7fTrA8tkCxYnmv7U13h5RPJ35XodAUyjR3cX | 9 | 1 | 1.12 | 6.95 | 50 | 36.5 | -1.07 | 2199 | 778 | 2.83 | 230.4 | 18.6 | 0.24 | 49.9 | 62 | 60 | 10 |

---

## 6. The four studied wallets

| wallet | node | grade | return on spend | trades per token | entry age | hold | on a dip |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 3Xk2 | N4 | A | 3.03 % | 1.13 | 45 s | 49 s | 41 % |
| 8dtx | N4 | A | 2.65 % | 1.35 | 126 s | 17 s | 49 % |
| 64hP | N5 | A | 1.18 % | 3.04 | 107 s | 24 s | 82 % |
| omego | N5 | B | 0.76 % | 5.85 | 370 s | 15 s | 73 % |

All four are in the two thinnest-margin nodes. Their margins at **their own**
fill are smaller than the fill degradation a reactive entry pays, which is the
structural reason every clone of them fails: the logic has no room in it for a
second trader. 8dtx is not a heavy scalper in this window - 1.35 trades per
token, one buy each. 64hP and omego are the genuine repeat traders.

---

## 7. What this does not claim

The map ranks the margin each node runs on. It tests neither node for us. A
node becomes a rule only through the full-tape test: express the entry context
in tape terms, take **every** instance including those no roster wallet joined,
fill at `lag_115` with an adverse in-slot pick, charge both legs, freeze, then
a disjoint holdout. Levels here are measured on a population selected for being
profitable, so only the ranking between nodes carries.
