# Wallet management

Live holds several trading wallets at once. Each real rule names one active wallet, or none, which means the primary at the moment the entry fires. The position stores the pubkey that bought. The exit signs with that wallet. Changing the primary, or a rule's wallet, affects the next entry only.

Lab has no keys and no Wallets page. A lab save of a rule does not write the wallet assignment. Forge keeps its own burner pool. Hunter does not call it.

## Where a key lives

The private key is an envelope file under `WALLET_KEYSTORE` (`name.enc`, AES-256-GCM, the same shape Forge uses). `WALLET_KEK` is the one passphrase, in each machine's environment. Postgres stores the pubkey, the name, the status (`active`, `draining`, `archived`), and `key_ref` (the filename only). The key is never in Postgres, an API response, or a log.

An active or draining wallet whose file is missing or does not decrypt fails boot. Archived wallets are not decrypted. When `WALLET_KEYSTORE` is unset, `WALLET_PRIVATE_KEY` and `NONCE_ACCOUNTS` remain the one trader, named `main`. Probe mode still uses the env key and exits before the database.

A new wallet cannot trade until it has SOL and nonce accounts. Provision creates 1 to 16 rent-exempt nonce accounts, or a move delivers them.

## Assignment

`rule_wallets` is its own table. It is not a column on `strategy_rules` and it is not inside params, so a lab save and `db-incremental-sync.ps1` do not copy it or clear it. The primary is `trader_wallets.is_primary`, one row at a time.

The decision kernel never sees a wallet. Live builds one trader per loaded wallet. Fills look the position pubkey up on that book. Pages that still read a single wallet keep the trader that was primary at boot until the process restarts. New nonce accounts are watched on the push feed after a restart; until then the nonce poll covers sells.

## Move

One move at a time, resumable, and it runs on the server.

1. The source becomes `draining`. Rules that name it, and the primary flag when it was primary, move to the destination.
2. Wait until in-flight entries and unconfirmed nonce sells finish.
3. Freeze the source, transfer each token balance, repoint open positions, then thaw. A failed close only leaves rent.
4. `authorize_nonce_account` so the nonce addresses stay the same and the pool membership follows. The destination trader is rebuilt.
5. Sweep spendable SOL. Nonce accounts keep their rent. A small fee reserve stays on the source for the sweep itself.
6. Verify: no open positions, no token balance, no nonce authority on the source. Only then archive. The `.enc` file stays.

## Sync

`hunter/scripts/wallet-sync.ps1` runs on the workstation. `-SshTarget` names the server. It does not hook into `db-incremental-sync.ps1`.

It copies missing `.enc` files both ways and stops when the same filename has different bytes. It never deletes a file. It copies wallet rows both ways. Status, primary, nonce authority, and an in-progress move stay as they are on the server. A wallet row is inserted on a side only when that side has the file. It does not move SOL and it does not touch trades, positions, rules, fingerprints, or `.env`. A wallet can sign only on the box that has its file.
