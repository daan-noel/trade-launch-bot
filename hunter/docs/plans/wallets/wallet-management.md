# Wallet management

Live holds several trading wallets at once. Each real rule names one active wallet, or none, which means the primary at the moment the entry fires. The position stores the pubkey that bought. The exit, a manual Re-sell or Verify, the orphan sell and its sibling close, and the cleared-Holding reconcile all read and sign with that wallet (`WalletRuntime::for_position`). Changing the primary, or a rule's wallet, affects the next entry only.

Lab has no keys and no Wallets page. A lab save of a rule does not write the wallet assignment. Forge keeps its own burner pool. Hunter does not call it.

## Where a key lives

The private key is an envelope file under `WALLET_KEYSTORE` (`name.enc`, AES-256-GCM, the same shape Forge uses). `WALLET_KEK` is the one passphrase, in each machine's environment. Postgres stores the pubkey, the name, the status (`active`, `draining`, `archived`), and `key_ref` (the filename only). The key is never in Postgres, an API response, or a log. A write never replaces an existing file: a second wallet under a taken name is refused before anything is stored.

An active or draining wallet whose file is missing or does not decrypt fails boot. Archived wallets are not decrypted.

## The env wallet

`WALLET_PRIVATE_KEY` and `NONCE_ACCOUNTS` load at boot beside the registry, named `main`, unless the registry already loaded that pubkey or archived it. It is primary only when no registry row is. Probe mode uses the env key and exits before the database. With `WALLET_KEYSTORE` unset it is the only wallet.

"Bring it in" (`POST /api/wallets/adopt-env`, or importing the same key) writes its file, inserts its row, and attaches `NONCE_ACCOUNTS`. A row that already exists keeps its name and only gets the missing nonces. The running trader is rebuilt only when its nonce set changes. After that the env key is needed only by `probe`.

## Readiness

Each loaded wallet reports one state, the first missing piece in the order it is fixed: `no_key` (not loaded here, or the key does not match), `needs_nonces`, `init_failed` (the trader's `initialize` error is kept and shown), `needs_sol` (cached balance under `LOW_SOL_LAMPORTS`, 0.01 SOL), `ready`. `needs_sol` still signs; the per-buy balance floor decides each entry. Assigning a rule to, setting as primary, or manually buying with a wallet that cannot sign is refused. A rule whose wallet is not loaded on this box fails its entry; it never buys with another wallet. The balance floor reads the wallet that pays.

## Nonces

A wallet cannot trade without nonce accounts. Provision creates 1 to 16 rent-exempt accounts; it checks the registry row before spending. Attach records accounts the wallet already owns: one `getMultipleAccounts` reads them all and `executor_core::check_nonce_account` keeps only those whose authority is this wallet. A move hands them over with `authorize_nonce_account`, so the addresses stay the same.

The push feed watches every loaded wallet and its nonces. A change to that set (create, import, provision, attach, move, archive, restore) resubscribes the feed in place through `WatchedAccounts`; no restart.

## Assignment

`rule_wallets` is its own table. It is not a column on `strategy_rules` and it is not inside params, so a lab save and `db-incremental-sync.ps1` do not copy it or clear it. The primary is `trader_wallets.is_primary`, one row at a time. A manual buy may name a wallet; it is assigned to that one buy's episode rule in memory only.

The decision kernel never sees a wallet. Live builds one trader per loaded wallet. Fills look the position pubkey up on that book.

## Wallet-scoped reads and actions

Holdings, the holdings summary, wallet tokens, rent status and recover, cashback status and claim, and the manual sell take `?wallet=` (the sell takes it in the body); absent means the primary at that moment (`DeployState::trader_for`). The holdings and cashback caches hold one slot per wallet. A manual sell's reconcile closes only positions that wallet bought. The boot holdings reconcile walks every loaded wallet in turn.

## Page actions

- Send SOL to another registered wallet, or to an outside address with the source name typed. SOL committed to a buy in flight and a fee reserve stay.
- Rename changes the label; the key file keeps its filename.
- Archive needs an active wallet that is not primary, named by no rule, with no open position, no nonce account, no token, and SOL at or under 0.001. Anything else is moved instead. The trader is unloaded.
- Restore loads the file again and returns the row to active, never primary.
- Restart rebuilds the trader on its nonce set after an init that failed on something passing.

Every action is a row in `wallet_activity` (kind, detail, signatures), shown per wallet.

## Move

One move at a time, and it runs on the server. Starting one inserts the row and returns. A driver task re-checks every 5 s and advances a move that is waiting on in-flight work or was cut by a restart; a step that failed waits for a manual retry, which runs it again. `move_lock` keeps the driver and a request from advancing at once. `GET /api/wallets/move/preview` reads what would move first: rules, the primary flag, token balances, nonces, SOL, and positions still in flight.

1. The source becomes `draining`. Rules that name it, and the primary flag when it was primary, move to the destination.
2. Wait until in-flight entries and unconfirmed nonce sells finish.
3. Freeze the source, transfer each token balance, repoint open positions, then thaw. A failed close only leaves rent.
4. `authorize_nonce_account` so the nonce addresses stay the same and the pool membership follows. The destination trader is rebuilt.
5. Sweep spendable SOL. Nonce accounts keep their rent. A small fee reserve stays on the source for the sweep itself.
6. Verify: no open positions, no token balance, no nonce authority on the source. Only then archive and unload. The `.enc` file stays.

## Sync

`hunter/scripts/wallet-sync.ps1` runs on the workstation. `-SshTarget` names the server. It does not hook into `db-incremental-sync.ps1`. Both `.env` files set `WALLET_KEYSTORE`; a relative value resolves against `hunter/` on each side. On the server, `deploy/hunter.compose.yml` bind-mounts `hunter/keystore` into the live container at `/keystore`, so a synced file is the file the bot reads. Both machines use the same `WALLET_KEK`.

It copies missing `.enc` files both ways and stops when the same filename has different bytes. It never deletes a file. It copies wallet rows both ways. Status, primary, nonce authority, and an in-progress move stay as they are on the server. A wallet row is inserted on a side only when that side has the file. It does not move SOL and it does not touch trades, positions, rules, fingerprints, or `.env`. A wallet can sign only on the box that has its file.
