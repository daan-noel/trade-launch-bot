# Wallet Sync - `wallet-sync.ps1`

Copy Hunter **trading wallets** between the workstation and the EC2 server, in the
direction you choose. It moves two things:

1. the encrypted key files (`*.enc` in `WALLET_KEYSTORE`), and
2. the wallet rows in Postgres (`trader_wallets`, `trader_nonces`, `wallet_moves`,
   `rule_wallets`).

It never moves SOL, and it never touches trades, positions, rules, fingerprints, `.env`,
or `WALLET_KEK`. For trade history use [db-incremental-sync.md](db-incremental-sync.md);
for rules use the Rules board's **Sync** button.

Design detail: [wallet-management.md](../docs/plans/wallets/wallet-management.md#sync).

---

## TL;DR

Run on the **workstation**, in PowerShell, from `hunter/`:

```powershell
# 1. See what would happen (writes nothing)
./scripts/wallet-sync.ps1 -Direction ServerToLocal -DryRun

# 2. Do it
./scripts/wallet-sync.ps1 -Direction ServerToLocal
```

Then press **Refresh** on the target's Wallets page.

| You want to... | Command |
| --- | --- |
| Bring the server's wallets to the workstation | `-Direction ServerToLocal` |
| Push wallets made on the workstation to the server | `-Direction LocalToServer` |
| Preview first (recommended) | add `-DryRun` |

**The source wins.** Whatever the source says about a shared wallet (name, status,
primary, archived) overwrites the target.

---

## Before you run it

- `ssh`, `scp`, and `psql` are on `PATH`.
- The local Postgres is up (docker, port from `DB_PORT` in `hunter/.env`, default `5555`).
- The SSH key exists at `hunter/aws-ec2-key.pem` or `~/.ssh/aws-ec2-key.pem` (or pass
  `-SshKey`). If it has a passphrase, set `$env:SSH_KEY_PASSPHRASE` or type it once
  when prompted.
- Both `.env` files set `WALLET_KEYSTORE`, and **both machines use the same
  `WALLET_KEK`** - otherwise the copied key files cannot be decrypted on the target.
- No wallet move is running on either side.

---

## Parameters

| Param | Default | Meaning |
| --- | --- | --- |
| `-Direction` | **required** | `ServerToLocal` or `LocalToServer`. Picks the source. |
| `-DryRun` | off | Print the plan, write nothing. |
| `-SshTarget` | `ubuntu@35.158.128.131` | The server. |
| `-SshKey` | `hunter/aws-ec2-key.pem`, else `~/.ssh/aws-ec2-key.pem` | Private key for ssh/scp. |
| `-RemoteDir` | `~/trade-launch-bot/hunter` | Hunter dir on the server; its `.env` is read from here. |
| `-Database` | `hunter_bot` | Local DB name (server uses its `POSTGRES_DB`, falling back to this). |
| `-LocalPgPort` | `5555` | Local Postgres port. **`DB_PORT` in `hunter/.env` overrides it.** |
| `-LocalPgUser` | `postgres` | Local Postgres user. |
| `-LocalPgPassword` | `$env:PGPASSWORD` | Local password; falls back to `POSTGRES_PASSWORD` in `hunter/.env`. |
| `-TunnelLocalPort` | `5433` | Local end of the SSH tunnel to the server's Postgres. Change it if 5433 is taken. |
| `-RemotePgPort` | `0` (auto) | Server Postgres port. `0` reads `DB_PORT`, then `POSTGRES_HOST_PORT`, from the server `.env`, else `5555`. |
| `-LocalPgHost` | `localhost` | Accepted but unused: psql always connects to `127.0.0.1`. |

Everything else (server password, user, keystore path) comes from the server's `.env`.

---

## What it does, step by step

1. Reads `hunter/.env` locally and the server's `.env` over ssh.
2. Lists `*.enc` on both sides. A filename present on both sides with **different
   bytes** stops the run.
3. Opens an SSH tunnel `127.0.0.1:5433 -> server Postgres`.
4. Reads wallet rows from both DBs and stops if anything is unsafe (see below).
5. Prints the **plan**: key files to copy, wallets to add/update, which one becomes
   primary, how many rule assignments change. `-DryRun` stops here.
6. Copies missing key files source -> target with `scp` (**add-only**: nothing is deleted
   or replaced).
7. Writes the rows to the target in **one transaction**:
   - `trader_wallets`: every source wallet whose key file is on the target is inserted
     or overwritten. The source primary becomes the **only** target primary. Wallets
     that exist only on the target are left alone.
   - `trader_nonces`: every source nonce whose authority wallet is on the target.
   - `wallet_moves`: finished source moves, by id (existing ids untouched).
   - `rule_wallets`: for every rule both sides have, the target's assignment becomes
     the source's (set or cleared).
8. Checks the target: exactly one active primary, and a key file behind every active
   wallet. Problems print as warnings.
9. Closes the tunnel and deletes the temporary passphrase file.

---

## When it refuses (nothing is written)

| Message | Fix |
| --- | --- |
| `A wallet move (...) is in progress` | Let the move finish on that box, then re-run. |
| `Envelope '<file>' differs between this machine and the server` | Same filename, different key. Find out which one is right; rename or remove the wrong one by hand. |
| `Wallet <pubkey> has key_ref 'a' on ... and 'b' on ...` | The same wallet points at different key files on each side. Fix the row on one side. |
| `The name '<name>' belongs to another wallet` | Rename one of the two wallets on its Wallets page. |
| `WALLET_KEYSTORE is unset` / `POSTGRES_PASSWORD is missing` | Add it to the named `.env`. |
| `Tunnel port 5433 never opened` / `SSH tunnel exited` | Port busy (pass `-TunnelLocalPort`) or ssh cannot reach the server. |

## Warnings (run continues)

- `Skip <wallet>: <key_ref> is not on <source> either` - the row has no key file
  anywhere, so it is not copied.
- `Skip rule <id> assignment: wallet ... is not on <target>` - that rule keeps its
  current wallet.
- `<source> has no active primary` - the target's primary is left as it is.
- `<target> has N active primary wallets` - set one on the target's Wallets page.
- `<wallet> is active on <target> but <key_ref> is not there; it cannot sign.`

---

## After it finishes

Press **Refresh** on the target's Wallets page. It re-reads the registry, decrypts the
new key files, and loads balances and nonces from the chain. Until then the running bot
still uses the old wallet set.
