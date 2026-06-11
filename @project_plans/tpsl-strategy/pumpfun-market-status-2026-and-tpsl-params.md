# Pump.fun Market Status (Mid-2026) & TPSL Param Recommendations

> ⚠️ **Strategy/params superseded.** The current entry/exit strategy and starting param values
> now live in [`tpsl-scalp-continuation-plan.md`](tpsl-scalp-continuation-plan.md). This doc is
> kept for its **market data (§1–§2, §5)**, **Mayhem analysis**, and **sources**, which the
> scalp plan still cites. The §4 launch-sniper param table is historical — use the scalp plan's
> table instead.

> Snapshot date: **2026-06-08**. Companion to `pumpfun-sniper-strategy-research.md` (which set the
> 2025→early-2026 baseline). This doc updates that baseline to the **current** regime and converts it
> into concrete values for the **implemented** TPSL columns. Numbers vary by source/date — ranges and
> confidence are noted. Sources at the bottom.

---

## TL;DR

1. **Mayhem Mode is the single biggest change.** A pump.fun feature (live ~Apr 2026) where an autonomous
   AI agent **mints +1B tokens (2B total supply)** and **random-walk buys/sells the coin for its first
   24h**, then burns leftovers. It is **slightly net-sell biased**, produces **±300% swings in minutes**,
   and adoption has ramped to **~half of new launches** (~10k/day of ~20k). This is **manufactured noise,
   not demand** — pump.fun itself says there is "no rational strategy to mirror it."
2. **Tokens die faster and in greater proportion than the 2025 baseline.** ~95% daily turnover, ~98.6%
   end as rugs/scams, median time-to-death ~25 min, liquidity drain in **30–90s**.
3. **The crowd left.** Daily new users −82% (183k → 33k), volume ~$20B → <$3B, pump.fun revenue −80%.
   Thinner books → **worse exit slippage** → size down and lean on the liquidity-death exit.
4. **Devs got smarter.** Bundlers fake "organic" 24/7 volume and split holdings across 40+ wallets, so
   **distinct-buyer-count and volume signals are dead**. Slot-concentration + retained-supply still work.
5. **Net effect on params:** widen stops (chop is worse), bank profit earlier (net-sell drift + 92% dump),
   shorten max-hold, make the **reserve-drop exit the primary rug-catcher**, and **exclude Mayhem tokens
   outright** (done — §4a). Final values in §4.

---

## 1. Market state — the numbers

| Metric | 2025 peak | Mid-2026 (now) | Δ |
|---|---|---|---|
| Tokens created / day | ~30k+ | **~17–20k** (user-reported ~20k; sources 17.3k–30k) | ↓ |
| Tokens **defunct** / day | — | **~9.9k** (≈95% daily turnover) | ↑ |
| **Mayhem-Mode** launches / day | 0 (didn't exist) | **>10k** (~half of launches; up from +500 in launch week) | ↑↑↑ |
| Graduation rate | ~2% | **0.6–1.15%** (recent uptick to a 6-month high after cashback/fee changes) | ↓ |
| Rug / scam end-state | ~high | **98.6%** (Solidus Labs) | ↑ |
| Median time-to-death | ~2h (2024) → ~25 min | **~25 min; 15% die day-1, 31% within a week** | ↓ |
| Liquidity drain once dev pulls | 30–90s | **30–90s** (unchanged — still seconds) | = |
| Daily **new** users | 183,189 (Jan 2025) | **33,275** (−82%) | ↓ |
| Recurring users | ~258k | **~66k** (−74%) | ↓ |
| Memecoin volume | ~$20B (mid-2025) | **<$3B** (Dec 2025) | ↓ |
| Traders in profit | — | **56.8% (Feb) → 70% (Mar) → 73.3% (Apr) 2026** — modest recovery | ↑ |
| Same-block sniping | dominant | **still dominant** — ~15k tokens/mo sniped by deployer-funded wallets, 4,600+ sniper wallets, ~87% win | = / ↑ |

**Example day (representative, ~mid-2026):** of **~20,000** launches, **~10,000** opt into Mayhem Mode,
**~9,900** are effectively dead within 24h, **~120–230** graduate (0.6–1.15%), and **>50%** are sniped in
the creation block by deployer-funded wallets. A $50 buy on a freshly-graduated thin token moves price
**30–40%**.

---

## 2. What changed since the last research pass

### 2a. Mayhem Mode — the new dominant token type *(highest impact)*
- **Mechanics:** opt-in at creation only (immutable). AI agent mints **+1B → 2B total supply**, then for
  **24h** does a random walk: buys and sells with ~equal probability, but **sells average slightly larger
  → structural net-sell drift**. After 24h, unsold/agent tokens are **burned**.
- **On-chain footprint:** launched via **`create_v2`** (not legacy `create`), bonding-curve account owned
  by **Token-2022** (not legacy SPL Token), distinct fee recipients, IDL flag `is_mayhem_mode` /
  `mayhem_state` / `set_mayhem_virtual_params`. → **trivially detectable** at ingest.
- **Trading reality:** ±300% intraday swings without news; Reddit reports of −70–80% in the first day from
  slippage/path-dependence. pump.fun explicitly warns there is **no rational strategy to mirror the bot**.
- **Why it matters to *this* bot:**
  - The agent **is** the "volume" and the "buyers" — so volume/liquidity wash guards and "organic
    continuation" signals are polluted by design on Mayhem tokens.
  - **Net-sell drift** means *holding longer is structurally worse* than on legacy tokens.
  - **2B supply** breaks every `÷ TOKEN_TOTAL_SUPPLY` calc (see §5.1) by 2×.
  - The chop will **whipsaw tight stops** out before any real move.

### 2b. Faster death, higher mortality
Median time-to-death ~25 min; 15% die on day 1; drain is 30–90s. The old sim's "5-day average win" was
scoring AMM survivors, not the sniper edge. Everything lives in the **first seconds-to-minutes**.

### 2c. Fewer traders, thinner liquidity
Users −82%, volume −85%, revenue −80%. Thin books mean **your own exit moves price against you** — the
liquidity-death exit may not even fill at the quoted price. Size positions down.

### 2d. Smarter devs / bundling escalation
Bundlers now run **randomized buy/sell cycles to fake organic 24/7 volume** and **split a 40% position
across 40 wallets** to look like healthy distribution. → buyer-count and volume are noise; the durable
tells remain **slot concentration + retained supply + genuine later/outside buying**.

### 2e. Fee/incentive changes (real-mode realism)
Creator-fee overhaul + cashback in 2026 (Dynamic Fees V1 was "too easy to launch, too hard to trade").
Fixed costs persist: **0.02 SOL creation fee, Jito tip per bundle, ~0.000005 SOL/sig**, plus priority fees.
Sniping is still a fee auction (>50% buy in block 0). Model these in paper or paper will lie.

---

## 3. Implications for the TPSL strategy

- **Reserve-drop (liquidity-death) exit is now the *primary* exit**, not a backstop. It's the only thing
  that reacts inside the 30–90s drain that price stops miss.
- **Widen price stops.** Normal 50–500% swings + Mayhem ±300% noise chop out anything tight.
- **Bank profit earlier.** 92% of tokens with ≥30 swaps dump; Mayhem adds net-sell drift. A realized +60%
  beats a paper +300% that round-trips to zero.
- **Shorten max-hold.** ~25-min median death; cut undecided positions in minutes.
- **Exclude Mayhem tokens** — manufactured chaos, no snipeable edge (done — §4a).
- **Don't trust volume/buyer-count entry signals** — they're manufactured. (Affects the not-yet-built
  N4/N6/N9 filters more than the current exit columns.)

---

## 4. Recommended param values  *(decisions locked 2026-06-08)*

**Locked policy:** legacy-only (Mayhem **excluded**), trade mode **both paper + real**,
`buy_amount = 0.05 SOL`. Data **does** contain Mayhem tokens, so exclusion must be enforced (see §4a).

These map to the **currently-implemented** columns (`strategy_tpsl_rule.rs`). Exit cascade priority is
already correct: **LiquidityExit → StopLoss → TakeProfit → TrailingStop → Stall → TimeStop**.

| Param (column) | **Value (legacy-only)** | Why |
|---|---|---|
| `p_liquidity_drop_pct` (E4) | **30%** | Drain is 30–90s; this is now the *primary* rug-catcher, reacts inside the drain window price stops miss. |
| `stop_loss` | **40%** | Survive normal 50–500% chop without being whipsawed out. |
| `take_profit` | **60%** | Bank the common first leg (92% of ≥30-swap tokens dump). Raise to ~150% only if you want to chase runners and lean on the trailing stop. |
| `p_trailing_stop_pct` (E1) | **25%** | Tight trails get whipsawed by normal meme swings. |
| `p_stall_secs` (E3) | **60s** | "No new higher-high for 60s = dead." Pumped-then-flat rarely re-pumps. |
| `p_time_stop_secs` (E2) | **480s (8 min)** | ~25-min median death; cut the undecided. Aggressive variant: 300s. |
| `buy_amount` (SOL) | **0.05** *(locked)* | Tiny vs. an early curve's ~30 SOL reserves (~0.15% impact) → slippage negligible; safe for real-mode testing. |
| `p_max_concurrent_tokens` | **5–10** | Spread risk across the ~98% rug base; no single position can sink the run. |
| `p_max_total_tokens` | bankroll ÷ 0.05, with margin *(needs bankroll)* | Caps total run exposure. |
| `p_initial_buy_sol` (+ `tolerance_pct`) | set to **your bot's launch fingerprint** (band + 10–20% tol) | **Required:** `token_matches_rule` returns `false` if *no* positive fingerprint filter is set → zero matches. Pick one of `p_initial_buy_sol` / `p_cu_limit` / `p_cu_price` / `p_max_sol_cost` / `p_spendable_sol_in`. The real dev-loading reject is block-0 %-of-supply (planned N3), not raw initial-buy SOL. |

### 4a. Mayhem exclusion — shipped

Config alone couldn't do it: `token_matches_rule` never read `is_mayhem_mode`, and the `p_ix_labels`
filter only checks labels are non-empty (not content), so `p_ix_labels=["Create"]` wouldn't exclude
`"Create_v2"`/Mayhem. Fixed with a one-line filter in the live sim
([`simulation_tpsl.rs:326`](../../backend/src/strategies/tpsl_sniper_1/simulation_tpsl.rs#L326)):
`.filter(|t| !t.is_mayhem_mode && token_matches_rule(t, &rule))`. Mayhem tokens are now excluded from
every backtest. (If real-mode trading later needs the same gate as a toggle, promote it to a
`p_exclude_mayhem: bool` param via the N1 `p_exclude_rugged` plumbing pattern.)

---

## 5. Code / data caveats (verified in this repo)

1. **`TOKEN_TOTAL_SUPPLY` is hardcoded to 1B** — `backend/src/config/constants.rs:146`
   (`1_000_000_000_000_000` = 1e9 × 1e6). **Mayhem tokens have 2B total supply.** Consequences:
   - Market cap (`TOKEN_TOTAL_SUPPLY × price`) is **2× too high** for Mayhem tokens
     (`token_cache.rs:115`, `trade_repo.rs:304`, and the frontend chart's `TOKEN_TOTAL_SUPPLY`).
   - Planned supply-% entry filters **N3 `p_max_dev_block0_pct`, N4 `p_max_first_slot_bundle_pct`,
     N5 `p_max_bundle_held_pct`** divide by `TOKEN_TOTAL_SUPPLY`. On Mayhem tokens the computed % will be
     **2× the true %**, → **over-rejecting** them. **Fix:** derive supply per-token (the IDL exposes mint
     supply / `set_mayhem_virtual_params`) instead of using the constant.
2. **Paper realism still pending** (per the strategy doc's "Realism pass"): slippage vs. reserves,
   priority/Jito fees, fill latency, −100% rug floor. Without these, the §4 numbers will look better in
   paper than they trade.

---

## 6. Still to finalize

- **Run bankroll** — needed to set `p_max_total_tokens` (= bankroll ÷ 0.05, with margin).
- **Which fingerprint the sniper selects on** (`p_initial_buy_sol` / `p_cu_*` / `p_max_sol_cost` /
  `p_spendable_sol_in`) + its band — required for any token to match (see §4).
- **Validate on recent (post-Apr-2026) data**, then land the §5.2 realism pass before trusting real-mode PnL.

---

## Sources

- [Every 24 Hours on Pump.fun, 10,417 Tokens Are Launched while 9,912 Become Defunct (ChainPlay)](https://chainplay.gg/blog/lifespan-pump-fun-memecoins-analysis/) — turnover, 12-day avg lifespan, 15% die day-1 / 31% in a week, 98% <3 months.
- [Mayhem Mode (pump.fun docs)](https://pump.fun/docs/mayhem-mode) & [Mayhem Mode SDK doc (GitHub)](https://github.com/nirholas/pump-fun-sdk/blob/main/docs/mayhem-mode.md) — +1B→2B supply, 24h window, burn, Token-2022, immutable-at-creation.
- [Full Mayhem Mode support for Pump.fun (Chainstack)](https://chainstack.com/trading-bot-update-full-mayhem-mode-support-for-pump-fun/) — `create`/`create_v2` detection, Token-2022 bonding-curve account, fee recipients, 2B supply.
- [Pump.fun launches Mayhem Mode (Cryptopolitan)](https://www.cryptopolitan.com/pump-fun-launches-mayhem-mode-letting-ai-agents-loose-in-the-trenches/) & [BTCC/Cryptopolitan](https://www.btcc.com/en-US/square/Cryptopolitan/1172048) — net-sell bias, ±300% swings, −70–80% reports, "no rational strategy to mirror."
- [Mayhem Mode slow first week (Cryptopolitan)](https://www.cryptopolitan.com/pump-fun-mayhem-mode-slow-first-week/) — daily launches 17.3k → 17.8k in week 1 (adoption baseline).
- [Daily registrations plunge 183k → 33k (MEXC)](https://www.mexc.com/learn/article/report-daily-pump-fun-registrations-plunge-from-183k-to-33k-as-memecoin-frenzy-fades/1) — −82% new users, −74% recurring.
- [80% Less Revenue for Pump.fun (Cointribune)](https://www.cointribune.com/en/crypto-80-less-revenue-for-pump-fun-traders-are-worried/) & [Pump.fun Traders Are Making a Comeback (CoinGecko)](https://www.coingecko.com/research/publications/pump-fun-traders-are-making-a-comeback) — volume <$3B, profit-share recovery 56.8%→73.3%.
- [Graduating tokens break to 1.15% (Cryptopolitan)](https://www.cryptopolitan.com/pump-fun-graduating-tokens-break-to-1-15-of-new-launches/) & [Graduations six-month high (Cryptopolitan)](https://www.cryptopolitan.com/pump-fun-token-graduations-six-month-high/) — graduation 0.6–1.15%.
- [Pump.fun 2026 Outlook: 98.6% Rug-Pull Problem (Coinmonks/Medium)](https://medium.com/coinmonks/pump-fun-2026-outlook-revenue-lawsuit-risks-token-unlocks-and-the-98-6-rug-pull-problem-0a85252c5da2) — 98.6% scam end-state.
- [Why Pump.fun Tokens Crash So Fast (Yellow)](https://yellow.com/learn/pump-fun-token-crash-explained) — ~25 min time-to-rug, 30–90s drain, 10–30 min death window, post-graduation minute-by-minute.
- [Liquidity Sniping Bot: Inside Job (Bitget)](https://www.bitget.com/news/detail/12560604803448) & [How to Bundle on Pump.fun 2026 (SolBundler)](https://solbundler.app/blog/how-to-bundle-pump-fun) — 15k tokens/mo sniped, 4,600 wallets, deployer-funded ~87% win, fake-organic volume, 40-wallet splitting.
- [Exact Cost to Launch on Pump.fun 2026 (SolBundler)](https://solbundler.app/blog/exact-cost-to-launch-pump-fun-2026) & [Pump.fun fees (docs)](https://pump.fun/docs/fees) — 0.02 SOL creation fee, Jito tip per bundle, priority-fee context.
- [Pump.fun overhauls creator fees (The Block)](https://www.theblock.co/post/384975/pump-fun-overhauls-creator-fees-token-launches-highest-daily-september) — Dynamic Fees V1 retired; trader-incentive shift.
