# 2026-08-20 — Token metadata (uri host) tested as the missing 8dtx selector

Follow-up to `2026-08-17-wallet-8dtx-clone-refuted.md`, which parked the clone because his
edge is *which token he picks* and `tokens.meta` was empty. Metadata now lands, so the park
condition was re-tested.

## What actually got ingested

`017e453e` decodes the create-ix `uri` into `tokens.meta`. As of this run:

- 84,791 of 746,372 tokens carry `meta.uri`; **0 are enriched**. The off-chain document
  behind the uri is not fetched, so name/description/image/twitter/telegram/website — the
  fields a token-quality signal would actually live in — are still absent.
- URIs exist **only from 2026-08-18**, because the uri appears solely on the create tx and
  `raw_txs` drops after 3 days. The original 8dtx study window (07-22..08-16) can never be
  back-filled.
- The overlap that does exist is real: 8dtx is still active (437/387/276 mints on 08-18/19/20).

So the only metadata available for testing is the **uri string itself** — host and path.

## Instrument

`mstudy.{tok,tr,dp}`, tokens created 08-18..08-19. Decision point fixed at **age 68s** (his
median entry); state uses trades at `age <= 68` only, forward return uses `68 < age <= 188`.
Entry band `vsol0` 33-46 (his real reserves 3-16). Fixing one decision point for every token
isolates **selection** from timing. `end_pct` is a naive buy-and-hold over that window — a
**ranking** instrument, not a PnL claim; see "Not priced" below.

## The host axis is real, stable, and independent — and far too small

Raw host spread looks enormous (graduation 0.10% for `clout.family` to 3.73% for `ipfs.io`,
median volume 0.00 to 49). **That spread is liveness, not quality**: conditioned on a token
that actually trades, graduation collapses to 2.40-3.68% across the top hosts.

At the decision point, inside his band:

| set | n | no-exit | mean end | median end | win |
| --- | --- | --- | --- | --- | --- |
| his picks | 374 | 0.8% | **+12.90%** | +2.62% | **52.0%** |
| all in band | 3,405 | 9.3% | -5.44% | -16.52% | 29.0% |
| band + `ipfs.io` only | 1,854 | 8.3% | -3.93% | -10.72% | 34.2% |
| band, not his picks | 3,031 | 10.4% | -7.94% | -17.74% | 25.8% |

The host filter moves the population **-5.44 -> -3.93**: about **7% of the 20.8pp selection
gap**. Per-day the ordering holds (ipfs.io win 34.2% / 34.3%; uxento 22.2% / 24.9%), so the
effect is not noise — it is simply small.

**It is not redundant with tape activity**, and it is strongest where the tape is quiet —
exactly 8dtx's regime (his quiet gate is 7.0 SOL of 10s churn vs 37 at skipped setups):

| unique wallets pre-decision | `ipfs.io` mean / win | other hosts mean / win |
| --- | --- | --- |
| 1-18 | -3.55% / 33.2% | **-16.62% / 14.7%** |
| 18-50 | -2.74% / 37.4% | -6.68% / 27.0% |
| 51-471 | -6.28% / 30.8% | -3.62% / 23.2% |

In the quiet band the excluded hosts run -10.77% mean, 22.0% win, **18.1% no-exit** vs
`ipfs.io` at -3.11% / 35.5% / 10.3%. So the host is usable as a **death-exclusion filter in
the quiet regime**, matching the standing rule that identity predicts death, not success.
It still closes only 2.76pp of a 25.1pp gap there, and every host stays negative gross
against a ~3.5% cost bar.

8dtx himself over-picks `ipfs.io` (10.12% vs 6.00% pick rate in the quiet band) but takes
52 of his 194 quiet-band picks from the excluded hosts — **host is not his rule**.

## Name / symbol are dead

On-chain `name`/`symbol` cover the full history and cost nothing, so they were tested too.
Flat: win rate 25-31% across non-ascii names, all-caps symbols, and symbol length, with no
sign flip. A meme-keyword name is worse (-10.58% vs -4.52%, n=532) but that is one weak cell.

## As an EXCLUSION filter it works — held out

The question above ("does metadata explain his picks?") is not the only one worth asking.
The operator question is **"does dropping bad-metadata tokens improve the rule?"** — a filter
does not have to reproduce his selection to pay. Tested separately, and it holds.

Protocol: rank hosts on **08-18 only**, exclude the worst, validate on **08-19 untouched**.
Both days restricted to 11:00-24:00 UTC, because the uri decode shipped 08-18 ~10:00 and
hour-of-day is a known confounder here. On 08-18 alone the ranking is `meta.uxento.io`
-11.88%, `metadata.j7tracker.io` -7.10%, `ipfs.io` -3.11% — so the exclusion list is picked
without ever seeing 08-19.

Per-host, hour-matched, the ordering is stable across both days:

| host | 08-18 mean / win | 08-19 mean / win |
| --- | --- | --- |
| `ipfs.io` | -3.11% / 36.3% | -2.27% / 36.9% |
| `metadata.j7tracker.io` | -7.10% / 22.0% | -12.59% / 16.0% |
| `meta.uxento.io` | -11.88% / 20.6% | -13.10% / 23.9% |

Held-out day 08-19, quiet band:

| policy | n | kept | no-exit | mean end | median end | win |
| --- | --- | --- | --- | --- | --- | --- |
| no filter | 954 | 100% | 16.6% | -5.38% | -15.1% | 30.9% |
| drop uxento + j7tracker | 711 | 74.5% | 15.6% | -2.96% | -9.1% | 34.8% |
| `ipfs.io` only | 592 | 62.1% | 12.7% | **-2.27%** | -6.5% | **36.9%** |

**+3.11pp mean and +6.0pp win rate on a day never used to build the list**, for 38% of the
trades given up. The tightest policy (`ipfs.io` only) beats the looser one, so the shape of
the finding is **keep generic IPFS, drop the tool-launchpad hosts** — the excluded hosts are
bot-launcher infrastructure, and their tokens die harder.

Note the gain is **not** coming from stuck bags: no-exit barely moves (16.6% -> 15.6%). It
comes from the body of the distribution (median -15.1% -> -9.1%).

**A "no uri" policy is not needed.** The 12.9% of window tokens lacking a uri are entirely
pre-deployment (0% coverage before 08-18 10:00, 100% after) — an artifact, not a token
property.

## Verdict

Two questions, two answers:

1. **Does metadata explain 8dtx's selection? No.** He over-picks `ipfs.io` (10.12% vs 6.00%)
   but takes 52 of his 194 quiet-band picks from the excluded hosts. His picks show 0.5%
   no-exit vs 10-18% for the population — he essentially never buys a token that stops
   trading, and nothing in the uri string reproduces that. **The clone stays parked**; the
   park condition asked for socials / creator reputation, and fetching the document behind
   `meta.uri` is still not done.
2. **Does excluding bad metadata hosts improve a rule? Yes, ~3pp, held out.** Worth wiring
   as an entry exclusion.

**But it does not turn a losing rule into a winner.** The best policy still prints -2.27%
gross against a ~3.5% cost bar. It is a real improvement to a negative baseline, not a fix.

**Not priced.** Every number here is a fixed-window buy-and-hold proxy; a standalone harness
has previously run ~5.7pp optimistic against `reduce`. Before the exclusion is believed as
money it needs a simulate run on the true unfiltered corpus. The evidence is also **thin —
two days, one of them partial**, so it should be re-checked as days accumulate.
