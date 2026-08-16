//! Family search — a **from-scratch** lab job that finds, for one launch style, the
//! metric combination that works on both sides (entry and exit) across a fingerprint's
//! sibling family.
//!
//! Sibling of [rule search](crate::rule_search), never a rewrite of it: nothing here
//! modifies that module, its handler, or its report, and every change to shared sweep
//! code is additive — a new opt-in field, a new function, a new variant.
//!
//! The load-bearing constraint is charter D5: **the result depends on no existing
//! rule.** Delete every row in the `rules` table and the output is identical. Buy
//! size, caps, fill, cost and the copycat setting all come from the request; a
//! promoted rule may not supply any of them, because cost is U-shaped under
//! `pumpfun_impact` and the caps change which tokens are entered at all.
//!
//! Charter: [family-search.md] · plan: [family-search-plan.md].
//!
//! [family-search.md]: ../../docs/roadmap/family-search.md
//! [family-search-plan.md]: ../../docs/roadmap/family-search-plan.md

pub mod attribution;
pub mod gates;
pub mod oracle;

#[cfg(test)]
pub mod fixtures;
