//! `swing_1` — Kill→Volume Swing-Phase strategy domain.
//!
//! Meme-coin devs manufacture price swings: early **kill-swings** (short, deep
//! near-death lows) eat sniper/launch bots, then a **volume-making phase**
//! (longer, shallower higher-lows) attracts real traders before the rug. This
//! strategy reads each token's swing chain with a single causal leg classifier
//! applied three ways — to find the kill→volume transition, to trigger entry on
//! the first volume-phase confirmed higher-low, and to detect a symmetric
//! next-kill exit. See [swing1-plan.md](../../../../swing1-plan.md).
//!
//! Pricing is the shared GMGN spot ([`crate::models::trade::TradeRow::chart_spot_price`])
//! so a leg detected offline (sweep) is the leg detected live.

pub mod classifier;
pub mod swing;
