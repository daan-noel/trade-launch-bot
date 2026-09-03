# Market model and workflow (rebuilt from scratch)

**STATUS: DRAFT - pending operator corrections.** This is the foundation rebuilt
2026-09-03 from wide research and the operator's causal framing, deliberately NOT bound
to any previously derived rule or result. Only market physics and measurement honesty
carry over. The pure theses (section B) govern everything downstream; correct those
first.

---

## A. What this market is (the machine)

### The platform layer is an attention market

Discovery feeds (pump.fun trending, Axiom Pulse, GMGN, etc.) rank tokens by
**transaction count, recency, unique buyers, and volume**. A single transaction moves a
token to the top of "recently traded" instantly. Attention is therefore **purchasable
with transactions** - and attention converts to retail flow.

The fee system makes this a real business. Creator fees are dynamic (since January
2026): **0.05-0.95% per trade, with the maximum 0.95% paid in the $88k-$300k market cap
band** - just past graduation (~$69k / ~85 real SOL). Fees are shareable across up to 10
wallets and assignable to Community-Takeover admins. The dev's paycheck is: **get the
token to graduation, keep volume churning in the band just past it.** Graduation is not
a milestone - it is where the dev's revenue rate peaks.

### The dev is an entrepreneur running a repeatable operation

Standard equipment:

- **Bundlers**: launch with up to 16 extra wallets pre-buying supply.
- **Volume/bump bots**: multi-sub-wallet; **bump mode** = many tiny frequent buys to
  game frequency-ranked feeds; **volume mode** = fewer larger buys to game
  volume-ranked feeds.
- Fee/CU presets, wallet rotation, fee-sharing plumbing.

8,000-15,000 tokens launch daily; under 2% graduate; ~99% of launches show
pump-and-dump or rug-consistent patterns. Devs rotate wallets constantly - **identity
lives in the machinery, never the wallet.**

Academic backing (arXiv "A Midsummer Meme's Dream"): wash trading is the most common
manipulation (74.8% of flagged cases), run by **tiny coordinated groups (median ~2.8
actors, half by a single actor), recurring on the same token ~3.6 times**. Critical:
**62.9% of extraction events (pump-and-dump / rug) follow visibility-building
operations (wash trading / cheap price inflation) on the same token.** Manipulation is
*staged*: build attention first, extract second.

### The bot layers

| layer | role |
| --- | --- |
| Snipers | 2-5 ms infrastructure, multi-relay, dynamic Jito tipping; race launches and known triggers |
| Copy/tracker bots | follow labeled "smart money" wallets (GMGN/Axiom leaderboards); amplify whatever tracked wallets do |
| Volume bots | the dev's instrument (above); 60-80% of all volume is bots |
| Smart traders / harvesters | the small daily-profitable population; they read the machine |
| Retail | arrives via feeds, KOLs, livestreams; the buyer everyone else sells to |

### The physics (immovable)

- Bonding curve: `price = vsol^2 / k`. Any observable flow **has already moved the
  price by the amount observed**. Watching flow tells you the past, never the future.
- Costs at our size: 1.25% fee per leg + own impact `B/vsol` per leg => **~3%+ round
  trip**. Every idea must beat this.
- Slot ordering: transactions in a slot (~400 ms) confirm together; order inside is a
  priority-fee / Jito-tip auction (95%+ of stake runs Jito; tips are 60%+ of priority
  volume). **Latency cannot buy position inside a slot** - only tip auctions can, and
  that is a bidding war, not an edge.
- Our seat: ~100 ms reaction; we land reliably in the next slot. Profit that lives
  inside one slot is unreachable; anything playing out over minutes is fully reachable.

---

## B. The pure theses (the logic under the hood)

- **T1 - Nearly every price move is manufactured.** Someone pays for it, with a goal
  and a budget. The tape is a record of *campaigns*, not opinions. Reading the tape
  means asking: who is paying for this and what do they want?
- **T2 - The core business loop:** transactions -> feed placement -> retail attention
  -> volume -> creator fees + supply sales. The dev is the entrepreneur of this loop;
  everyone else services it (volume bots), parasitizes it (snipers, harvesters), or
  funds it (retail).
- **T3 - Exploitation is staged: visibility first, extraction second.** A token's life
  is a sequence of *phases* (launch -> scramble -> stall -> campaign / dump /
  abandonment -> ...). The money is at the **phase transitions**, because that is when
  future flow becomes predictable.
- **T4 - The dev's decision points originate every campaign, and each decision becomes
  visible as a characteristic transaction.** A stall (gap, no prints) is a decision
  node: abandon, dump, or spend on a campaign. The dev acts through tools, and every
  tool has a fingerprint (instruction structure, fee/CU preset, funding pattern). So
  *what breaks a silence* is readable information about *what was decided*. The gap
  alone means nothing - most gaps precede death; the signature that ends it means
  everything.
- **T5 - Actors are their machinery.** Wallets rotate daily; instruction structures,
  presets, and funding graphs persist. All identification - devs, tools, bot layers -
  is built on machinery. (Hard rule: never build a factor on wallet identity.)
- **T6 - Profitable traders are readers of this machine, and their logic is
  derivable.** They recognize a campaign's start earlier than the crowd and exit into
  the crowd's arrival. Their decision procedures consume only public data, so they can
  be reconstructed. A failed reconstruction indicts the hypothesis space, never the
  existence of the logic.
- **T7 - On a curve, profit can only come from anticipating FUTURE flow, and future
  flow only comes from someone's PLAN** - the dev's remaining budget and intent, the
  feed's mechanical response, retail's arrival. Every rule must be a claim about an
  actor's plan ("the dev has decided to push this to graduation"), never a claim about
  the price path, which is already-spent information.
- **T8 - Our seat monetizes campaigns, not pops.** Multi-slot, minutes-long moves with
  a convex tail. The book shape that follows: win rate well under 50%, negative median,
  the tail pays. The exit must never cap the tail, and must abandon fast when
  follow-through fails.
- **T9 - Everything rots.** Fee rules change, tools update, launchers rotate, metas
  turn over in weeks. No rule is an asset. **The pipeline that re-derives rules from
  the actor model is the asset.** Continuous monitoring of the machine layer is part of
  the system, not maintenance.
- **T10 - Honesty laws** (measurement discipline, not market ideas): score on the full
  token universe, never a curated list; price fills at our real reaction on **both**
  legs; charge full costs; rank on total net SOL; freeze a rule before testing it on
  untouched time; verify any offline result trade-by-trade against the engine before
  believing it; treat every feature as guilty of look-ahead until audited.

---

## C. The workflow

```
            [PHYSICS + HONESTY LAWS]  -- fixed once, applies to every phase
                       |
                       v
   PHASE 1: CENSUS OF MACHINES                    (continuous, refreshed weekly)
     classify every tx by build fingerprint (tool census)
     cluster wallets by funding graph (dev clusters)
     label the layers: dev tools / volume bots / snipers /
                       copy bots / terminals / human
                       |
                       v
   PHASE 2: DEV BEHAVIOR MODEL                    (continuous)
     reconstruct token lives as PHASE SEQUENCES
     measure the decision node:
       P(campaign | gap broken by tool X, count N, size S)
       campaign follow-through: duration, flow, graduation
       budget patterns, recurrence of the same machinery
                       |
                       v
   PHASE 3: TRADER STUDIES (thermometer only)
     take daily-profitable, non-proxy wallets
     describe each as a READER: which phase transitions
     does he buy, on which signatures, what does he skip
     state his logic as one causal sentence; verify it
                       |
                       v
   PHASE 4: HYPOTHESIS -> RULE
     write the causal story FIRST: who acts, why now,
     why will flow CONTINUE after our entry
     then express it in machine terms:
       Door        (creation-time facts: launch machinery)
       Event       (the decision-point signature, completing print)
       Permissions (state true at the event: age, depth, phase)
       Exit        (from the story: harvest the campaign,
                    abandon fast on failed follow-through)
                       |
                       v
   PHASE 5: HONEST MONEY
     full tape, real fill both legs, full costs,
     total net SOL + days-positive, frozen rule on
     untouched blocks + a random-entry null
     red -> the STORY is wrong -> back to Phase 2/4
     (never patch the rule to fix a red block)
                       |
                       v
   PHASE 6: ENGINE + PER-TRADE RECONCILIATION
     implement in metrics that say EXACTLY the finding;
     offline book vs engine book agree trade-by-trade
                       |
                       v
   PHASE 7: PAPER -> SMALL REAL
     live evidence at the real seat
                       |
                       +-----> results feed back into PHASE 2:
                               the dev model is a living document
```

### The load-bearing differences from the old approach

1. **Hypotheses come from the actor model, not from feature enumeration.** Phase 4 is
   forbidden from starting until it can say the story out loud: *"the dev of this kind
   of launch, at this kind of stall, spends through this tool to restart the attention
   loop, because his fee income peaks past graduation - so flow continues after our
   entry."* No story, no rule.
2. **Phases 1-2 are a permanent observatory, not a one-time study.** The census and the
   dev model refresh on a schedule; rule health is interpreted against them ("the rule
   died because that tool updated" is a diagnosis feature-search can never make).
3. **Trader studies are instruments, not targets.** They calibrate the dev model and
   suggest which signatures matter. The rule is always about the dev's decision, never
   about the trader. (Racer signatures - e.g. builds carrying CreateAccountWithSeed -
   mark that someone else's trigger fired: confirmation evidence, never the entry.)
4. **Failure is informative upward.** A red money result refutes a *story about the
   dev*, which improves Phase 2. A refuted feature combination teaches nothing.
5. **The purpose is riding, not racing.** Enter at the campaign's visible start
   (tolerating a slot of lag by design), take profit into the campaign's success -
   retail arrival / the graduation band - or abandon fast when follow-through fails.

### Phase semantics pinned by the operator

- "no tx + 2 Axiom" / "no tx + 4 Terminal" style signatures: the count is the number
  of buys from that tool breaking the gap. The examples stand for all specific
  tools/routers; the actual (gap, tool, count, size) relationships are unmeasured and
  are the first thing Phases 1-2 must produce.
- First-print vs completing-print firing, and dev-funded vs third-party volume
  services: open questions to MEASURE, never assume.

---

## D. Sources

- [pump.fun fee docs](https://pump.fun/docs/fees)
- [CoinMarketCap - $2M creator payouts on day one of the new fee model](https://coinmarketcap.com/academy/article/pumpfun-creators-earn-dollar2m-in-first-day-under-new-fee-structure)
- [Yahoo Finance - creator fee model overhaul](https://finance.yahoo.com/news/pump-fun-fee-model-hands-125849600.html)
- [Medium - Project Ascend creator earnings](https://medium.com/coinmonks/pump-fun-new-revenue-plan-how-project-ascend-is-boosting-creator-earnings-in-the-memecoin-world-32901d90f4ac)
- [arXiv - A Midsummer Meme's Dream: market manipulations in the meme coin ecosystem](https://arxiv.org/html/2507.01963v1)
- [arXiv - Predicting the success of new crypto-tokens: the Pump.fun case](https://arxiv.org/abs/2602.14860)
- [Bubblemaps - insider cluster detection](https://blog.bubblemaps.io/how-to-analyze-meme-coin-holders-with-bubblemaps/)
- [Axiompedia - Pulse filters](https://axiompedia.com/guides/trading/axiom-pulse-explained)
- [Smithii - pump.fun bundler (16 wallets)](https://smithii.io/en/pump-fun-bundler-bot/)
- [OpenLiquid - volume bot modes](https://openliquid.io/tools/pump-fun-volume-bot/)
- [jumpbit - bump bot mechanics](https://jumpbit.io/en/solana/pumpfun-tools/pumpfun-bump-bot)
- [RPC Fast - sniper infrastructure](https://rpcfast.com/blog/how-to-launches-snipe-pump)
- [Chorus One - do priority fees / Jito tips land transactions faster](https://chorus.one/reports-research/transaction-latency-on-solana-do-swqos-priority-fees-and-jito-tips-make-your-transactions-land-faster)
- [Chainstack - Jito bundles and tips](https://chainstack.com/jito-explained-bundles-tips-mev-solana/)
- [Cryptopolitan - graduation rate](https://www.cryptopolitan.com/pump-fun-graduating-tokens-break-to-1-15-of-new-launches/)
- [AssureDefi - rug / pump-and-dump prevalence](https://www.assuredefi.com/blog/meme-coin-rug-pulls-pump-dumps-how-to-spot-and-prevent-fraud)
