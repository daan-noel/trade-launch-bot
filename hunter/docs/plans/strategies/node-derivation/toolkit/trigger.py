"""Excess intensity: which print class a wallet reacts to, and at what lag.

A waiting time is not a reaction time: "seconds since the last big buy" reads about a second on a
busy coin whether or not anyone reacts. The valid measurement holds the coin's own print rate:

  cases     the wallet's prints (its buys, or its closing sells)
  controls  moments on the SAME coin where it did not act:
              coin  public prints within +/- 60 s of a buy (entry triggers); `near` sets whose
                    buys: "node" (any instrument wallet's, as the hot-tape record) or "group"
                    (the group's own - tighter, it absorbs the group's own busy periods)
              hold  public prints strictly inside the same holding episode (exit triggers)
  histogram every public print in the 5 s before each case / control, by (class, lag bin)
  lift      case rate / control rate per cell. 1.00 is no relation; a SPIKE at one lag is a
            reaction to that class at that latency; the spike's position IS the reaction time

  excess_intensity(S, groups, cases="buy", controls="coin", near="node")
      -> {group: (lift, excess, n cases, n controls)}
      groups: {group name: [wallet ids]}; a wallet may sit in several groups

The wallets' own prints are excluded from every class but NODE(diag), a diagnostic that can never
be a term (law 20).
"""
from __future__ import annotations

import numpy as np
import pandas as pd

from .facts import Run

LOOKBACK = 5.0
N_CTRL = 4
CTRL_WIN = 60.0
BURST_GAP = 0.4
EDGES = np.array([0.0, 0.025, 0.050, 0.075, 0.100, 0.150, 0.200, 0.300, 0.400,
                  0.600, 0.900, 1.400, 2.200, 3.400, 5.000])
LBL = ["0-25", "25-50", "50-75", "75-100", "100-150", "150-200", "200-300", "300-400",
       "400-600", "600-900", "0.9-1.4s", "1.4-2.2s", "2.2-3.4s", "3.4-5.0s"]
CLASSES = ["any", "buy", "buy>=0.5", "buy>=1", "sell", "sell>=0.5", "sell>=1", "up>=1%", "up>=3%",
           "down>=1%", "down>=2%", "pro", "new_build", "burst_start", "NODE(diag)"]


def pro_builds(T):
    """One-operator recipes: >= 200 prints from <= 50 wallets (a bot's own trade-ix)."""
    nb = len(T.builds)
    cnt = np.bincount(T.build, minlength=nb)
    wpb = pd.DataFrame({"b": T.build, "w": T.wallet}).drop_duplicates()
    nwal = np.bincount(wpb.b.to_numpy(), minlength=nb)
    return (cnt >= 200) & (nwal <= 50)


def classes(R, is_pro_b):
    pub, side, sol, mv, bu = R.pub, R.side, R.sol, R.mv, R.bu
    seen = set(); first = np.zeros(R.n, dtype=bool)
    for i in range(R.n):
        if bu[i] not in seen:
            seen.add(bu[i]); first[i] = True
    gap = np.concatenate(([1e9], np.diff(R.tm)))
    C = np.empty((len(CLASSES), R.n), dtype=bool)
    C[0] = pub
    C[1] = pub & (side == 1); C[2] = C[1] & (sol >= 0.5); C[3] = C[1] & (sol >= 1.0)
    C[4] = pub & (side == -1); C[5] = C[4] & (sol >= 0.5); C[6] = C[4] & (sol >= 1.0)
    C[7] = pub & (mv >= 1.0); C[8] = pub & (mv >= 3.0)
    C[9] = pub & (mv <= -1.0); C[10] = pub & (mv <= -2.0)
    C[11] = pub & is_pro_b[bu]
    C[12] = pub & first
    C[13] = pub & (gap >= BURST_GAP) & (side == 1)
    C[14] = R.mine
    return C


def _closes(R, w):
    """(close index, first buy index) of each position of wallet w on this coin."""
    out = []; p = 0.0; peak = 0.0; e0 = -1
    for s in np.nonzero(R.wal == w)[0]:
        s = int(s)
        if R.side[s] == 1:
            if p <= 0.0:
                e0 = s; peak = 0.0
            p += R.tokd[s]; peak = max(peak, p)
            continue
        if p <= 0.0 or e0 < 0:
            continue
        p = max(p - R.tokd[s], 0.0)
        if p <= 0.02 * peak:
            p = 0.0
            out.append((s, e0))
    return out


def excess_intensity(S, groups, cases="buy", controls="coin", near="node", seed=20260911):
    T = S.T
    is_pro_b = pro_builds(T)
    rng = np.random.default_rng(seed)
    nC, nL = len(CLASSES), len(LBL)
    H = {g: np.zeros((nC, nL)) for g in groups}; Hc = {g: np.zeros((nC, nL)) for g in groups}
    nk = {g: 0 for g in groups}; nc = {g: 0 for g in groups}
    wset = {int(w) for ws in groups.values() for w in ws}
    for r in np.unique(T.run_of[np.isin(T.wallet, list(wset))]):
        r = int(r)
        if T.end[r] - T.start[r] < 30:
            continue
        R = Run(S, r)
        C = classes(R, is_pro_b)

        def acc(Hd, g, i):
            j = int(np.searchsorted(R.tm, R.tm[i] - LOOKBACK, side="left"))
            if j >= i:
                return
            bi = np.searchsorted(EDGES, R.tm[i] - R.tm[j:i], side="right") - 1
            ok = (bi >= 0) & (bi < nL)
            bi = bi[ok]
            for c in range(nC):
                m = C[c, j:i][ok]
                if m.any():
                    np.add.at(Hd[g][c], bi[m], 1.0)

        for g, ws in groups.items():
            if cases == "buy":
                ks = np.nonzero(np.isin(R.wal, ws) & (R.side == 1))[0]
                if not len(ks):
                    continue
                around = np.nonzero(R.mine & (R.side == 1))[0] if near == "node" else ks
                win = np.zeros(R.n, dtype=bool)
                for k in around:
                    lo = np.searchsorted(R.tm, R.tm[k] - CTRL_WIN, side="left")
                    hi = np.searchsorted(R.tm, R.tm[k] + CTRL_WIN, side="right")
                    win[lo:hi] = True
                pool = np.nonzero(win & R.pub)[0]
                if len(pool) < 8:
                    continue
                for k in ks:
                    acc(H, g, int(k)); nk[g] += 1
                for k in rng.choice(pool, size=min(len(pool), N_CTRL * len(ks)), replace=False):
                    acc(Hc, g, int(k)); nc[g] += 1
            else:
                for w in ws:
                    for s, e0 in _closes(R, w):
                        acc(H, g, s); nk[g] += 1
                        inside = np.nonzero(R.pub[e0 + 1:s])[0] + e0 + 1
                        if controls == "hold" and len(inside):
                            for k in rng.choice(inside, size=min(len(inside), N_CTRL),
                                                replace=False):
                                acc(Hc, g, int(k)); nc[g] += 1
    out = {}
    for g in groups:
        case = H[g] / max(nk[g], 1); ctrl = Hc[g] / max(nc[g], 1)
        with np.errstate(divide="ignore", invalid="ignore"):
            lift = np.where(ctrl > 0, case / ctrl, np.nan)
        out[g] = (pd.DataFrame(lift, index=CLASSES, columns=LBL).round(2),
                  pd.DataFrame(case - ctrl, index=CLASSES, columns=LBL).round(3), nk[g], nc[g])
    return out


def peak(lift, cls):
    """(lag bin, lift) of a class's highest cell."""
    row = lift.loc[cls]
    return row.idxmax(), float(row.max())
