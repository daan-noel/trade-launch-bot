"""Per-coin fact arrays at every print: the public tape state a term can be spelled in.

Every fact at print k is built from prints before k (and the reserve they left), so a trigger is
never inside its own window. "Public" excludes the instrument wallets' prints; windows over
recipes, buys and sells count every print, as a live rule sees them.

  Run(session, r)          arrays for one coin (run r): t, tm (monotone time), v (vsol after the
                           print), sol, side (1 buy / -1 sell), wal, bu (build_core code), age,
                           pub, vbef (vsol before), tokd (tokens moved), mv (% price move of the
                           print), rmax, stall (s since the last new high), cb / cs / cbp (cumulative
                           SOL bought, sold, publicly bought)
  run.j(w)                 index of the first print inside the last w seconds, per print
  run.recipes(k, w)        distinct build_core recipes printed in the w s before k
  run.bought(k, w)         SOL bought (any wallet) in the w s before k; run.sold(k, w) likewise
  run.holders()            public wallets whose reserve-sized bag is above zero before each print
                           (cached). NOT a holder count: a full exit leaves a positive float
                           residue, so it reads close to distinct buyers (evidence 1.22). Spell a
                           holder term from token_amount, or say distinct buyers (r1_exact.py)
  run.holders_and_seller(flag)
                           one pass over the coin: public holders BEFORE each print, and for the
                           flagged prints the seller's seconds since its last buy on the coin, its
                           profit on the coin at the pre-sell price, and the share of its bag sold
"""
from __future__ import annotations

import numpy as np
import pandas as pd

from kernel import K


class Run:
    def __init__(self, S, r: int):
        T = S.T
        a, b = T.start[r], T.end[r]
        self.r, self.a, self.b, self.n = r, a, b, b - a
        self.t = T.t[a:b]; self.v = T.v[a:b]; self.sol = T.sol[a:b]; self.side = T.side[a:b]
        self.wal = T.wallet[a:b]; self.bu = T.build[a:b]
        self.day = T.day[a:b]; self.slot = T.slot[a:b]
        self.tm = np.maximum.accumulate(self.t)
        self.age = self.tm - S.c_s[r]
        self.mine = S.is_node[a:b]; self.pub = ~self.mine
        side, v, sol, t = self.side, self.v, self.sol, self.t
        self.vbef = v - sol * np.where(side == 1, 1.0, -1.0)
        with np.errstate(divide="ignore", invalid="ignore"):
            tokd = np.where(side == 1, K / self.vbef - K / v, K / v - K / self.vbef)
        self.tokd = np.nan_to_num(tokd, nan=0.0, posinf=0.0, neginf=0.0)
        self.mv = np.concatenate(([0.0], ((v[1:] / v[:-1]) ** 2 - 1.0) * 100.0))
        self.vprev = np.concatenate(([v[0]], v[:-1]))
        self.rmax = np.maximum.accumulate(v)
        newhigh = np.concatenate(([True], v[1:] > self.rmax[:-1]))
        t_nh = pd.Series(np.where(newhigh, t, np.nan)).ffill().to_numpy()
        self.stall = np.concatenate(([0.0], t[1:] - t_nh[:-1]))
        self.cb = np.concatenate(([0.0], np.cumsum(np.where(side == 1, sol, 0.0))))
        self.cs = np.concatenate(([0.0], np.cumsum(np.where(side == -1, sol, 0.0))))
        self.cbp = np.concatenate(([0.0], np.cumsum(np.where(self.pub & (side == 1), sol, 0.0))))
        self._j = {}
        self._hc = None

    def holders(self) -> np.ndarray:
        """Public wallets holding the coin BEFORE each print (cached)."""
        if self._hc is None:
            self._hc = self.holders_and_seller(np.zeros(self.n, dtype=bool))[0]
        return self._hc

    def j(self, w: float) -> np.ndarray:
        if w not in self._j:
            self._j[w] = np.searchsorted(self.tm, self.tm - w, side="left")
        return self._j[w]

    def recipes(self, k: int, w: float) -> int:
        lo = int(self.j(w)[k])
        return len(np.unique(self.bu[lo:k])) if k > lo else 0

    def wallets(self, k: int, w: float) -> int:
        lo = int(self.j(w)[k])
        return len(np.unique(self.wal[lo:k])) if k > lo else 0

    def bought(self, k: int, w: float) -> float:
        return float(self.cb[k] - self.cb[int(self.j(w)[k])])

    def sold(self, k: int, w: float) -> float:
        return float(self.cs[k] - self.cs[int(self.j(w)[k])])

    def pub_bought_incl(self, w: float) -> np.ndarray:
        """Public SOL bought in the w s up to and including each print (the ride exit's input)."""
        return self.cbp[np.arange(self.n) + 1] - self.cbp[self.j(w)]

    def move(self, k: int, w: float) -> float:
        """% price move over the w s before print k (reserve left by k-1 against the window start)."""
        lo = int(self.j(w)[k])
        return float((self.vprev[k] / self.v[max(lo - 1, 0)]) ** 2 - 1.0) * 100.0

    def holders_and_seller(self, flag: np.ndarray):
        n = self.n; wal = self.wal; side = self.side; tokd = self.tokd; tm = self.tm
        sol = self.sol; vbef = self.vbef; mine = self.mine
        lastb = {}; bsol = {}; btok = {}; pos = {}
        shold = np.full(n, np.nan); spnl = np.full(n, np.nan); sfrac = np.full(n, np.nan)
        hc = np.zeros(n, dtype=np.int32); holders = 0
        for i in range(n):
            hc[i] = holders
            w = wal[i]
            p = pos.get(w, 0.0)
            if side[i] == -1:
                if flag[i]:
                    lb = lastb.get(w)
                    if lb is not None:
                        shold[i] = tm[i] - lb
                    bt = btok.get(w, 0.0)
                    if bt > 0:
                        spnl[i] = ((vbef[i] ** 2 / K) / (bsol[w] / bt) - 1.0) * 100.0
                    if p > 0:
                        sfrac[i] = min(tokd[i] / p, 1.0)
                q = max(p - tokd[i], 0.0)
            else:
                lastb[w] = tm[i]
                bsol[w] = bsol.get(w, 0.0) + sol[i]; btok[w] = btok.get(w, 0.0) + tokd[i]
                q = p + tokd[i]
            pos[w] = q
            if not mine[i]:
                holders += int(q > 0) - int(p > 0)
        return hc, shold, spnl, sfrac
