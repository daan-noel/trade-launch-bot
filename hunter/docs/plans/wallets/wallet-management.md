# Wallet management

Live holds several trading wallets at once. Each real rule names one active wallet, or none, which means the primary at the moment the entry fires. The position stores the pubkey that bought. The exit, a manual Re-sell or Verify, the orphan sell and its sibling close, and the cleared-Holding reconcile all read and sign with that wallet (`WalletRuntime::for_position`). Changing the primary, or a rule's wallet, affects the next entry only.

Lab has no keys and no Wallets page. A lab save of a rule does not write the wallet assignment. Forge keeps its own burner pool. Hunter does not call it.

## Where a key lives

Every key hunter-live signs with is an envelope file under `WALLET_KEYSTORE` (`<pubkey>.enc`, AES-256-GCM, the same shape Forge uses). `WALLET_KEK` is the one passphrase, set in each machine's `.env` by hand and never copied by a script. Both are required at boot. Postgres stores the pubkey, the name, the status (`active`, `draining`, `archived`), and `key_ref` (the filename only). The key is never in Postgres, an API response, or a log. A write never replaces an existing file. Rows from before pubkey filenames keep their `<name>.enc` file; `key_ref` names whichever file it is.

A name is a label: 1 to 32 characters, trimmed, no control characters, unique ignoring case (`trader_wallets_name_ci`). Rename changes the label only. Create, import, and rename all check it through `ops::name_error`.

An active or draining wallet whose file is missing, does not decrypt, or names another pubkey is kept as that wallet's load error (shown on the page) and signs nothing; every other wallet loads. Archived wallets are not decrypted.

## No wallet in `.env`

No private key or nonce list is read from the environment. With no wallet loaded (a fresh box, or every key failing) the process still boots: ingest, paper trading and the pages run, the process trader is an inert one that is never initialized and signs nothing, every real entry fails closed, and the Wallets page says so. Creating or importing the first wallet makes it primary and it signs the next entry. The single-wallet pages built on the boot trader read the inert one until the next restart.

`probe` signs with one keystore wallet: `--wallet <name|pubkey>`, or the primary. It decrypts that wallet only and exits before ingest and HTTP.

## Readiness

Each loaded wallet reports one state, the first missing piece in the order it is fixed: `no_key` (not loaded here, or the key does not match; the load error says why), `needs_nonces`, `init_failed` (the trader's `initialize` error is kept and shown), `needs_sol` (cached balance under `LOW_SOL_LAMPORTS`, 0.01 SOL), `ready`. `needs_sol` still signs; the per-buy balance floor decides each entry. Assigning a rule to, setting as primary, or manually buying with a wallet that cannot sign is refused. A rule whose wallet is not loaded on this box fails its entry; it never buys with another wallet. A position whose wallet is not loaded here is never signed or closed; it waits for the wallet. Its sell goes `ExitStuck` with nothing sent, the reaper skips it, Re-sell and Verify answer `409 wallet X is not loaded on this box`, and the next pass after a Refresh that loads the key sells it. The balance floor reads the wallet that pays.

## Nonces

A wallet cannot trade without nonce accounts. Provision creates 1 to 16 rent-exempt accounts; it checks the registry row before spending. Attach records accounts the wallet already owns: one `getMultipleAccounts` reads them all and `executor_core::check_nonce_account` keeps only those whose authority is this wallet. A move hands them over with `authorize_nonce_account`, so the addresses stay the same.

Refresh reads the chain's view of a wallet and stores it: `POST /api/wallets/{pubkey}/sync` for one, `POST /api/wallets/sync` for the page, which first re-reads the registry (`WalletRuntime::reload`: loads rows and key files new on this box, unloads archived ones, follows renames, nonce sets, the primary, and rule assignments) under `move_lock`. Import runs the chain read before the trader starts. Candidates are the registered nonces and every `nonceAccount` a System instruction names in the wallet's newest and oldest 100 transactions (Helius `getTransactionsForAddress`). A trading wallet advances every nonce it rotates through, so the newest page carries the live set. One `getMultipleAccounts` at `confirmed` reads the wallet's balance and every candidate. New confirmed nonces are attached, and registered ones the chain no longer confirms are retired. The balance cache is refreshed, and the trader is rebuilt when its nonce set changed. `getProgramAccounts` cannot do this on Helius: V1 refuses the System Program, and V2 pages through the whole keyspace. A nonce that is never advanced and was created outside both history pages is not found; attach it by pubkey. Sync runs on a click, never on a timer.

The push feed watches every loaded wallet and its nonces. A change to that set (create, import, provision, attach, sync, move, archive, restore) resubscribes the feed in place through `WatchedAccounts`; no restart.

## Assignment

`rule_wallets` is its own table. It is not a column on `strategy_rules` and it is not inside params, so a lab save and `db-incremental-sync.ps1` do not copy it or clear it. A manual buy may name a wallet; it is assigned to that one buy's episode rule in memory only.

The primary is `trader_wallets.is_primary`, at most one row (a unique index), and only a row names it: nothing in memory picks one. Create and import make the new wallet primary when no active row is, read from the registry. Set primary needs a wallet that can sign; archive refuses the primary; a move hands it to the destination. With active wallets and none primary, an entry on a rule without a wallet fails closed with an error log, a wallet-scoped read or action without `?wallet=` is refused, and the page shows a banner. A primary that cannot sign fails its rules closed; nothing switches to another wallet.

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

`hunter/scripts/wallet-sync.ps1 -Direction ServerToLocal|LocalToServer [-DryRun]` runs on the workstation; `-SshTarget` names the server. Each run copies one way and the source wins. It does not hook into `db-incremental-sync.ps1`. Both `.env` files set `WALLET_KEYSTORE`; a relative value resolves against `hunter/` on each side. On the server, `deploy/hunter.compose.yml` bind-mounts `hunter/keystore` into the live container at `/keystore`, so a synced file is the file the bot reads. Both machines use the same `WALLET_KEK`.

It stops before writing when either side has a move in progress, when one filename holds different bytes on each side, when a wallet's `key_ref` differs, or when a source name belongs to another wallet on the target. Otherwise it copies missing key files source to target (add-only, never deletes or replaces), then in one transaction: overwrites or adds every source wallet whose file is on the target, makes the source primary the only target primary, copies nonces and finished moves, and sets each shared rule's assignment to the source's. A wallet only the target has is left alone. `-DryRun` prints that plan and writes nothing. Afterwards it checks the target for exactly one active primary and a key file behind every active row. Trades, positions, rules, fingerprints, `.env`, and the KEK are never copied.

The target loads the result when its Wallets page Refresh runs (below).

