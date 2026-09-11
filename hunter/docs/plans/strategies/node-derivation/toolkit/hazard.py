"""An actor's closing hazard: when does it sell, by profit since entry x time held?

Law 26: an exit is read at the actor's own hold, on the pool the sentence selects - never on an
unselected pool, which is mostly dying coins and returns the shortest clock. For every public
print inside each of its holds, the chance its NEXT print is the closing sell; alongside, the
shape of its closes (profit, time held, peak, and the split into take-profit / stop / between).

  closing_hazard(S, wallet_id, pool)  pool(R, e0) -> label for the hold starting at print e0
                                      (e.g. "P" when the permission holds there), or None to skip.
                                      Returns (closes DataFrame, {label: hazard table},
                                      {label: closes-per-cell table})

Read it at fine resolution before spelling a stop: coarse bins put a stop 5-20 points too tight.
"""
from __future__ import annotations

import numpy as np
import pandas as pd

from .facts import Run

PNL_E = np.array([-1e9, -40, -30, -25, -20, -10, 0, 5, 8, 10, 12, 15, 20, 30, 1e9])
PNL_L = ["<-40", "-40..-30", "-30..-25", "-25..-20", "-20..-10", "-10..0", "0..5", "5..8", "8..10",
         "10..12", "12..15", "15..20", "20..30", ">30"]
HLD_E = np.array([0, 5, 10, 20, 30, 45, 60, 90, 1e9])
HLD_L = ["0-5s", "5-10s", "10-20s", "20-30s", "30-45s", "45-60s", "60-90s", ">90s"]


def _pct(a, b):
    with np.errstate(divide="ignore", invalid="ignore"):
        return ((a / b) ** 2 - 1.0) * 100.0


def closing_hazard(S, wallet_id, pool, min_den=30):
    T = S.T
    HZ = {}; eps = []
    for r in np.unique(T.run_of[T.wallet == wallet_id]):
        r = int(r)
        if T.end[r] - T.start[r] < 30 or not np.isfinite(S.c_s[r]):
            continue
        R = Run(S, r)
        p8 = 0.0; peak8 = 0.0; e0 = -1
        for s in np.nonzero(R.wal == wallet_id)[0]:
            s = int(s)
            if R.side[s] == 1:
                if p8 <= 0.0:
                    e0 = s; peak8 = 0.0
                p8 += R.tokd[s]; peak8 = max(peak8, p8)
                continue
            if p8 <= 0.0 or e0 < 0:
                continue
            p8 = max(p8 - R.tokd[s], 0.0)
            if p8 > 0.02 * peak8:
                continue
            p8 = 0.0
            lab = pool(R, e0)
            if lab is None:
                continue
            v0 = float(R.v[e0])
            hold = np.nonzero(R.pub[e0 + 1:s])[0] + e0 + 1
            pk = float(_pct(R.v[e0:s].max(), v0)) if s > e0 else 0.0
            pnl = float(_pct(R.v[s - 1], v0))
            eps.append((lab, r, pnl, float(R.tm[s] - R.tm[e0]), pk))
            H = HZ.setdefault(lab, np.zeros((2, len(PNL_L), len(HLD_L))))
            pi = np.searchsorted(PNL_E, pnl, side="right") - 1
            hi = np.searchsorted(HLD_E, R.tm[s] - R.tm[e0], side="right") - 1
            if 0 <= pi < len(PNL_L) and 0 <= hi < len(HLD_L):
                H[0, pi, hi] += 1
            if len(hold):
                pr = _pct(R.v[hold - 1], v0); hl = R.tm[hold] - R.tm[e0]
                pi = np.searchsorted(PNL_E, pr, side="right") - 1
                hi = np.searchsorted(HLD_E, hl, side="right") - 1
                ok = (pi >= 0) & (pi < len(PNL_L)) & (hi >= 0) & (hi < len(HLD_L))
                np.add.at(H[1], (pi[ok], hi[ok]), 1.0)
    E = pd.DataFrame(eps, columns=["pool", "run", "pnl", "held", "peak"])
    hz = {}; cnt = {}
    for lab, (num, den) in HZ.items():
        with np.errstate(divide="ignore", invalid="ignore"):
            h = np.where(den >= min_den, 100.0 * num / (num + den), np.nan)
        hz[lab] = pd.DataFrame(h, index=PNL_L, columns=HLD_L).round(2)
        cnt[lab] = pd.DataFrame(num, index=PNL_L, columns=HLD_L).astype(int)
    return E, hz, cnt
