//! Launcher runtime settings (Helius RPC/sender, nonce accounts, keystore path).

use anyhow::{bail, Context, Result};
use solana_sdk::pubkey::Pubkey;
use std::path::PathBuf;
use std::str::FromStr;

/// Env-backed settings for the launch executor.
#[derive(Debug, Clone)]
pub struct LauncherSettings {
    pub rpc_url: String,
    pub sender_urls: Vec<String>,
    pub nonce_accounts: Vec<String>,
    /// Directory containing envelope-encrypted wallet blobs (`key_ref` is relative).
    pub keystore_dir: PathBuf,
    /// Passphrase for [`super::keystore::EnvKek`] (wraps ed25519 secrets at rest).
    pub kek_passphrase: String,
    /// Jito block-engine JSON-RPC base (defaults to mainnet global). Used for the
    /// leader-schedule poll (`getNextScheduledLeader`) and as the fallback submit URL.
    pub jito_block_engine_url: String,
    /// Block-engine URLs a bundle is submitted to **in parallel** (`JITO_BLOCK_ENGINE_URLS`,
    /// comma-separated regional endpoints). The same signed bundle raced across
    /// regions lands via whichever region reaches the leader first; the txs share
    /// signatures so they can only be included once on-chain (no double-spend). Falls
    /// back to `[jito_block_engine_url]` when unset (single-region submit).
    pub jito_block_engine_urls: Vec<String>,
    /// How many times the confirm watcher re-bids a `dropped` bundle before giving
    /// up (each re-bid climbs the Jito tip-escalation ladder: p95, p99, …). `0`
    /// disables auto re-bid — a dropped bundle stays dropped for a manual
    /// `POST /bundles/:id/execute`. Env `BUNDLE_MAX_RETRIES`, default `2` (so a
    /// launch bundle gets up to 3 total attempts at escalating tips).
    pub bundle_max_retries: u32,
    /// Jito tip floor for a launch bundle, in SOL (`JITO_MIN_TIP_SOL`) — the
    /// fallback when the live tip-floor feed is cold/stale.
    pub jito_min_tip_sol: f64,
    /// Jito tip **ceiling** for a launch bundle, in SOL (`JITO_MAX_TIP_SOL`) — the
    /// hard per-bundle cost guardrail the escalation ladder clamps to. Higher than
    /// the trade-path default (0.005): a contested launch's whole-bundle tip must be
    /// able to climb far enough to actually win the auction. A tip that never lands
    /// costs nothing, so the ceiling only bounds spend once the bundle wins.
    pub jito_max_tip_sol: f64,
    /// Landed-tip percentile the first attempt targets (`JITO_TIP_PERCENTILE`,
    /// 25|50|75|95|99) before the per-re-bid escalation climbs above it.
    pub jito_tip_percentile: u8,
    /// Jito leader-schedule gate tuning (see [`crate::jito_leader`]) — bounds how a
    /// bundle submit waits for a Jito-participating slot leader before firing.
    pub leader_gate: LeaderGateConfig,
    /// Persistent launch Address Lookup Table (`PUMP_LAUNCH_ALT`). `None` (unset)
    /// keeps the legacy single-message create path; `Some` makes the launcher
    /// compile the create + dev-buy as a v0 tx against this table so it fits the
    /// 1232 B limit. Provision it once with the `create-alt` CLI. Required in
    /// practice for `create_v2` + dev-buy (that combo overflows without it).
    pub launch_alt: Option<Pubkey>,
    /// Wallet-pool backup root (wallet-pool Phase 4) — `None` disables the
    /// post-generation backup entirely; there's no safe default location to
    /// assume, so this stays opt-in rather than required.
    pub backup_dir: Option<PathBuf>,
    /// Pinata JWT for pinning token-metadata images/JSON to IPFS (see
    /// `metadata_upload`) — `None` disables metadata-template authoring with a
    /// clear error rather than a required-at-boot var; nothing else needs it.
    pub pinata_jwt: Option<String>,
    /// Automated treasury→pool funding (docs/wallet-funding-plan.md) — `None`
    /// (the default: `FUND_ENABLED` unset/false) disables the background funder
    /// AND the manual `POST /api/wallet_pool/fund` endpoint. The kill switch.
    pub funding: Option<FundingConfig>,
    /// Shared secret gating `POST /api/wallet_pool/{id}/export` (raw private-key
    /// export). `None` (the default: `WALLET_EXPORT_SECRET` unset) hard-disables
    /// the endpoint — it returns 403. Opt-in per deployment; the endpoint hands
    /// out spendable keys, so it is off unless a secret is set. Serve over TLS.
    pub export_secret: Option<String>,
    /// Post-launch token management (token-management-plan.md) — `None` (the
    /// default: `MANAGE_ENABLED` unset/false) hard-disables the destructive
    /// `POST /api/tokens/{mint}/manage/execute` endpoint (503). The kill switch:
    /// previewing a plan and reading holdings are always allowed; firing real
    /// sells/buys is not, unless explicitly enabled. Mirrors `funding`.
    pub manage: Option<ManageConfig>,
    /// Whether the mandatory fingerprint auditor (Phase 2.F) *waves through* its
    /// fingerprint tells (equal amounts, star funding, same-slot clusters, …). The
    /// audit ALWAYS runs and is persisted regardless; this only decides whether a
    /// tell blocks execution. Default `true` (log + persist, don't block) preserves
    /// existing launch behavior (equal-amount bundles are a known tell the
    /// unlinkability workstream addresses via funding/schedule, not a launch block);
    /// set `AUDIT_ENFORCE_FINGERPRINT=true` to hard-block on any tell. A hard reject
    /// (a malformed account shape) is NEVER waved through, regardless of this flag.
    pub allow_fingerprint: bool,
}

/// Config for executing post-launch management actions (real sells/buys). Only
/// constructed when `MANAGE_ENABLED=true`.
#[derive(Debug, Clone)]
pub struct ManageConfig {
    /// Slippage floor (bps) applied to each management sell — protects proceeds
    /// against a thin curve. Default 10% (managed tokens are often low-liquidity).
    pub sell_slippage_bps: u64,
    /// Log intended actions and place NO real trades. Test before live.
    pub dry_run: bool,
    /// Compute-unit price (micro-lamports/CU) for manage buy/sell txs. The manage
    /// path is operator-timed (latency is NOT a race), so this is decoupled from the
    /// launch create-leg fee and set an order of magnitude lower — the priority fee
    /// is `cu_price × cu_limit`, and a cold manual sell doesn't need to out-bid a
    /// launch slot. Default 50_000 (vs the 750k launch create fee).
    pub cu_price_micro_lamports: u64,
    /// Jito tip **floor** (SOL) for manage buy/sell txs. Default `0.0`: manage txs
    /// are submitted via plain RPC (see [`crate::trader_config::build_manage_trader_config`]),
    /// where a Jito tip buys nothing — the tip instruction is a no-op transfer, so
    /// the tip is zeroed rather than paid to a tip account that never sees a bundle.
    pub jito_min_tip_sol: f64,
    /// Jito tip **ceiling** (SOL) for manage txs. Default `0.0` (pins the tip to 0
    /// with the floor). Raise both only if you route the manage path back through a
    /// bundle/sender that actually credits the tip.
    pub jito_max_tip_sol: f64,
}

impl ManageConfig {
    /// `None` unless `MANAGE_ENABLED=true`. A set-but-malformed numeric var is a
    /// hard error (see [`env_u64`]).
    pub fn from_env() -> Result<Option<Self>> {
        if !env_flag("MANAGE_ENABLED", false) {
            return Ok(None);
        }
        Ok(Some(Self {
            sell_slippage_bps: env_u64("MANAGE_SELL_SLIPPAGE_BPS", 1_000)?,
            dry_run: env_flag("MANAGE_DRY_RUN", false),
            cu_price_micro_lamports: env_u64("MANAGE_CU_PRICE_MICRO_LAMPORTS", 50_000)?,
            jito_min_tip_sol: env_f64("MANAGE_JITO_MIN_TIP_SOL", 0.0)?,
            jito_max_tip_sol: env_f64("MANAGE_JITO_MAX_TIP_SOL", 0.0)?,
        }))
    }
}

/// Jito leader-schedule gate tuning (see [`crate::jito_leader`]). Always present —
/// the gate is on by default and disabling it (`JITO_LEADER_GATE_ENABLED=false`)
/// reverts to today's ungated submit. Fail-open by construction: the gate is an
/// optimization, never a hard dependency (an RPC error or spent budget submits
/// anyway), so these knobs only shape *when* a submit fires, never *whether* it does.
#[derive(Debug, Clone)]
pub struct LeaderGateConfig {
    /// Master switch (`JITO_LEADER_GATE_ENABLED`, default `true`). `false` skips the
    /// getNextScheduledLeader poll entirely and submits immediately.
    pub enabled: bool,
    /// Hard cap on how long a submit waits for a Jito leader before firing anyway
    /// (`JITO_LEADER_MAX_WAIT_MS`, default `2000`). Bounds worst-case launch latency:
    /// a launch is never blocked indefinitely by an unlucky leader schedule.
    pub max_wait_ms: u64,
    /// Submit once a Jito leader is within this many slots of the current slot
    /// (`JITO_LEADER_SEND_WITHIN_SLOTS`, default `2`). The block-engine forwards a
    /// bundle to the upcoming leader, so firing a slot or two early is correct — the
    /// bundle sits in the leader's queue rather than missing the slot.
    pub send_within_slots: u64,
}

impl LeaderGateConfig {
    /// Read the gate knobs from env (all optional, with defaults). A set-but-malformed
    /// numeric var is fatal (see [`env_u64`]) — same rationale as the other configs.
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            enabled: env_flag("JITO_LEADER_GATE_ENABLED", true),
            max_wait_ms: env_u64("JITO_LEADER_MAX_WAIT_MS", 2_000)?,
            send_within_slots: env_u64("JITO_LEADER_SEND_WITHIN_SLOTS", 2)?,
        })
    }
}

/// Safety-railed config for autonomous real-SOL funding (docs/wallet-funding-plan.md
/// P3). Every field is a guard against draining the treasury; all overridable via
/// env, with conservative defaults. Only constructed when `FUND_ENABLED=true`.
#[derive(Debug, Clone)]
pub struct FundingConfig {
    /// Never spend the treasury below this floor (lamports).
    pub treasury_reserve_lamports: u64,
    /// Hard stop mid-batch once this much has been sent in one funding pass (lamports).
    pub max_spend_per_interval_lamports: u64,
    /// Per-wallet target amount by role (jittered at send time).
    pub amount_dev_lamports: u64,
    pub amount_bundler_lamports: u64,
    /// Amount jitter fraction: each transfer is `amount * (1 ± jitter)`.
    pub amount_jitter_pct: f64,
    /// Max random inter-send delay (ms) — timing de-correlation.
    pub max_delay_ms: u64,
    /// Keep at least this many `funded` wallets warm per role (top-up target).
    pub target_funded_dev: i64,
    pub target_funded_bundler: i64,
    /// Log intended transfers and send NOTHING (revert claims). Test before live.
    pub dry_run: bool,
}

impl FundingConfig {
    /// `None` unless `FUND_ENABLED=true`. Reads every `FUND_*` var with a
    /// conservative default so a partial config can't silently over-spend — and a
    /// set-but-malformed value is FATAL (see [`env_u64`]), never a silent revert to
    /// the permissive default.
    pub fn from_env() -> Result<Option<Self>> {
        if !env_flag("FUND_ENABLED", false) {
            return Ok(None);
        }
        Ok(Some(Self {
            treasury_reserve_lamports: env_u64("FUND_TREASURY_RESERVE_LAMPORTS", 50_000_000)?,
            max_spend_per_interval_lamports: env_u64(
                "FUND_MAX_SPEND_PER_INTERVAL_LAMPORTS",
                1_000_000_000,
            )?,
            amount_dev_lamports: env_u64("FUND_AMOUNT_DEV_LAMPORTS", 50_000_000)?,
            amount_bundler_lamports: env_u64("FUND_AMOUNT_BUNDLER_LAMPORTS", 30_000_000)?,
            amount_jitter_pct: env_f64("FUND_AMOUNT_JITTER_PCT", 0.15)?,
            max_delay_ms: env_u64("FUND_MAX_DELAY_MS", 8_000)?,
            target_funded_dev: env_u64("FUND_TARGET_FUNDED_DEV", 2)? as i64,
            target_funded_bundler: env_u64("FUND_TARGET_FUNDED_BUNDLER", 5)? as i64,
            dry_run: env_flag("FUND_DRY_RUN", false),
        }))
    }
}

fn env_flag(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(v) => matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"),
        Err(_) => default,
    }
}

/// Parse a `u64` env var, or `default` if unset. A var that is SET but malformed
/// is a hard error, not a silent fall-back to the default — these back money
/// safety rails (`FUND_*` reserve/cap, `MANAGE_SELL_SLIPPAGE_BPS`), and a typo
/// like `5O000000` silently reverting to a permissive default is exactly the
/// footgun this guards. Refuse to boot instead.
fn env_u64(key: &str, default: u64) -> Result<u64> {
    match std::env::var(key) {
        Ok(v) => v
            .trim()
            .parse()
            .with_context(|| format!("{key} must be a non-negative integer, got {v:?}")),
        Err(_) => Ok(default),
    }
}

/// Parse an `f64` env var, or `default` if unset. Set-but-malformed is fatal —
/// same rationale as [`env_u64`].
fn env_f64(key: &str, default: f64) -> Result<f64> {
    match std::env::var(key) {
        Ok(v) => v
            .trim()
            .parse()
            .with_context(|| format!("{key} must be a number, got {v:?}")),
        Err(_) => Ok(default),
    }
}

impl LauncherSettings {
    /// The Jito tip **ceiling** (`JITO_MAX_TIP_SOL`) expressed in lamports — the
    /// most a launch bundle's whole-bundle tip can escalate to across re-bids. The
    /// dev-wallet launch gate budgets this so a contested launch that climbs the
    /// tip ladder to the ceiling can't strand the dev wallet under-funded. Rounds
    /// up so a fractional-lamport tip never under-budgets.
    pub fn launch_tip_ceiling_lamports(&self) -> u64 {
        (self.jito_max_tip_sol * pump_trader::protocol::LAMPORTS_PER_SOL as f64).ceil() as u64
    }

    pub fn from_env() -> Result<Self> {
        let rpc_url = std::env::var("HELIUS_RPC_URL")
            .or_else(|_| std::env::var("RPC_URL"))
            .context("HELIUS_RPC_URL (or RPC_URL) required for launcher")?;
        let sender_urls = sender_urls_from_env()?;
        if sender_urls.is_empty() {
            bail!("at least one sender URL required (HELIUS_FAST_SENDER_URL or HELIUS_SENDER_URLS)");
        }
        let nonce_raw = std::env::var("NONCE_ACCOUNTS")
            .context("NONCE_ACCOUNTS required for launcher (comma-separated pubkeys)")?;
        let nonce_accounts: Vec<String> = nonce_raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        if nonce_accounts.is_empty() {
            bail!("NONCE_ACCOUNTS parsed empty");
        }
        let keystore_dir = std::env::var("WALLET_KEYSTORE")
            .map(PathBuf::from)
            .context("WALLET_KEYSTORE required (directory of envelope-encrypted wallet blobs)")?;
        let kek_passphrase = std::env::var("LAUNCHER_KEK_PASSPHRASE")
            .or_else(|_| std::env::var("WALLET_KEK_PASSPHRASE"))
            .context("LAUNCHER_KEK_PASSPHRASE (or WALLET_KEK_PASSPHRASE) required")?;
        let jito_block_engine_url = std::env::var("JITO_BLOCK_ENGINE_URL").unwrap_or_else(|_| {
            "https://mainnet.block-engine.jito.wtf/api/v1/bundles".to_string()
        });
        // Parallel submit fan-out. Empty/unset → single-region submit to the base URL.
        let jito_block_engine_urls = {
            let list: Vec<String> = std::env::var("JITO_BLOCK_ENGINE_URLS")
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
            if list.is_empty() {
                vec![jito_block_engine_url.clone()]
            } else {
                list
            }
        };
        let bundle_max_retries = env_u64("BUNDLE_MAX_RETRIES", 2)? as u32;
        let jito_min_tip_sol = env_f64("JITO_MIN_TIP_SOL", 0.0002)?;
        let jito_max_tip_sol = env_f64("JITO_MAX_TIP_SOL", 0.01)?;
        let jito_tip_percentile = env_u64("JITO_TIP_PERCENTILE", 75)? as u8;
        let leader_gate = LeaderGateConfig::from_env()?;
        let launch_alt = match std::env::var("PUMP_LAUNCH_ALT") {
            Ok(v) if !v.trim().is_empty() => Some(
                Pubkey::from_str(v.trim())
                    .context("PUMP_LAUNCH_ALT is not a valid pubkey")?,
            ),
            _ => None,
        };
        let backup_dir = std::env::var("WALLET_BACKUP_DIR").ok().map(PathBuf::from);
        let pinata_jwt = std::env::var("PINATA_JWT").ok().filter(|s| !s.is_empty());
        let funding = FundingConfig::from_env()?;
        let export_secret = std::env::var("WALLET_EXPORT_SECRET")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let manage = ManageConfig::from_env()?;
        // Default: don't block on fingerprint tells (audit still runs + persists).
        let allow_fingerprint = !env_flag("AUDIT_ENFORCE_FINGERPRINT", false);
        Ok(Self {
            rpc_url,
            sender_urls,
            nonce_accounts,
            keystore_dir,
            kek_passphrase,
            jito_block_engine_url,
            jito_block_engine_urls,
            bundle_max_retries,
            jito_min_tip_sol,
            jito_max_tip_sol,
            jito_tip_percentile,
            leader_gate,
            launch_alt,
            backup_dir,
            pinata_jwt,
            funding,
            export_secret,
            manage,
            allow_fingerprint,
        })
    }
}

fn sender_urls_from_env() -> Result<Vec<String>> {
    if let Ok(list) = std::env::var("HELIUS_SENDER_URLS") {
        let urls: Vec<String> = list
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        if !urls.is_empty() {
            return Ok(urls);
        }
    }
    if let Ok(one) = std::env::var("HELIUS_FAST_SENDER_URL") {
        if !one.is_empty() {
            return Ok(vec![one]);
        }
    }
    Ok(Vec::new())
}
