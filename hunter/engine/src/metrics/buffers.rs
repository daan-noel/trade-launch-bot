//! [`Buffers`] — which per-coin buffers a set of reads needs, one list per backing
//! buffer. The rule compiler, the engine's per-coin registration and the sweep's series
//! all derive registration from this one walk, so a read can never be registered on a
//! buffer it does not read (a deque folded on every trade for nothing) or miss the one
//! it does (a reading that is `NaN` forever and looks like a strict gate).

use smallvec::SmallVec;

use super::crowd_after_age::AgeAnchor;
use super::registry::{Family, Metric};
use super::tags::TagKey;
use super::track::TokenTrack;
use super::{MetricRef, WindowSpec};

/// What a set of reads needs of one fingerprint tag.
#[derive(Debug, Clone, PartialEq)]
pub struct TagRead {
    pub key: TagKey,
    pub name: &'static str,
    /// Read trade by trade (`m_flow`, `m_holdings.profit_sol`): opens a `TagState`.
    pub trade: bool,
    /// Read at the template level (`m_slot`, `m_wave`, `m_crowd.unique_ix_templates`).
    pub template: bool,
    /// Windows read on the trade-level state.
    pub windows: SmallVec<[WindowSpec; 2]>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Buffers {
    /// Untagged flow windows (both axes of a sliced span).
    pub flow_windows: SmallVec<[WindowSpec; 2]>,
    pub price_windows: SmallVec<[WindowSpec; 2]>,
    pub crowd_windows: SmallVec<[WindowSpec; 2]>,
    pub build_windows: SmallVec<[WindowSpec; 2]>,
    /// Since-age buyer sets with the cap their reads need.
    pub crowd_anchors: SmallVec<[(AgeAnchor, u32); 1]>,
    pub tags: SmallVec<[TagRead; 2]>,
    pub print_wallet: bool,
    pub holder_book: bool,
    pub slot_state: bool,
    /// A read counts in slots or reads the slot / wave families: a loader must supply
    /// the slot column, or those readings never move.
    pub needs_slot: bool,
}

fn push(v: &mut SmallVec<[WindowSpec; 2]>, w: WindowSpec) {
    if !v.contains(&w) {
        v.push(w);
    }
}

impl Buffers {
    /// Add one read. `anchor_cap` is how many wallets a since-age buyer set must hold
    /// for the read to stay exact.
    pub fn absorb(&mut self, r: MetricRef, anchor_cap: u32) {
        self.needs_slot |= r.span.needs_slot() || matches!(r.metric.family(), Family::Slot | Family::Wave);
        match r.metric.family() {
            Family::Flow | Family::Holdings if r.is_fingerprint_scoped() => {
                let t = r.tag.expect("fingerprint-scoped");
                let entry = self.tag_entry(t.key, t.name);
                entry.trade = true;
                if let Some(w) = r.span.window {
                    push(&mut entry.windows, w);
                }
            }
            Family::Flow => {
                for w in [r.span.window, r.span.slice].into_iter().flatten() {
                    push(&mut self.flow_windows, w);
                }
            }
            Family::Holdings => self.holder_book = true,
            Family::Price => {
                if let Some(w) = r.span.window {
                    push(&mut self.price_windows, w);
                }
            }
            Family::Crowd => match r.metric {
                Metric::UniqueWallets | Metric::TradesPerWallet => {
                    if let Some(w) = r.span.window {
                        push(&mut self.crowd_windows, w);
                    }
                }
                Metric::UniqueIxShapes => {
                    if let Some(w) = r.span.window {
                        push(&mut self.build_windows, w);
                    }
                }
                Metric::BuyerCount | Metric::BuyerIsNew => {
                    if let Some(a) = r.span.since_age {
                        match self.crowd_anchors.iter_mut().find(|(x, _)| *x == a) {
                            Some((_, c)) => *c = (*c).max(anchor_cap),
                            None => self.crowd_anchors.push((a, anchor_cap)),
                        }
                    }
                }
                Metric::UniqueIxTemplates => self.template_read(r),
                _ => {}
            },
            Family::Print => self.print_wallet = true,
            Family::Slot => self.template_read(r),
            Family::Wave => {
                if r.tag.is_some() {
                    self.template_read(r);
                }
            }
            Family::State | Family::Position => {}
        }
    }

    /// The buffers of `refs`, with a fixed since-age cap (a series draws the count, it
    /// judges no threshold).
    pub fn of(refs: impl IntoIterator<Item = MetricRef>, anchor_cap: u32) -> Self {
        let mut b = Self::default();
        for r in refs {
            b.absorb(r, anchor_cap);
        }
        b
    }

    /// Merge `other` in (the union a whole rule set needs).
    pub fn union(&mut self, other: &Buffers) {
        for (src, dst) in [
            (&other.flow_windows, &mut self.flow_windows),
            (&other.price_windows, &mut self.price_windows),
            (&other.crowd_windows, &mut self.crowd_windows),
            (&other.build_windows, &mut self.build_windows),
        ] {
            for &w in src {
                push(dst, w);
            }
        }
        for &(a, cap) in &other.crowd_anchors {
            match self.crowd_anchors.iter_mut().find(|(x, _)| *x == a) {
                Some((_, c)) => *c = (*c).max(cap),
                None => self.crowd_anchors.push((a, cap)),
            }
        }
        for t in &other.tags {
            let e = self.tag_entry(t.key, t.name);
            e.trade |= t.trade;
            e.template |= t.template;
            for &w in &t.windows {
                push(&mut e.windows, w);
            }
        }
        self.print_wallet |= other.print_wallet;
        self.holder_book |= other.holder_book;
        self.slot_state |= other.slot_state;
        self.needs_slot |= other.needs_slot;
    }

    /// Register every coin-level buffer on `track`. Tags need their fingerprint's
    /// definition, so their caller registers them ([`TokenTrack::ensure_tag`]).
    pub fn ensure_on(&self, track: &mut TokenTrack) {
        for &w in &self.flow_windows {
            track.ensure_window(w);
        }
        for &w in &self.price_windows {
            track.ensure_price_window(w);
        }
        for &w in &self.crowd_windows {
            track.ensure_crowd_window(w);
        }
        for &w in &self.build_windows {
            track.ensure_build_window(w);
        }
        for &(anchor, cap) in &self.crowd_anchors {
            track.ensure_crowd_after_age(anchor, cap);
        }
        if self.print_wallet {
            track.ensure_print_wallet();
        }
        if self.holder_book {
            track.ensure_holder_book();
        }
        if self.slot_state {
            track.ensure_slot_state();
        }
    }

    fn tag_entry(&mut self, key: TagKey, name: &'static str) -> &mut TagRead {
        match self.tags.iter().position(|x| x.key == key) {
            Some(i) => &mut self.tags[i],
            None => {
                self.tags.push(TagRead { key, name, trade: false, template: false, windows: SmallVec::new() });
                self.tags.last_mut().expect("pushed")
            }
        }
    }

    fn template_read(&mut self, r: MetricRef) {
        if r.metric.family() != Family::Wave {
            self.slot_state = true;
        }
        if let Some(t) = r.tag {
            self.tag_entry(t.key, t.name).template = true;
        }
    }
}
