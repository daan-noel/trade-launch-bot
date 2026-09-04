# Tool census and decision-node tables

Phase 1-2 observatory of [market-model-and-workflow.md](market-model-and-workflow.md):
who the machines are, and what follows when one of them breaks a token's silence. All
tables live in the **LOGGED** schema `census` on the workstation PG (a crash cannot
truncate them; the UNLOGGED failure mode is recorded in memory).

The clean vocabulary window starts **2026-08-30 17:48:14 UTC** - the decoder upgrade at
that instant renamed several instructions, so a template seen only before it is the same
machine under an older name. Rows from 08-28 to the cutover carry the old names.

## Tables

| table | grain | content |
| --- | --- | --- |
| `census.tool_census` | buy build template (md5 of `ix_labels::text`) | legs, n_tx, n_mints, n_wallets, n_days, first/last seen, SOL size stats, cu_price mode; window 08-28.. |
| `census.tool_census_sell` | sell build template | same, lighter |
| `census.gap_breaks` | buy ending a >=10-slot silence | mint, slot, template, wallet, payer, sol, vsol, gap_slots; post-cutover only |
| `census.mint_summary` | mint | max_vsol + its slot over the window (graduation detector at >=114.9) |
| `census.gap_follow` | one row per gap break | next-60s/300s buy/sell SOL, ntx, unique wallets, grad_after flag |

Rebuild: one query at a time, `SET work_mem='128MB'` - two parallel builds at 512MB
OOM-kill the 6GB WSL VM.

## Reading rules

- **Effective n is mints, not breaks.** A machine with hundreds of breaks on a handful
  of mints is one operation, not a population.
- `grad_after` is right-censored by the window end; it ranks machines, it is not an
  absolute rate.
- Follow-through flow includes racers and copiers, not only retail.
- These are Phase 1-2 descriptions. No number here is a money claim; a rule built on a
  machine signature still walks Phases 4-7.

## First findings (window 08-30 17:48 .. 09-03)

- 17,254 buy build templates; 4,752 sell. Baseline over 295,518 silence breaks:
  P(>=1 SOL follows in 60s) = 48.1%, P(reaches the wall after) = 6.83%.
- **Brand symmetry separates layers.** Axiom (~1.12M SOL each way), Terminal, GMGN are
  buy/sell symmetric = the terminal crowd. The seed-builder cohort
  (AdvanceNonce + CreateAccountWithSeed + Token2022) is ~859 wallets buying ~1 SOL -
  the racer layer; its sell-side absence is mechanical (seed accounts are a buy-path
  construct).
- **Campaign-machine species exist and are few.** Exemplar: template `828457fa`
  (unknown program `9ddjzqYh..` + `Pump.Fun: Buy`) - 321 silence breaks on 7 mints,
  5 of 7 graduate, median 9.5 SOL follows within 60s. A dedicated operation.
- **The strongest repeated signature is `29d9aacb`**
  (`SetComputeUnitPrice` FIRST, then limit - the CB order is the fingerprint - + ATA
  idempotent + `Pump.Fun: Buy`): 5,959 breaks on 245 mints across 5 days, 94.7% get
  >=1 SOL follow-through in 60s, 39 of 245 mints reach the wall after (~4x baseline at
  the break grain). Same class: `d2c86e7a` (ATA Create + `BuyExactSolIn`), 122 mints,
  20 graduate.
- Terminals/aggregators (DFlow, Jupiter, Photon, Trojan, Maestro, Axiom-nonce) break
  silences at 1.5-2x baseline follow-through - readers arriving, not initiators.
- Bump-bot species: sub-0.01 SOL medians, periodic ~4-minute gaps, one build running
  63k buys through 97 wallets; a dust build with 1,380 breaks on 9 mints. Feed-position
  maintenance, mostly not campaigns.

- **Count predicts the wave, not the campaign.** N same-template buys in the breaking
  slot (the "no tx + N of tool X" signature) raises immediate follow-through
  monotonically - Terminal count 1->4: 59% -> 79% get >=1 SOL in 60s, avg 4.5 -> 10.1
  SOL - but P(reaches the wall) stays flat ~7-8%. Same shape on Axiom. The campaign
  machine `29d9aacb` fires alone on 98% of its breaks. So multi-tool bursts mark
  attention arrival (a racer-speed wave); campaign success tracks WHICH machine broke
  the silence, not how many landed.

## Open next steps

1. Creator-cluster linkage: does the breaker's `payer_id` trace to the launch cluster
   (payer coverage starts 09-02) - dev-funded vs third-party volume service.
2. Count semantics: N same-template buys in the breaking slot/run ("2 Axiom",
   "4 Terminal") vs follow-through.
3. Trader response (Phase 3): which breakers do the seed-builder racers and the
   daily-profitable wallets fire on.
