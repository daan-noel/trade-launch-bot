# The ix gate: what it measures, and how the engine says it

An ix gate reads the **build** of each transaction - the software that constructed it,
identified by its ordered instruction labels - and states something about the builds
behind a window's flow. This is the measurement that fixes the gate, the vocabulary the
engine uses to express it, and what is still missing.

The rule this gate belongs to: [wallet-8dtx-derived-rule.md](wallet-8dtx-derived-rule.md).
Method and the traps that produce wrong answers:
[trigger-ix-derivation-method.md](trigger-ix-derivation-method.md).

## 1. Corpus and how every row below is priced

Full universe 2026-08-01..08-21. Every number is on the **225,010 causal fires on tokens
8dtx never trades**, so nothing reads a hindsight label. Net is after 1.25 % fee plus
about 2 % round-trip impact at 1 % of pool. The exit and the fill are **held constant
across every row**, so only the gate differs.

Scratch tables in schema `w8`:

| table | one row per | carries |
| --- | --- | --- |
| `w8.gate` | fire | the gate transaction's `tx_index` |
| `w8.gb` | fire x build | SOL, count, distinct wallets |
| `w8.gc` | fire | composition sums by class |
| `w8.g` | fire | totals, distinct tools, distinct wallets |

**Locating the gate transaction.** `allsol` equals `cum_sol` on all 225,010 rows, so the
derivation's gate is the point where the running total over **leg-0 buys in `tx_index`
order** first reaches `cum_sol`. That prefix resolves on **225,010 of 225,010** fires.

Acceptance against the derivation's own columns:

| column | reproduced |
| --- | --- |
| `allsol` | 225,010 / 225,010 |
| `seedsol` | 225,010 / 225,010 |
| `rsol` | 221,025 / 225,010 (98.2 %) |
| `ngrp` | 212,094 / 225,010 (94.3 %) |

Totals and the seed marker are exact, which is what makes the gate point trustworthy.
`rsol` and `ngrp` differ because the router list and the build key are **definitions**,
and the tables below re-derive both in money rather than inherit them.

## 2. Composition: what counts as a person

Quiet `<= 3`, age `>= 75` slots, `vsol <= 42`, burst floor 1.5 SOL, all held.

| gate | n | net | win | w1 | w2 | w3 | w4 | SOL |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| no seed marker only | 20,429 | **-0.33** | 37.5 | 0.5 | -0.4 | -0.6 | -0.7 | -52.6 |
| 100 % ATA, program ignored | 12,570 | **+0.30** | 38.0 | 1.0 | 0.2 | 1.1 | -0.8 | 28.8 |
| 100 % named router | 2,368 | +6.68 | 51.8 | 5.8 | 7.4 | 6.1 | 7.3 | **122.8** |
| + no seed | 2,359 | +6.69 | 51.8 | 5.8 | 7.3 | 6.2 | 7.3 | 122.6 |
| + CU | 2,323 | +6.86 | 52.0 | 6.0 | 7.5 | 6.3 | 7.5 | 123.8 |
| **+ ATA** | **1,379** | **+7.51** | 52.7 | 7.0 | 7.1 | 10.0 | 6.5 | 80.4 |
| Axiom or Photon, + ATA | 943 | +8.02 | 53.1 | 7.3 | 7.4 | 11.2 | 6.8 | 58.8 |

**The conjunction is the mechanism.** A flag is not a person-detector and a program is
not one either:

- ATA without a named program: **+0.30 %**
- named program without ATA: +6.68 %
- both: **+7.51 %**

The microscope on fires whose burst is one single build says the same thing harder -
`unnamed program + ATA + CU` **loses money**, -1.22 % on 2,401 fires, while
`axiom + ATA + CU` reads +8.03 % on 888.

**ATA is not a proxy for anything already priced.** The two halves of the 100 %-router
population are near-identical on every axis that pays:

| | burst SOL | buys | wallets | age (slots) | vsol | quiet SOL | net |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| all ATA | 2.63 | 2.79 | 2.78 | 776 | 35.4 | 0.79 | **7.51** |
| not all ATA | 2.61 | 3.08 | 3.06 | 934 | 36.4 | 0.96 | 5.52 |

And it holds inside matched burst-size bands, which is where a size proxy would die:

| burst SOL | with ATA | without |
| --- | ---: | ---: |
| 1.5 - 2.4 | **8.49** (727) | 5.74 (573) |
| 2.4 - 3.3 | **6.21** (395) | 3.03 (229) |
| 3.3 - 4.2 | **8.60** (157) | 4.69 (100) |
| 4.2 - 5.1 | 4.96 (69) | 3.87 (44) |

Above 5 SOL the sign flips on 31 fires against 43 - the band the rule already refuses as
"the move already happened".

## 3. Diversity: unmeasurable on this corpus

`w8.mx` **is** the base gate, and the base gate already requires two or more distinct
build groups: `ngrp` has **min 2 and zero rows at 1**. A term the population carries
cannot be priced against the population.

Inside the router+ATA gate it is also nearly vacuous - **1,368 of 1,379** fires already
carry two or more tools and two or more wallets. `wallets >= 3` reads +8.92 % on 631
fires but week 2 sags to 3.9, so it is a wobble, not a term.

This does **not** say the term is worthless. The engine fires at the first instant every
condition holds, which is earlier in the slot than the base gate, and there the term is
not vacuous - adding it moves a 1,500-mint sample from 205 fires at -0.16 % to 41 at
+1.21 %. It says only that **no money ranking for it exists yet**.

## 4. Novelty: measured, and rejected

"Every buyer in the burst is new to this token" is a real cut but not an independent one.

| all ATA | all new to mint | n | net |
| --- | --- | ---: | ---: |
| no | no | 792 | 5.91 |
| no | **yes** | 197 | **3.95** |
| yes | no | 167 | 4.88 |
| yes | yes | 1,212 | **7.87** |

First-on-mint **alone is worse than neither** (3.95 against 5.91). On top of ATA it adds
+0.36 pp, and it costs a per-token set of wallets seen so far - state on the axis this
derivation does not use. ATA is the structural, per-transaction version of the same idea
and captures the value. **Not built.**

## 5. Redundant terms

| term | effect | verdict |
| --- | --- | --- |
| exclude the seed marker | 6.68 -> 6.69 | already implied by router purity |
| require CU | 6.69 -> 6.86 | +0.17 pp, not worth a marker |

## 6. The gate that ships

> **100 % of the burst's buy SOL comes through a named router AND carries an ATA create.**

Costing 42 % of the fires and a third of the total SOL to buy +0.83 pp per trade, four
weeks out of four. Collapsed to one trade per mint: 1,334 mints, **+7.62 %**, 52.4 % of
mints profitable, top 1 % of mints holds 27.7 % of the profit - a body, not a lottery.

## 7. How the engine says it

A trade's markers are one word set by the **producer** (the only layer holding the label
strings) and compared by the engine. A **template** is `all` + `none` over that word:
every marker in `all` present, no marker in `none`. A trade is organic if it matches any
one template.

```
FINGERPRINT.metric_config
  m_flow_split:
    organic_ix_templates: [ {all: [Axiom Trade,  ATA]},
                            {all: [Photon,       ATA]},
                            {all: [Bloom Router, ATA]},
                            {all: [Trojan Trade, ATA]},
                            {all: [Terminal,     ATA]} ]
    wallet_contagion:  false
    creator_is_volume: false
  volume_ix_patterns:  ABSENT
```

```
PER TRADE   (once, at ingest)

  ix_labels ["..", "Axiom Trade: ..", "Associated Token: CreateIdempotent", ".."]
        |
        v   substring scan over the fixed marker vocabulary
  marker word (u16)      AXIOM | ATA | CU
        |
        v   match ANY template?   all present, none forbidden
     ORGANIC                                          else   VOLUME
```

```
ENGINE      window = 1 slot

  tx    marker word        class      SOL
  12    AXIOM|ATA|CU       organic    0.8
  13    PHOTON|ATA|CU      organic    0.9
  14    (no marker)        volume     0.4
        ----------------------------------------
        nonvol_buy     1.7      vol_buy  0.4
        vol_buy_share  19.0 %        ->  the gate FAILS

  without tx 14:  vol_buy_share 0.0 %  ->  the gate PASSES
```

```
WORKFLOW    trade  ->  marker word  ->  classify  ->  window sums  ->  metric  ->  condition
                       (producer)      (engine)      (per window)     (read)     (rule)
```

**There is no second list, and no duplication.** `volume_ix_patterns` is the older
hash-list mechanism and this gate does not use it - it stays absent. Naming both a
template set and a pattern list is a **validation error**, because a mask and a hash list
are two contradictory classifiers on one axis and silently letting one win is how a rule
stops measuring what it says.

**Why a template is not a pattern list under another name.** What rotates is the exact
label array - 531 distinct sequences carry the seed marker on a three-week tape and new
ones ship continuously. `Axiom + ATA` is a sentence about the buyer ("clicked Axiom, has
never held this mint") and stays true for every future build. Under ten entries over a
fixed vocabulary, no hashes.

## 8. What is built, what is not

| piece | state |
| --- | --- |
| marker word, `u16`, router + machinery markers | built |
| `organic_ix_markers`, a single ANY-mask | built |
| **ATA marker** | **to build** - one vocabulary entry |
| **template test, `all` + `none`** | **to build** - `marks()` is ANY today |
| **`vol_buy_share`** | **to build** - zero new state, the sums already accumulate |
| `buy_tools` (distinct builds among buys) | **deferred** - see section 3 |
| first-on-mint, `unique_wallets`, CU marker | not building, measured |

`organic_ix_markers` stays valid as sugar for a single-entry template, so nothing already
stored changes meaning.

`buy_tools` is deferred on purpose: it has no money ranking, and the tighter composition
gate may already remove the over-firing it exists to fix. The order is ship the two
measured pieces, re-run the engine against the SQL fire set, and add it only if the
over-firing survives.

## 9. Not yet established

- **`gross` is the derivation's fixed hold.** Held constant across every row here, so the
  ranking is sound; the absolute numbers are not the rule's own 8-second exit.
- **No fill sweep on the upgraded gate.** d0 / d1 / worst-in-slot is unrun. That column
  decides everything: the same family measured at a millisecond lag rather than an
  in-slot position dies in every published attempt.
- **No fresh-day forward test.** All four weeks are 08-01..08-21.
