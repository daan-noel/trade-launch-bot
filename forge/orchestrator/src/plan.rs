//! The `Operation` / `Plan` model — ONE uniform description of "a trade" and "a
//! batch of trades," keyed on **orthogonal axes** so a launch, a snipe, a volume
//! leg, a consolidate, and a fund are the same shape with different field values.
//!
//! The axes (mechanism ⊥ role ⊥ intent ⊥ venue ⊥ amount):
//!   - **mechanism** ([`OpKind`], reused from the venue catalog's [`VariantKind`]
//!     so the two can never drift) — *what on-chain action*: create / buy / sell /
//!     move-SOL / move-token;
//!   - **role** ([`Role`]) — *which wallet persona* acts (dev / bundler / …);
//!   - **intent** ([`Intent`]) — *why* (snipe / make-volume / exit / fund / …);
//!   - **venue** ([`VenueId`]) — *which launchpad*;
//!   - **amount** ([`Amount`]) — *how much*, denominated per the chosen variant.
//!
//! So `DevBuy ≡ { kind: Buy, role: Dev, intent: Snipe }` — not a bespoke type.
//! This is the unification the three inconsistent launcher paths (free-text
//! variant match, enum→string→enum round-trip, hardcoded dispatch with no variant
//! knob) collapse onto.
//!
//! **`min_out` is NOT stored here** — slippage tolerance is (`slippage_bps`); the
//! floor is late-bound from live reserves at send (see the provider). Freezing a
//! `min_out` into a `Plan` would let a dry-run "pass" a fill the live tx reverts.
//!
//! Addresses are base58 `String`s (not `Pubkey`) so a `Plan` stays serializable
//! and persistable (it generalizes `bundles.legs` JSONB + `manage::ActionPlan`);
//! the provider resolves/validates them against `solana_sdk::Pubkey` at build time.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// The mechanism axis IS the venue catalog's `VariantKind` — reused verbatim so
// "the kind of thing this op does" is single-sourced with the on-chain ix table
// (no parallel enum that could drift). Re-exported as `OpKind` for read-clarity
// at Plan call sites.
pub use pump_trader::VariantKind as OpKind;
pub use pump_trader::VenueId;

/// The wallet **persona/role** an op acts as. Mirrors the forge `WalletRole`
/// vocabulary (`dev` / `bundler` / `treasury`) plus the trading roles the volume/
/// exit macros need. `as_str` is the SSOT string used in persistence + the
/// `ActionPlan`/`PlanLeg` bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The launch/dev wallet (creates the token, dev-buys).
    Dev,
    /// A launch-bundle co-buyer.
    Bundler,
    /// A volume/trading wallet (make-volume, ladder, exit).
    Volume,
    /// The treasury/funding sink (consolidate target, funding source).
    Treasury,
    /// A wallet outside our own graph (external counterparty of a transfer).
    External,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Dev => "dev",
            Role::Bundler => "bundler",
            Role::Volume => "volume",
            Role::Treasury => "treasury",
            Role::External => "external",
        }
    }

    /// Parse the SSOT string (bridge from `PlanLeg.role` / `WalletSelection.role`).
    /// Unknown roles map to `External` rather than erroring so a legacy role
    /// string never blocks a plan; the auditor can flag unexpected roles instead.
    pub fn from_str_lossy(s: &str) -> Role {
        match s {
            "dev" => Role::Dev,
            "bundler" => Role::Bundler,
            "volume" => Role::Volume,
            "treasury" => Role::Treasury,
            _ => Role::External,
        }
    }
}

/// **Why** an op happens — orthogonal to the mechanism. `{Buy, Snipe}` is a
/// dev/bundler entry; `{Buy, MakeVolume}` is a wash-volume leg; `{Sell, Exit}` is
/// a position close; `{TransferSol, Fund}` vs `{TransferSol, Consolidate}` split
/// the two funding directions. Drives persona/scheduling defaults downstream
/// (Phase D) and audit rules (Phase E) without overloading the mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    /// Latency-critical entry (dev-buy / bundler buy at launch).
    Snipe,
    /// Ordinary accumulate (manual/API buy).
    Accumulate,
    /// Wash/organic-looking volume.
    MakeVolume,
    /// Close a position.
    Exit,
    /// Move SOL/token OUT to a wallet (funding).
    Fund,
    /// Move SOL/token IN to treasury (consolidate/sweep).
    Consolidate,
    /// Token creation.
    Create,
}

impl Intent {
    pub fn as_str(self) -> &'static str {
        match self {
            Intent::Snipe => "snipe",
            Intent::Accumulate => "accumulate",
            Intent::MakeVolume => "make_volume",
            Intent::Exit => "exit",
            Intent::Fund => "fund",
            Intent::Consolidate => "consolidate",
            Intent::Create => "create",
        }
    }
}

/// The canonical **amount** — one economic quantity, denominated by which side is
/// exact. This is the "variant ⊥ amount" axis: the SAME `Amount` can be encoded
/// by more than one variant (e.g. `ExactQuote` → `buy_exact_sol_in`), and
/// choosing a variant never changes the economic spend, only the encoding. Values
/// are exact base-unit integers (`_quote` = SOL lamports, `_base` = token units)
/// per the locked money vocab — never a hard-coded `amount_lamports`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Amount {
    /// Spend an exact QUOTE (SOL lamports) amount; receive ≥ the min-base floor.
    /// A buy denominated in SOL in (`buy_exact_sol_in` / `buy_exact_quote_in_v2`).
    ExactQuote(u64),
    /// Acquire an exact BASE (token) amount; spend ≤ the max-quote ceiling.
    /// A buy denominated in tokens out (`buy` / `buy_v2`).
    ExactBase(u64),
    /// Sell an exact BASE (token) amount; receive ≥ the min-quote floor (`sell`).
    ExactBaseIn(u64),
    /// A native SOL move of exact lamports (`TransferSol`).
    Sol(u64),
    /// An SPL token move of exact base units (`TransferToken`).
    Token(u64),
    /// No swap amount (create).
    None,
}

impl Amount {
    /// The quote (SOL lamports) this op spends, for roll-ups. `ExactBase`'s spend
    /// is a live-quote ceiling not known until send, so it contributes `0` to the
    /// static roll-up (the dry-run annotates it as computed-at-send).
    pub fn quote_spend(self) -> u64 {
        match self {
            Amount::ExactQuote(q) | Amount::Sol(q) => q,
            _ => 0,
        }
    }

    /// The base (token) units this op moves/targets, for roll-ups.
    pub fn base_amount(self) -> u64 {
        match self {
            Amount::ExactBase(b) | Amount::ExactBaseIn(b) | Amount::Token(b) => b,
            _ => 0,
        }
    }
}

/// A reference to a wallet an op acts through. `pubkey` is the base58 address
/// (always present — it's what signs/receives, and the ONE canonical identity);
/// `managed_wallet_id` links our managed-pool row when the wallet is ours (absent
/// for an external counterparty). `alias` reads plans persisted under the old
/// `wallet_id` key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WalletRef {
    #[serde(default, alias = "wallet_id", skip_serializing_if = "Option::is_none")]
    pub managed_wallet_id: Option<Uuid>,
    pub pubkey: String,
}

impl WalletRef {
    pub fn new(pubkey: impl Into<String>) -> Self {
        Self { managed_wallet_id: None, pubkey: pubkey.into() }
    }
    pub fn managed(managed_wallet_id: Uuid, pubkey: impl Into<String>) -> Self {
        Self { managed_wallet_id: Some(managed_wallet_id), pubkey: pubkey.into() }
    }
}

/// Stable per-plan op identifier used for dependency ordering (a bundler buy
/// `deps` the create; a consolidate `deps` the exits). A `u32` index into the
/// plan's op list, assigned by the builder.
pub type OpId = u32;

/// One on-chain operation — the atom of a `Plan`. The five orthogonal axes plus
/// the acting wallet, an optional target (mint for a swap, recipient for a
/// transfer), the slippage tolerance (NOT a frozen `min_out`), and its deps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operation {
    pub id: OpId,
    pub kind: OpKind,
    pub venue: VenueId,
    /// The catalog variant name (e.g. `"buy_exact_sol_in"`, `"create_v2"`). The
    /// provider rejects any name not in `catalog::valid_variants(venue)` of this
    /// `kind` — an off-catalog variant is unbuildable.
    pub variant: String,
    pub role: Role,
    pub intent: Intent,
    pub amount: Amount,
    /// Slippage tolerance in bps. `None` ⇒ `min_out = 1` (no protection, no
    /// reserve read) — kept distinct from `Some(0)`. The floor is computed at
    /// send from live reserves, never stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slippage_bps: Option<u64>,
    /// The wallet that signs/acts.
    pub wallet: WalletRef,
    /// The token mint (swap/create) or transfer recipient, base58. `None` only for
    /// ops that don't target one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Ops that must land before this one (bundler buy after create, consolidate
    /// after exits). Ids reference other ops in the same plan.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deps: Vec<OpId>,
    /// Authored ix **layout** (the operator hand-picked this leg's decoration step
    /// order via the template's `leg_structures`). `None` ⇒ the builder uses the
    /// canonical shape. Only the *shape* (wrapper order) — never the core ix bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<Vec<executor_core::DecoStep>>,
    /// When set, the disguise must NOT swap this op's authored `variant` (the
    /// operator hand-picked it). CU/price/tip still come from the persona disguise
    /// (so hand-picked legs keep anti-fingerprint fee jitter).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub lock_variant: bool,
}

impl Operation {
    /// A pump.fun token create (`create` / `create_v2`). No amount, no slippage.
    pub fn create(id: OpId, variant: &str, dev: WalletRef, mint: impl Into<String>) -> Self {
        Self {
            id,
            kind: OpKind::Create,
            venue: VenueId::PumpFun,
            variant: variant.to_string(),
            role: Role::Dev,
            intent: Intent::Create,
            amount: Amount::None,
            slippage_bps: None,
            wallet: dev,
            target: Some(mint.into()),
            deps: Vec::new(),
            layout: None,
            lock_variant: false,
        }
    }

    /// A buy leg. Role/intent distinguish a dev-buy (`Dev`/`Snipe`), a bundler buy
    /// (`Bundler`/`Snipe`), an accumulate, or a volume-make leg — same mechanism.
    #[allow(clippy::too_many_arguments)]
    pub fn buy(
        id: OpId,
        variant: &str,
        role: Role,
        intent: Intent,
        wallet: WalletRef,
        mint: impl Into<String>,
        amount: Amount,
        slippage_bps: Option<u64>,
        deps: Vec<OpId>,
    ) -> Self {
        Self {
            id,
            kind: OpKind::Buy,
            venue: VenueId::PumpFun,
            variant: variant.to_string(),
            role,
            intent,
            amount,
            slippage_bps,
            wallet,
            target: Some(mint.into()),
            deps,
            layout: None,
            lock_variant: false,
        }
    }

    /// A sell leg (`ExactBaseIn` tokens; `Exit` intent by default).
    pub fn sell(
        id: OpId,
        variant: &str,
        role: Role,
        wallet: WalletRef,
        mint: impl Into<String>,
        tokens_base: u64,
        slippage_bps: Option<u64>,
        deps: Vec<OpId>,
    ) -> Self {
        Self::sell_with(id, variant, role, Intent::Exit, wallet, mint, tokens_base, slippage_bps, deps)
    }

    /// A sell leg with an explicit intent — an `Exit` close vs a `MakeVolume`
    /// churn leg are the same mechanism (`Sell`), differing only in *why*.
    #[allow(clippy::too_many_arguments)]
    pub fn sell_with(
        id: OpId,
        variant: &str,
        role: Role,
        intent: Intent,
        wallet: WalletRef,
        mint: impl Into<String>,
        tokens_base: u64,
        slippage_bps: Option<u64>,
        deps: Vec<OpId>,
    ) -> Self {
        Self {
            id,
            kind: OpKind::Sell,
            venue: VenueId::PumpFun,
            variant: variant.to_string(),
            role,
            intent,
            amount: Amount::ExactBaseIn(tokens_base),
            slippage_bps,
            wallet,
            target: Some(mint.into()),
            deps,
            layout: None,
            lock_variant: false,
        }
    }

    /// A typed native-SOL move — the ONE shape both funding-out (`Fund`) and
    /// consolidate-in (`Consolidate`) use, replacing the three raw
    /// `system_instruction::transfer` bypasses (auditable, never bare ixs). The
    /// acting (signing) wallet is `from`, so its role defaults to `Treasury` (a
    /// treasury→pool fund); a consolidate sweeps a *used* wallet whose role the
    /// caller knows — use [`Operation::transfer_sol_as`] to record it.
    pub fn transfer_sol(
        id: OpId,
        variant: &str,
        intent: Intent,
        from: WalletRef,
        to: impl Into<String>,
        lamports: u64,
        deps: Vec<OpId>,
    ) -> Self {
        Self::transfer_sol_as(id, variant, Role::Treasury, intent, from, to, lamports, deps)
    }

    /// A native-SOL move recording the acting wallet's `role` explicitly — a
    /// consolidate sweep signs as the *swept* wallet (`Volume`/`Bundler`), which
    /// the auditor reads when it looks for a star-shaped own-graph funding tell.
    #[allow(clippy::too_many_arguments)]
    pub fn transfer_sol_as(
        id: OpId,
        variant: &str,
        role: Role,
        intent: Intent,
        from: WalletRef,
        to: impl Into<String>,
        lamports: u64,
        deps: Vec<OpId>,
    ) -> Self {
        Self {
            id,
            kind: OpKind::TransferSol,
            venue: VenueId::PumpFun,
            variant: variant.to_string(),
            role,
            intent,
            amount: Amount::Sol(lamports),
            slippage_bps: None,
            wallet: from,
            target: Some(to.into()),
            deps,
            layout: None,
            lock_variant: false,
        }
    }

    /// Attach an authored ix layout + lock the variant against disguise swap — the
    /// hand-picked-leg path (`leg_structures`). Chained onto a `buy` op the operator
    /// authored a recipe for. `lock` reflects an authored variant; the layout is the
    /// authored decoration order (validated at the gate before send).
    pub fn with_authored(mut self, layout: Option<Vec<executor_core::DecoStep>>, lock: bool) -> Self {
        self.layout = layout;
        self.lock_variant = lock;
        self
    }
}

/// A monotonic op-id allocator so macros can be composed into one plan without id
/// collisions — `fund` then `bundle_launch` then `exit` all draw from the same
/// sequence, and each op's `deps` reference ids from the same space.
#[derive(Debug, Clone)]
pub struct IdSeq(OpId);

impl IdSeq {
    /// Start allocating from `start` (usually `0` for a fresh plan).
    pub fn new(start: OpId) -> Self {
        IdSeq(start)
    }
    /// The next unused id.
    pub fn next(&mut self) -> OpId {
        let id = self.0;
        self.0 += 1;
        id
    }
    /// The id that will be handed out next (without consuming it).
    pub fn peek(&self) -> OpId {
        self.0
    }
}

impl Default for IdSeq {
    fn default() -> Self {
        IdSeq(0)
    }
}

/// **Timing / bundling** — a typed INPUT from the forge unlinkability workstream.
/// `bundle` groups
/// ops that submit atomically (Jito bundle); `delay_ms` spaces the rest. Not
/// generated here (Phase D personas draw from it).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schedule {
    pub slots: Vec<ScheduleSlot>,
}

/// One op's scheduling: which bundle it belongs to (if any) and its send delay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleSlot {
    pub op_id: OpId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle: Option<u32>,
    #[serde(default)]
    pub delay_ms: u64,
}

/// A batch of operations plus its schedule — the single object the whole write
/// side runs on. Persisted (generalizing `bundles.legs` JSONB) for preview/replay,
/// audited before execution, and (Phase F) built into txs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// The token this plan concerns, base58 (a launch's fresh mint, a manage
    /// action's existing mint). `None` for a mint-less plan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mint_address: Option<String>,
    pub ops: Vec<Operation>,
    #[serde(default)]
    pub schedule: Schedule,
}

impl Plan {
    /// A plan concerning one mint with the given ops (schedule default — the
    /// unlinkability workstream fills it).
    pub fn for_mint(mint_address: impl Into<String>, ops: Vec<Operation>) -> Self {
        Self {
            mint_address: Some(mint_address.into()),
            ops,
            schedule: Schedule::default(),
        }
    }

    /// Find an op by id (dep resolution / audit).
    pub fn op(&self, id: OpId) -> Option<&Operation> {
        self.ops.iter().find(|o| o.id == id)
    }
}
