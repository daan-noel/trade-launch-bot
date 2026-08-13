# Find the best rule for a fingerprint

**Input:** a fingerprint and a datetime range (your estimate of one habit).
**Output:** one champion `RuleParams` for that slice, compared to ungated and to
any incumbent.

Search the range you picked. Do not hide a tail of it.

```
  fingerprint + datetime range
              │
              ▼
  1. freeze one harvest
              │
              ▼
  2. generate beams (slot product)
              │
              ▼
  3. simulate each full rule on the range
              │
              ▼
  4. champion = archive max
              │
              ▼
  champion  vs  ungated  vs  incumbent
```

**Score:** `simulate()`, realized PnL, worst fill + `pumpfun_impact`, copycat
guard ON. Same buy size as the incumbent when one exists. The candidate is
always a complete rule (entry conjunction + harvest), never a metric scored
alone.

## 1. Harvest

Three ungated cells (empty entry):

| cell | exit |
| --- | --- |
| demand | `nonvol_buy` burst + `liquidity>=85` (split fingerprints only) |
| swing | `liquidity>=85` + `stall>=240` |
| time-stop | `held>=300` |

Freeze the best of demand / swing. Time-stop cannot be the frozen harvest.
If every cell loses, still freeze demand (or swing if there is no split).

## 2. Beams

A beam is one filling of three slots, generated from `REGISTRY` + this range's
time-to-peak — not copied from a past winner.

```
  checkpoint  ×  extra selector (0 or 1)  ×  one trigger family
  + ungated  +  T4 (trigger, no selector)
```

| Slot | How it joins | Filling |
| --- | --- | --- |
| Checkpoint | `time` with an **upper bound** | 2–3 `time in (a,b)` bands before this range's typical peak. Median time-to-peak ≲ 15 s → skip; only ungated + T4. |
| Extra selector | can permanently fail | `liquidity` band; windowed `gross_flow` / `unique_wallets` / `nonvol_gross` floor at a checkpoint |
| Trigger | times the buy inside the window | accumulation **or** dip **or** confirmed-move **or** organic — never two in one beam |
| Wait-only | monotonic lifetime floor, no cap | not used as a selector |
| Exit | `scope: Position` | harvest only |

The engine waits until entry is true. A condition selects only if it can
**permanently fail**.

A new metric joins its slot from registry flags (`monotonic`, `kind`, `family`,
`scope`). A new colliding trigger family is the only hand thesis. Unknown /
`Standalone` → its own exclusive beam.

Values come from this range. Same-window clauses on one dynamic group merge
(`window_size_sec` unique per group array).

Optional drop / add / retune: re-simulate the whole remaining rule each time.
Champion is the archive max, not the greedy-path end.

## 3. Report

| Check | Meaning |
| --- | --- |
| Champion vs ungated (same harvest) | If it does not beat ungated, the juice is buy-everything |
| n floor | Too few closed trades → no rule |
| PF > 1 under authority | |
| Fill spread (optimistic / authority) | Quote next to every SOL number |
| Selective claim | enter% of matched, guard OFF, ≲ 60% — necessary, not sufficient |
| T4 / no selector | latency ladder; a 1 s entry floor must still pay |

Ungated always runs:

| ungated | beams | report |
| --- | --- | --- |
| loses | all lose | refuse |
| pays | all lose | juice is ungated |
| either | one pays | that filling is the candidate |

Refuse is a valid result. Paper the next launch burst; if the habit moved, pick
a new range and run again.
