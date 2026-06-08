# Pump.fun Launch-Sniper TPSL — Entry/Exit Strategy (research-grounded)

> Scope: improve **which tokens to buy** and the **entry/exit logic** for a launch/sniper-style TPSL, for both paper and real modes. "Force-close at end of data" is dropped (a backtest-DB artifact, not a trading rule). Kept and recalibrated: **max-hold, trailing-peak, momentum-death**.

## The market reality (2025–2026) — this rewrites the timescale

- **~0.6% of tokens graduate**; ~98.6% show rug-pull behavior. Median time-to-graduation **4.4 min**, median ~457 trades.
- **Time-to-death is minutes, not days.** Average time-to-rug fell from ~2h to **under ~25 min**; *nearly all* rugs happen **10–30 min after launch**, and once a dev pulls, **liquidity drains in 30–90 seconds**.
- **>50% of tokens are sniped in the creation block.** Same-block sniping is now the dominant model, not an edge case. Deployer-funded snipers run ~**87% win rate**.
- **Strongest success predictor (academic, not blog): speed of liquidity accumulation with FEW trades.** Tokens that reach a given vSOL in **≤10 trades** dramatically outperform those needing 100–1000+. Fast accumulation = "the single most informative predictor."
- **Buyer composition matters:** >70% bot-attributed trades → systematically lower graduation; **≥30% non-bot (distinct human) participation → higher**.
- **92.2% of tokens with ≥30 swaps suffer ≥1 dump.** Assume a dump is coming; bank profits early.

**Implication for your bot:** the whole game lives in the **first seconds-to-minutes**. A 5-day "average win" in the old sim means the exit model was scoring AMM survivors, not the sniper edge. Recalibrate every time window down ~100×: holds in **minutes**, trailing/liquidity reactions in **seconds**.

## Which tokens to enter (entry selection)

Don't pure-snipe block 0 blindly — that makes you the dev/sniper exit liquidity. Use a **2-stage gate**: cheap creation-tx safety first, then a short **confirmation window** (the academic edge). All of this is computable from your existing schema (`trades`: `wallet_address, slot, block_time, token_amount, virtual_sol_reserves`; `tokens.creator_wallet`, `initial_buy_sol`; `net_token_amount_by_wallet_and_mint`).

**Stage 1 — reject at creation (hard filters):**

- **Dev block-0 buy share too high** → reject. Practitioner line: creator buying **>10% of supply in block 0**, or **dev+linked wallets holding >5%**, means you're exit liquidity.
- **Bundle/insider concentration** → reject if the first 1–3 slots are dominated by few wallets capturing a large share of tokens bought (coordinated bundle).
- Keep your existing creation-fingerprint filters (init buy size, CU limit/price, labels) as the bot signature.

**Stage 2 — confirm in a short window (e.g. first ~3–15s / first N slots) before committing, or scoring it post-hoc in the sim:**

> ⚠️ **Do NOT use "distinct buyer count" as a quality signal — it's defeated by bundlers.** A Jito bundle lands the mint + up to ~25 wallets' buys atomically in the **same creation slot** so outside snipers can't wedge in; splitting across many fresh wallets makes it *look* diverse. The bundle's one unavoidable tell is the **slot**: to beat public snipers it must buy in slot 0, so more bundle wallets = **more same-slot concentration**, not more diversity. Measure *when + how much still held*, not *who* (this is exactly how Trench.bot's bundle checker works: a per-slot viewer of wallets/SOL/retained supply; "3+ wallets in one slot still holding = bundle").

- **First-slot bundle share (primary):** tokens bought by *all* wallets in the creation slot (and first 1–2 slots) ÷ curve-sold supply. High = you're exit liquidity → **skip**. (Computable from `trades.slot` + `token_amount`.)
- **Bundle retained supply:** of that launch-slot cohort, how much is **still held** at entry time (net buys−sells per wallet — reuse `net_token_amount_by_wallet_and_mint`). Large unsold overhang = **skip**.
- **Organic continuation (the real demand signal):** net buying in **later** slots from wallets **absent** at launch, with the curve still climbing *while* the bundle distributes. This replaces "launch-window head-count."
- **Liquidity-accumulation velocity:** `virtual_sol_reserves` climbing fast with **few trades** (high ΔvSOL per trade/second) — the single strongest academic predictor.
- **Liquidity floor at entry:** `virtual_sol_reserves ≥ min` (exit price impact is brutal below this).
- **Wash-trade guard:** high `volume` on tiny liquidity (volume/liquidity above a cap) = manufactured → skip.
- **Dev / creator hasn't dumped:** creator net balance not already falling; no creator sell in the window.
- **Creator reputation:** has this `creator_wallet` (or its prior mints) rugged before? Derive from tokens grouped by `creator_wallet` + `is_rugged`. Devs spin up new *buyer* wallets cheaply, but a known-rugger or zero-history creator is still a usable prior.

> **Limit (be honest):** proving two wallets are the same person needs the **funding graph** (who sent them SOL) — that is **not in the trade DB**, and it's defeatable anyway (devs fund via CEX/mixer, so even Bubblemaps misses it). Don't chase wallet identity; use slot-concentration + retained-supply + later organic demand, which don't require it.

> Stable default: **confirmation-snipe** (Stage 2 on) beats pure block-0 snipe on win rate at a small cost in top-end upside — the right trade for "efficient & stable." Entry caps your bundle *overhang*; the fast exits handle what entry can't clear.

## Entry/exit logic (cascade — first trigger wins)

Single entry → single exit (scale-out is a later upgrade). Walk post-entry trades chronologically; recalibrated to the minutes/seconds reality.

**Exit cascade, by priority:**

1. **Liquidity-death / rug exit (fastest, most important):** `virtual_sol_reserves` drops **≥ ~30–40% off its peak within a short window** → exit immediately at whatever fill. This is the real killer (drain = 30–90s), so it must react in seconds, not on a price candle.
2. **Hard stop-loss:** e.g. **−30% to −50%** from entry.
3. **Take-profit / scale target:** memes rarely hit fixed +500%; bank early. Target **+50% to +2–3×**; ideally **scale out** (e.g. sell ~50% at +50–100%, let a runner ride under a trailing stop). 92% dump → taking the first leg is most of the edge.
4. **Trailing stop (the core meme exit):** track peak since entry, exit on a **~20–30% drop off peak** (wide — tight stops get whipsawed by normal 50–500% swings).
5. **Momentum-death / stall:** no new higher-high (or no trade) for **N seconds–minutes** → sell into the flatline; pumped-then-flat tokens almost never re-pump.
6. **Time stop / max-hold:** if unresolved within **~5–15 minutes** (NOT 24–48h) → exit at current price. Given ~25-min median death, holding longer just donates to the rug.

**Tunable as new rule columns:** `p_liquidity_drop_pct`, `p_trailing_stop_pct`, `p_stall_secs`, `p_time_stop_secs`, plus entry gates `p_min_liquidity_sol`, `p_max_first_slot_bundle_pct`, `p_max_bundle_held_pct`, `p_min_organic_sol`, `p_min_vsol_velocity`, `p_max_dev_block0_pct`, `p_max_volume_per_liq`, `p_exclude_rugged`, `p_skip_rugged_creator`. 0/NULL = disabled, matching the existing `ignore_zero_*` convention.

## Paper vs real (don't let paper lie to you)

Paper test must model the frictions real mode pays, or it will look far too good:

- **Slippage / price impact** on entry and (especially) exit — size vs `virtual_sol_reserves`. Thin liquidity = you move the price against yourself.
- **Priority fee / Jito tip** per trade (sniping is a fee auction; >50% buy in block 0).
- **Latency:** real fills land 1+ slots after the signal; model entry at a slightly worse price than the trigger trade.
- **Rug realism:** a rugged "open" position is **−100%**, not free — bake that into PnL.
- **Sell-side reality:** the liquidity-death exit may not fill; cap recoverable value when reserves have collapsed.

Real mode adds: RPC/shred latency budget, max RPS limits, retry/backoff, and a kill-switch on repeated failed sells.

## Highest-leverage, lowest-risk shortlist (do these first)

1. **Recalibrate all time windows to minutes/seconds** (time stop ~5–15 min, stall in seconds–minutes). Biggest single fix.
2. **Liquidity-velocity reserve-drop exit** — catches the 30–90s drain that price-based stops miss.
3. **Wide trailing stop (20–30%)** + **early scale-out** — converts "never +500%, never −80%" ghosts into banked wins.
4. **Confirmation-snipe entry gate**: **first-slot bundle share + bundle retained supply + later organic buying** + vSOL velocity + dev-not-dumping (NOT distinct-buyer count — bundlers fake that). Directly buys the academic edge and caps bundle overhang.
5. **Model slippage + priority fees + −100% rugs in paper** so paper ≈ real.

## Sources

- [Predicting the success of new crypto-tokens: the Pump.fun case (arXiv)](https://arxiv.org/html/2602.14860v1) — fast-accumulation/few-trades predictor, bot-dominance, 0.63% graduation, 92.22% dump rate.
- [Pump.fun MemeCoins Face Mass Extinction – <1% Survive (BitcoinKE)](https://bitcoinke.io/2025/03/pump-fun-memecoins-survival-rate/)
- [Exit the Liquidity Machine: Internal Sniping Arbitrage of Pumpfun (ChainCatcher)](https://www.chaincatcher.com/en/article/2185070) — >50% sniped in creation block, ~87% sniper win rate, supply concentration.
- [Liquidity Sniping Bot: the Inside Job behind Pump.fun launches (Bitget)](https://www.bitget.com/news/detail/12560604803448) — deployer→sniper funding, 15k SOL profits, 4,600 sniper wallets.
- [Pump.fun Strategy 2026: find gems & avoid rugs (Flashift)](https://flashift.app/blog/how-to-spot-the-next-viral-meme-coin-on-pump-fun-safely/) — dev >5% / block-0 >10% reject, bonding-curve confirmation, exit 2–3×.
- [Why Pump.fun Tokens Crash So Fast After Launch (Yellow)](https://yellow.com/learn/pump-fun-token-crash-explained) — time-to-rug ~25 min, drain 30–90s, 10–30 min death window.
- [Trench Radar Bundle Scanner / Bubblemap Bundle Viewer (docs)](https://docs.trench.bot/bundle-tools/bundle-scanner-guide) — per-slot bundle detection, retained-supply heuristic, funding-graph blind spot.
- [chainstacklabs/pumpfun-bonkfun-bot (GitHub)](https://github.com/chainstacklabs/pumpfun-bonkfun-bot) — practical listener/curve-state/TP-SL bot reference.
- [Pump.fun adopts dynamic fee model (Blockworks)](https://blockworks.com/news/pumpdotfun-fee-model) & [Pump.fun fees docs](https://pump.fun/docs/fees) — PumpSwap dynamic creator fees (fee/latency context for real mode).
