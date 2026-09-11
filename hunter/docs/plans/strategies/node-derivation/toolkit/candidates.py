"""One candidate table per tape: every trigger print, its public facts, and its exit outcome.

A variant of E, P or R is then a mask over the table plus occupancy (book.py), so a threshold
walk books in seconds and exactly as a live rule would. Build the table under LOOSE floors that
contain the current sentence and every loosening to be tried; a loosening past a floor cannot
be read off it.

  build(S, trigger, floor=None, exit=..., actor=None, extra=None) -> DataFrame

  trigger(run) -> bool mask over the coin's prints: the candidate prints. Fires only at
                  t >= S.t_min; the caller spells the rest (side, size, public, age).
  floor(f)     -> bool on the fact dict, applied before the exit is booked (the loose floors).
  exit         an exits.X() spec, booked from a fill 115 ms after the candidate.
  actor        a wallet id: DIAGNOSTIC columns only, never a term - act (it buys <= 0.5 s after
               the candidate), act_lag (s), act_pre (that buy landed by our fill), act_in (it
               buys inside our hold).
  extra(run,k) -> dict of node-specific facts added to the row.

Standard facts (all at the candidate print k, from prints before it):
  ssize  the print's SOL             age      s since creation     hold_n  public holders
  nb2 / nb3 / nb5 / nb10             distinct recipes in the last 2 / 3 / 5 / 10 s
  buys2 / buys3 / buys5 / buys10     SOL bought in the window; sells5 / sells10 SOL sold
  np5    prints in 5 s               nw5      wallets in 5 s
  stall  s since the coin's last new high
  shold  the seller's s since its last buy on the coin   spnl  its profit on the coin (%)
  sfrac  the share of its bag this print sells
  dd     % below the running high    mvk      the print's own % move
  mv3 / mv10 / mv60                  % move over the window before
  vres   vsol after the print        nbig30   public sells >= 1 SOL in the last 30 s
Outcome: ei (fill index), x (exit fill index), v0, v1, y (net SOL at 0.2), why, hold (s).
"""
from __future__ import annotations

import numpy as np
import pandas as pd

from kernel import LAG, fill_idx
from .exits import X, run_exit
from .facts import Run

RULE_EXIT = X("tp15 sl25 t90", arm=0.15, cap=90.0)


def build(S, trigger, floor=None, exit=RULE_EXIT, actor=None, extra=None):
    T = S.T
    rows = []
    for r in range(len(T.start)):
        n = T.end[r] - T.start[r]
        if n < 60 or not np.isfinite(S.c_s[r]):
            continue
        R = Run(S, r)
        if R.age[-1] < 60.0:
            continue
        cand = trigger(R) & (R.t >= S.t_min)
        if not cand.any():
            continue
        hc, shold, spnl, sfrac = R.holders_and_seller(cand)
        cb3 = R.pub_bought_incl(3.0)
        cbig = np.concatenate(([0], np.cumsum((R.pub & (R.side == -1) & (R.sol >= 1.0))
                                              .astype(np.int64))))
        ab = R.tm[(R.wal == actor) & (R.side == 1)] if actor is not None else np.array([])
        for k in np.nonzero(cand)[0]:
            k = int(k)
            f = dict(
                run=r, k=k, day=int(R.day[k]), t=float(R.t[k]), ssize=float(R.sol[k]),
                age=float(R.age[k]), hold_n=int(hc[k]),
                nb5=R.recipes(k, 5.0), nb2=R.recipes(k, 2.0), nb3=R.recipes(k, 3.0),
                nb10=R.recipes(k, 10.0),
                buys2=R.bought(k, 2.0), buys3=R.bought(k, 3.0), buys5=R.bought(k, 5.0),
                sells5=R.sold(k, 5.0), buys10=R.bought(k, 10.0), sells10=R.sold(k, 10.0),
                np5=int(k - R.j(5.0)[k]), nw5=R.wallets(k, 5.0),
                stall=float(R.stall[k]), shold=float(shold[k]), spnl=float(spnl[k]),
                sfrac=float(sfrac[k]),
                dd=float((R.vprev[k] / R.rmax[k - 1]) ** 2 - 1.0) * 100.0 if k > 0 else 0.0,
                mvk=float(R.mv[k]), mv3=R.move(k, 3.0), mv10=R.move(k, 10.0),
                mv60=R.move(k, 60.0), vres=float(R.v[k]),
                nbig30=int(cbig[k] - cbig[int(R.j(30.0)[k])]))
            if floor is not None and not floor(f):
                continue
            ei = fill_idx(R.t, k, LAG)
            y, x, why, hold = run_exit(exit, R.t, R.v, R.side, R.sol, R.mv, cb3, ei)
            f.update(ei=ei, x=x, v0=float(R.v[ei]), v1=float(R.v[x]), y=y, why=why,
                     hold=float(hold))
            if actor is not None:
                q = int(np.searchsorted(ab, R.tm[k], side="right"))
                act = int(q < len(ab) and ab[q] - R.tm[k] <= 0.5)
                q2 = int(np.searchsorted(ab, R.tm[ei], side="right"))
                f.update(act=act, act_lag=float(ab[q] - R.tm[k]) if act else np.nan,
                         act_pre=int(act and ab[q] <= R.tm[ei]),
                         act_in=int(q2 < len(ab) and ab[q2] <= R.tm[x]))
            if extra is not None:
                f.update(extra(R, k))
            rows.append(f)
    return pd.DataFrame(rows)
