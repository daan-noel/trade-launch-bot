"""Is a spelling his rule: hit rate and cover (derive 5.0).

One question: when the spelling happens anywhere on the market, does he buy?

  chance    a public print where the spelling turns true on a coin after being false for more
            than W slots, on every coin, counted only while he is free: not holding that coin,
            not in flight to it, under his most-open count, inside his active hours
  hit       his entry (first buy while flat) lands within W slots after the chance; the earliest
            chance inside an entry's window takes the hit, a later one there is in flight
  hit rate  hits / chances       100 % = he takes every chance: his rule
  cover     hits / his entries   how much of him it explains
  W         the end of his reaction spike (`reaction_window`), never a lag percentile

The arrays are one tape in canonical order (coin, slot, tx_index, leg_index), each coin one
contiguous run. `g` is any object carrying them (see `grid`):
  coin (int, runs contiguous), slot (int), t (s), day (int), n, ks = coin << 32 | slot (sorted),
  cstart / cend (run bounds, indexed by coin - coin_base).

An ENTRY is (coin, pos, slot): pos is the tape index of the first print AFTER his buy, so a chance
k precedes it when k < pos (for a buy on the tape, pos is that print's own index). A POSITION is
(coin, pos_in, pos_out, t_in, t_out); chances with pos_in <= k < pos_out are holding.

The proof is node-derivation/mid-tape/hr_synth.py (a fake trader with known rules) and
hr_hand.py (a hand count sharing no code with this module): re-run both after any change here.
"""
from __future__ import annotations

from types import SimpleNamespace

import numpy as np

GAP_OFF = 1800.0    # seconds with none of his trades: he is off, and no chance counts there
HITS_MIN = 30       # a hit rate stands on at least this many hits on the half it was built on


def grid(coin, slot, t, day):
    """The arrays the counter reads, from a tape's coin / slot / time / day columns."""
    coin = np.asarray(coin)
    slot = np.asarray(slot)
    chg = np.flatnonzero(np.diff(coin)) + 1
    return SimpleNamespace(coin=coin, slot=slot, t=np.asarray(t), day=np.asarray(day),
                           n=len(coin), ks=coin.astype(np.int64) * (1 << 32) + slot,
                           cstart=np.concatenate(([0], chg)), cend=np.concatenate((chg, [len(coin)])))


def edges(g, C, public, merge=0):
    """Public prints where C turns true on its coin after being false for MORE than `merge`
    slots: a flicker shorter than his reaction window is one chance, not two. merge=0 is the
    plain false -> true edge (a coin's first public print counts)."""
    idx = np.flatnonzero(public)
    c = C[idx]
    prev = np.zeros(len(idx), bool)
    same = g.coin[idx][1:] == g.coin[idx][:-1]
    prev[1:] = c[:-1] & same
    e = idx[c & ~prev]
    if merge <= 0 or len(e) == 0:
        return e
    tr = idx[c]                                   # the last TRUE public print before each edge
    j = np.searchsorted(tr, e, side="left") - 1
    lt = tr[np.maximum(j, 0)]
    near = (j >= 0) & (g.coin[lt] == g.coin[e]) & (g.slot[e] - g.slot[lt] <= merge)
    return e[~near]


def active_spans(trade_t, gap=GAP_OFF):
    """His active hours: runs of his trades (any side) with no gap longer than `gap` seconds."""
    tt = np.sort(np.asarray(trade_t))
    cut = np.flatnonzero(np.diff(tt) > gap)
    return tt[np.concatenate(([0], cut + 1))], tt[np.concatenate((cut, [len(tt) - 1]))]


def most_open(positions):
    """The most positions he held at once: his cap, read from his own book."""
    t_in, t_out = np.sort(positions[3]), np.sort(positions[4])
    return int((np.searchsorted(t_in, positions[3], side="right")
                - np.searchsorted(t_out, positions[3], side="right")).max())


def measure(g, chances, entries, positions, W, cap=None, active=None, days=None):
    """Count one spelling. chances: sorted tape indices (from `edges`). entries: (coin, pos, slot).
    positions: (coin, pos_in, pos_out, t_in, t_out). active: (lo, hi) spans. days: keep only
    chances and entries on these day indices. Returns chances, hits, entries, hit_rate, cover,
    the exclusion counts, and the counted chances with their hit flags."""
    k = np.asarray(chances)
    e_coin, e_pos, e_slot = entries
    _, p_in, p_out, t_in, t_out = positions
    if days is not None:
        k = k[np.isin(g.day[k], days)]
        keep = np.isin(g.day[np.minimum(e_pos, g.n - 1)], days)
        e_coin, e_pos, e_slot = e_coin[keep], e_pos[keep], e_slot[keep]
    why = np.zeros(len(k), np.int8)               # 0 counted, 1 holding, 2 cap, 3 off
    # positions are disjoint on a coin: the last one opened at or before k decides holding
    o = np.argsort(p_in, kind="stable")
    pin, pout = p_in[o], p_out[o]
    j = np.maximum(np.searchsorted(pin, k, side="right") - 1, 0)
    why[(k >= pin[j]) & (k < pout[j])] = 1
    if cap is not None:
        ti, to = np.sort(t_in), np.sort(t_out)
        openn = np.searchsorted(ti, g.t[k], side="right") - np.searchsorted(to, g.t[k], side="right")
        why[(why == 0) & (openn >= cap)] = 2
    if active is not None:
        lo, hi = active
        a = np.searchsorted(lo, g.t[k], side="right") - 1
        on = (a >= 0) & (g.t[k] <= hi[np.maximum(a, 0)])
        why[(why == 0) & ~on] = 3
    kc = k[why == 0]
    # each entry: the counted chances on its coin with slot >= entry slot - W and index < pos
    lo_idx = np.searchsorted(g.ks, e_coin.astype(np.int64) * (1 << 32) + (e_slot - W), side="left")
    a = np.searchsorted(kc, lo_idx, side="left")
    b = np.searchsorted(kc, e_pos, side="left")
    hit = np.zeros(len(kc), bool)
    flight = np.zeros(len(kc), bool)
    has = b > a
    hit[a[has]] = True
    for aa, bb in zip(a[has], b[has]):            # later chances in the same window: in flight
        flight[aa + 1:bb] = True
    counted = ~flight
    n_ch, n_hit, n_e = int(counted.sum()), int(hit.sum()), len(e_pos)
    return dict(chances=n_ch, hits=n_hit, entries=n_e,
                hit_rate=n_hit / n_ch if n_ch else float("nan"),
                cover=n_hit / n_e if n_e else float("nan"),
                excluded=dict(holding=int((why == 1).sum()), cap=int((why == 2).sum()),
                              off=int((why == 3).sum()), flight=int(flight.sum())),
                k=kc[counted], k_hit=hit[counted])


def luck(g, chances, entries, positions, W, coin_base=1, draws=3, seed=0, **kw):
    """The hit rate by luck: each chance moved to a random print of its own coin."""
    rng = np.random.default_rng(seed)
    k = np.asarray(chances)
    c = g.coin[k] - coin_base
    lo, hi = g.cstart[c], g.cend[c]
    return float(np.mean([measure(g, np.sort(lo + (rng.random(len(k)) * (hi - lo)).astype(np.int64)),
                                  entries, positions, W, **kw)["hit_rate"] for _ in range(draws)]))


def lags(g, chances, entries, maxw=50):
    """Slot lag from each entry back to the latest chance on its coin before it (<= maxw)."""
    e_coin, e_pos, e_slot = entries
    k = np.asarray(chances)
    b = np.searchsorted(k, e_pos, side="left") - 1
    kk = k[np.maximum(b, 0)]
    lag = e_slot - g.slot[kk]
    return lag[(b >= 0) & (g.coin[kk] == e_coin) & (lag <= maxw)]


def reaction_window(lag):
    """W = the end of his reaction spike: the last slot lag before the count falls under a tenth
    of the spike's peak. A lag percentile is wrong on a spelling that fires often: some unrelated
    chance always sits a few slots before his buy and fattens the tail (8dtx2t: spike end 1 slot,
    p95 31)."""
    h = np.bincount(lag)
    top = h.max()
    w = int(np.argmax(h))
    while w + 1 < len(h) and h[w + 1] >= 0.1 * top:
        w += 1
    return w
