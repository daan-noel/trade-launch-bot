"""Hit-rate method test: the curve tape of hr_prep.py and the facts the test rules read.

The counter itself is toolkit/hitrate.py (derive 5.0); this module loads data/hr_tape.parquet in
canonical order, computes the few facts the fake trader and the 8dtx2t read use, and keeps the
per-moment lift only to show beside hit rate what it gets wrong.
"""
from __future__ import annotations

import numpy as np
import pyarrow.parquet as pq

import _paths  # noqa: F401
from _paths import DATA
from toolkit.hitrate import edges, lags, measure, reaction_window  # noqa: F401

TBIG = 1e7          # composite key coin * TBIG + t; 6 days of seconds fit under it


class Tape:
    def __init__(self, cols=("coin", "slot", "t", "buy", "sol", "v", "actor", "first_here")):
        T = pq.read_table(str(DATA / "hr_tape.parquet"), columns=list(cols))
        self.coin = T.column("coin").to_numpy().astype(np.int32)
        s = T.column("slot").to_numpy()
        self.slot0 = int(s.min())
        self.slot = (s - self.slot0).astype(np.int32)
        del s
        self.t = T.column("t").to_numpy()
        self.buy = T.column("buy").to_numpy().astype(bool)
        self.sol = T.column("sol").to_numpy()
        self.v = T.column("v").to_numpy()
        self.actor = T.column("actor").to_numpy().astype(bool)
        self.first_here = T.column("first_here").to_numpy().astype(bool)
        del T
        self.n = len(self.t)
        self.day = (self.t // 86400).astype(np.int8)
        self.kt = self.coin.astype(np.float64) * TBIG + self.t          # nondecreasing
        self.ks = self.coin.astype(np.int64) * (1 << 32) + self.slot     # nondecreasing
        chg = np.flatnonzero(np.diff(self.coin)) + 1
        self.cstart = np.concatenate(([0], chg))                         # coin c = code c - 1
        self.cend = np.concatenate((chg, [self.n]))
        same = np.zeros(self.n, bool)
        same[1:] = self.coin[1:] == self.coin[:-1]
        self.same_prev = same

    # ---- facts, all read after print k from prints up to k ------------------------------------

    def move(self):
        """This print's own price move: (vsol / vsol before)^2 - 1; first print of a coin reads 0."""
        vb = np.empty_like(self.v)
        vb[1:] = self.v[:-1]
        vb[~self.same_prev] = self.v[~self.same_prev]
        return (self.v.astype(np.float64) / vb) ** 2 - 1.0

    def gap(self, public):
        """Seconds since the previous PUBLIC print on this coin (inf for the first)."""
        idx = np.flatnonzero(public)
        g = np.full(self.n, np.inf)
        tp, cp = self.t[idx], self.coin[idx]
        gg = np.full(len(idx), np.inf)
        same = cp[1:] == cp[:-1]
        gg[1:][same] = (tp[1:] - tp[:-1])[same]
        g[idx] = gg
        return g

    def buys_in(self, public, secs):
        """PUBLIC buys on this coin in the closed window [t - secs, t], up to and including k."""
        cb = np.cumsum(public & self.buy)
        j = np.searchsorted(self.kt, self.kt - secs, side="left")
        before = np.where(j > 0, cb[np.maximum(j - 1, 0)], 0)
        return cb - before

    def edges(self, C, public, merge=0):
        return edges(self, C, public, merge)


def old_lift(T, C, public, entries):
    """The old per-moment lift: share of his entries whose last public print before them has C,
    over the share of all public prints with C."""
    e_coin, e_pos, e_slot = entries
    pub = np.flatnonzero(public)
    b = np.searchsorted(pub, e_pos, side="left") - 1
    kk = pub[np.maximum(b, 0)]
    at_entry = C[kk].mean()
    base = C[public].mean()
    return at_entry / base if base else float("nan"), at_entry, base

