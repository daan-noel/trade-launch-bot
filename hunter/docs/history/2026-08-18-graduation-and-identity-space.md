# Graduation runs and the identity layer (2026-08-18)

Full universe 08-01..08-16, 345,869 mints, 30.4M trades, 689,157 wallets. Honest fills
(`mf.pfirst` of the next printing slot, +1 slot on both legs), 3.3% round trip, walk-forward
identity rosters (built 08-01..08-08, tested 08-09..08-16).

This session abandoned the bucket-average method that produced the previous 30 refutations.
The motivating observation: with 78% of P&L in 0.7% of trades, a negative bucket mean proves
only "do not buy this whole bucket" - it cannot see an edge that lives in 0.1% of moments.
So the search targeted **structural events** and **participant identity** instead of price
aggregates.

**Bottom line.** A real structural object was found - the graduation finish line - and it is
efficiently priced at every point on the curve. The identity layer yields exactly one
surviving signal, and it predicts **death, not success**. Information on this market is
one-sided.

---

## 1. The graduation finish line

Peak virtual reserves per token decay monotonically and then spike: 9,146 tokens sit exactly
at the vsol 115 ceiling (2.6% of the universe) against 383 in the 110-115 band. That spike is
the bonding curve completing and migrating.

| reaches vsol | tokens | graduate | completion |
| --- | --- | --- | --- |
| 60 | 29,396 | 9,146 | 31.1% |
| 80 | 15,799 | 9,146 | 57.9% |
| 100 | 10,961 | 9,146 | 83.4% |

**This object is invisible to a decile scan.** It is 2.6% of the universe, compressed into
the top pool-size decile alongside the 11,888 tokens that stall at 50-55 - which is exactly
the dilution failure that motivated the session.

Price is a deterministic function of reserves and on this curve `price ~ vsol^2`, so a token
running from vsol 80 to 115 gains **+107% mechanically**.

## 2. The payoff is binary and LEFT-tailed

Entry at the first print after crossing the threshold, exit at the last pre-migration price:

| entry th | outcome | n | mean | p50 | win |
| --- | --- | --- | --- | --- | --- |
| 80 | graduates | 4,655 | **+83.44%** | +94.1 | 97.1% |
| 80 | fails | 6,653 | **-76.58%** | -88.9 | 4.1% |
| 90 | graduates | 4,624 | +47.63% | +53.5 | 96.9% |
| 90 | fails | 3,719 | -78.99% | -91.8 | 3.8% |
| 100 | graduates | 4,474 | +21.62% | +24.5 | 95.5% |
| 100 | fails | 1,815 | -80.27% | -93.9 | 2.5% |

Realized gains track `(115/V)^2` to within a point. **This is the one regime in this codebase
where the loss tail, not the profit tail, carries the distribution** - the opposite of the
price-action space, where a take-profit inverted the payoff.

## 3. It is efficiently priced - the central result

Break-even completion probability against the actual rate:

| entry th | break-even P(grad) | actual | gap |
| --- | --- | --- | --- |
| 80 | 47.8% | 41.2% | **-6.6pp** |
| 90 | 62.4% | 55.4% | **-7.0pp** |
| 100 | 78.8% | 71.1% | **-7.7pp** |

**The gap is the same ~7pp at every point on the curve.** A constant shortfall across a
threefold change in payoff odds is the signature of a correctly priced market, not of a
missing feature. The finish line is public, deterministic, and computable by every
participant, so it carries no information asymmetry.

Every exit policy tested is negative: hold 60s/5m/30m, exit at vsol 95/100/105, and vsol
stops at 55/60 (9 configs, best -3.52%, IS and OOS agreeing).

**Stops do not rescue it.** 78.9% of th=80 entries breach -10% before resolving, including
roughly half of the eventual graduates. Honest stop fills (breach detected on the slot close,
filled at the *next* print) improve the mean from -10.71% to -4.97% and never reach zero.

## 4. The migration race is a hard execution limit

Only **30.4%** of tokens that reach vsol 110 still have a bonding-curve print to sell into -
the rest complete and migrate inside the reaction window. Any backtest that exits "at
graduation" is fictional for about 70% of its winners. This gate must be applied to any
future work on this event.

## 5. The floor asymmetry - keep this

The curve has a hard floor at vsol 30, so the maximum loss is mechanically bounded by entry
proximity to it: `(30/V_entry)^2 - 1`.

| entry vsol | max structural loss |
| --- | --- |
| 32 | **-12%** |
| 36 | -31% |
| 60 | -75% |
| 80 | **-86%** |

94.9% of tokens are enterable within 6 SOL of the floor. This is a risk fact that applies to
every rule: **entry vsol sets the downside before any exit logic runs.**

It does not by itself pay. From a floor entry only 11.9% ever reach vsol 40 (break-even needs
~20%), and measured returns are -6.47% (exit@40) to -8.26% (exit@80) over 295,887 entries.

## 6. The identity layer - one signal killed, one survived

Rosters built strictly from 08-01..08-08 and tested on 08-09..08-16.

**KILLED - veteran convergence at the floor.** Count of wallets already in the token that
carry a graduation track record looked strong and monotone: 0 veterans -> 11.63% reach vsol
40, 5+ veterans -> **26.74%**, a 2.3x lift clearing the ~20% break-even. It held at matched
entry price (hit rate 22.7% -> 36.7% at ventry 35). **It does not survive a crowd-size
control**: within matched buyer count *and* matched entry price, 5 cells favour high veteran
share and 5 favour low. The lift was the number of buyers, not who they were.

**SURVIVED - the death predictor.** At the vsol-80 crossing, low mean roster quality of the
pre-crossing buyers (`wrate <= 0.037`):

| crowd size | n | P(graduate) | baseline | mean | p50 |
| --- | --- | --- | --- | --- | --- |
| <=50 buyers | 123 | 32.5% | 40.5% | -20.41% | -88.1 |
| 51-200 | 233 | **16.3%** | 42.4% | -55.91% | -89.3 |
| 201-600 | 229 | **3.1%** | 40.1% | -69.38% | -89.2 |

**The effect strengthens with crowd size, which is the opposite of a crowd-size confound** -
that inversion is what distinguishes it from the killed signal. Stable on 6 of 8 days, the
two failures being the two smallest samples (n=25, n=19). A large crowd with no operator
track record in it is retail piling into a token nobody is supporting, and it dies 13x more
often than baseline.

**It is one-sided and not directly tradeable.** The complement (crowd 201-600, roster quality
above the cut) returns -1.09% on n=1,407 with median -36.22, only 3 of 8 days positive, and
the top 20 trades contribute +2,233 against a total P&L of -1,537. It fails every standing
gate. The signal detects doom; it does not detect success.

## 7. Why - and it now has a measurement

Aggregate realized wallet P&L over 08-01..08-08: **-394,083 SOL** across 447,106 wallets.
A bonding curve is a closed transfer system with no external cash flow, no yield and no
valuation anchor, so the sum of all trader P&L is negative by exactly the fees extracted.
The average taker must lose.

Two consequences, both confirmed on every experiment above:

- **Anything derivable from the public curve state is priced.** The curve is a deterministic
  public function and every participant sees the same tape in the same slot. Gross drift for
  a taker measured ~0 or negative everywhere - at the floor, mid-curve, and at the finish
  line - so net return is the negative of the cost.
- **Information is one-sided.** Failure requires only the *absence* of operator support,
  which is observable now. Success requires the *presence* of future buying, which is not yet
  determined. That is why the identity layer predicts death reliably and success not at all.

## 8. Honest note on the premise

The session opened by arguing that a fat right tail makes bucket means uninformative and that
the tail should be predicted directly. The graduation regime turned out to be **left**-tailed,
so the premise was half wrong. It was still the right move: targeting a structural event
rather than a feature decile is what surfaced the finish line, the migration race and the
floor asymmetry, none of which any bucket scan could see.

## 9. Data

`wstudy` additions: `xc` (per-token threshold crossings), `gx`/`go`/`v_go` (214,656 threshold
entries with honest fills and forward outcomes), `gs` (stop-loss sims), `gw`/`gr` (entry/exit
threshold grid), `ge`, `gf`, `gid`/`gid2` (identity features at crossings), `fe`/`fen`/`fend`/
`ffw`/`fr`/`fret`/`fid`/`fel`/`ftk` (floor-entry framework), `tw` (10.5M mint-wallet buy
pairs), `wr` (graduation-participation roster), `wpnl` (realized P&L roster).
