# 2026-08-31 — the busiest wallet on the tape is a router's PDA

## What happened

Checking a transaction by hand against the database turned up a trade credited to
the wrong account. For mint `5cs8iRtGHJJsDMVMfexDwkHKYqqHocsvu4DsUN1Gpump`,
signature `j1UbcwoLd18cggtrzbP1MWzSyMRbsqJfyiZUGgBvHLxNXacguChEL6ugDhNJCFmd87RbBA4gSKMUaPBWKZoSU7R`,
`trades.wallet_id` resolved to `ARu4n5mFdZogZAravu7CcizaojWnS6oqka37gdLT5SZn`. The
account that signed and paid was `A83JDx7TgPS2UqUhmWq3NZXQ2s3AcajaQ7DiJMkWRsDX`.

The decoder was not wrong about what it read. pump.fun's own `TradeEvent`, decoded
from the transaction's `Program data:` log, named `ARu4n5…`:

```
mint 5cs8iRtGHJJsDMVMfexDwkHKYqqHocsvu4DsUN1Gpump
sol 862799721  tok 14626663349352  isBuy 1
user ARu4n5mFdZogZAravu7CcizaojWnS6oqka37gdLT5SZn
```

and account #6 of the `buy` instruction — the `user` slot — was the same account.
Meanwhile `numRequiredSignatures` was 1 and the sole signer was `A83JDx7…`.

## Why

The swap went through the OKX DEX Router
(`proVF4pMXVaYqmy4NjniPh4pqKNfMmsihgd4wdkCX3u`), which does not pass its customer
through to pump.fun. It buys as a PDA of its own and then hands the position over:

- inner ix 8 → pump.fun `buy`, with `user` = the router's PDA
- inner ix 18 → `TransferChecked`, PDA's ATA → the signer's ATA, authority the PDA
- inner ix 16 → System transfer, PDA → the signer, refunding the change

`ARu4n5…` is system-owned, was not a transaction signer, and yet signed a CPI — so
it is an off-curve PDA. It is not a trader and never was. It is one shared vault
that every OKX customer's swap passes through.

The ingest decoder stored the venue's `user` verbatim (`wallet: ev.user.clone()`),
and `Trade` carried no fee-payer field at all, so the real trader was never
captured on any path.

## How much of the table it reached

`ARu4n5…` was the **single busiest wallet in the database** — ahead of every real
trader:

| wallet | trades |
| --- | --- |
| `ARu4n5…` (OKX router PDA) | 427,767 |
| `ssssswdk4RR8HqkE3uwUWzDbd6mXFTTPjcXBKNzQ57E` | 293,360 |
| `64hP97Bwr5PubotcTeGgfhkFrGiLVVxT2kVo9M9b4AEz` | 273,848 |

Over three days, 15,389 of 15,395 OKX-router legs collapsed onto that one id.

Sweeping every address in `wallet_dict` for the off-curve property — an Ed25519
public key is a point on the curve, a PDA is chosen because it is not — found 930
of 1,362,485 addresses that no keypair can sign for. 814 of them carried trades,
462,483 rows in total, and `ARu4n5…` alone was 427,767 of those (92.5%). The next
largest was 10,291. No second offender of comparable size existed.

That number is a floor, not a total: off-curve proves an address cannot sign, but a
router proxying through an ordinary keypair would not show up in it.

## Why it distorted results in two directions at once

- A per-wallet aggregate read N unrelated people as one mega-trader, and that
  synthetic trader topped every leaderboard by construction.
- A unique-wallet breadth count read those same N people as one participant, so
  crowd and participation metrics under-counted on exactly the tokens routed
  traffic touches most.

## The fix

Migrations `0014_trade_payer_and_proxy.sql` and
`0015_backfill_known_proxy_wallets.sql`, plus the decoder change behind them.

The discriminator chosen was **signatures, not a name list**: pump.fun's `buy`
requires `user` to sign, so a `user` absent from
`account_keys[..num_required_signatures]` can only be a PDA that signed a CPI. That
test needs no maintained registry of router addresses and catches a router the day
it deploys. `TxSender` in `shared/ingest/pumpfun/src/decode/protobuf.rs` applies it
once per transaction.

Two things were deliberately not done:

- **The payer does not overwrite the wallet.** A bot can pay from one keypair and
  trade from another, so both are stored and attribution is resolved downstream.
- **Fee payer was chosen over transfer-destination resolution.** Walking the inner
  instructions for the post-swap `TransferChecked` destination is the exact answer;
  the payer is one array index and covers 92.5% of the damage. The exact form can
  layer on in `hunter-lab` if the residual ever matters.

Rows already written cannot be repaired — `raw_txs` is opt-in with 3-day retention,
so an older trade's payer is gone. History is handled on the dictionary instead:
`wallet_dict.is_proxy`, backfilled from the off-curve sweep, which is a property of
the 32 bytes and therefore applies to transactions the feed dropped long ago.
