"""Hot-tape node, step 23: BUY THE DUMP PRINT inside a live up-move - the event step 21 named.

WHAT STEP 21 FOUND, per wallet, as excess intensity against the coin's own print rate:

  wallet   RACE cap15   reacts to                                   lag        avoids
  8fStGV   +2.28 %      a public SELL >= 1 SOL (lift 7.7-8.1)        25-200 ms  burst starts (0.01-0.23)
  AbQcLH   +2.29 %      burst start (9.93) and big prints both ways  25-50 ms
  49uohd   +2.24 %      big buys at 25-50 ms, sells at 150-300 ms    25-300 ms
  omegoM   +0.12 %      sells (5.0) and burst starts (4.7)           25-100 ms
  64hP97   +0.98 %*     nothing under 200 ms                         300-600 ms
  sssssw   -0.62 %      a public BUY >= 1 / a +3 % print (5.6-9.2)   75-100 ms  sells (0.35-0.45)
                                                        (* 8/8 days, but its body is negative)

Speed does NOT separate the members that pay - sssssw and omegoM react as fast as AbQcLH. The SIDE
does. The cleanest member buys the print that pushed price DOWN; the worst member buys the print
that pushed it UP. That is strategy 1.5's direction law read off a professional's own reaction:
buying into a flush has the price moving toward you, buying a breakout has it moving away.

And the latency is not a wall here. 8fStGV's reaction to the sell print is a PLATEAU from 25 to
200 ms, not a spike under our 115 ms.

  WHO    the momentum crowd on a coin that is still going up
  WHY    somebody just dumped a size into a live move; the move is not over
  WHAT   a public SELL print of size S, or a print that moves price down d %, on a coin up m % over
         the last minute that made a new high within s seconds
  DELAY  the rebound buyers have not landed 115 ms after the sell - 8fStGV's own reaction runs
         out to 200 ms

  D  none                 P  age >= 60 s . the up-move context . reserve band
  E  the dump print itself: fire on it, fill 115 ms later
  X  cap15 . cap60 . cap300 . trail40 c600       R one per coin      S 0.2

Also measured: firing on that sell print at our seat, do we land ahead of 8fStGV's own buy?
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import os
import time
from pathlib import Path

import numpy as np
import pandas as pd

from kernel import fill_idx, net
from cvx import DAY0, cell, show
from tape import Tape
from cvx_hot_event import node_ids

B = 0.2
LAG = 0.115

pd.set_option("display.width", 480)
pd.set_option("display.max_rows", 900)
pd.set_option("display.max_columns", 60)

# (name, kind, size-or-move threshold)
TRIG = [("sell>=0.5", "sell", 0.5), ("sell>=1", "sell", 1.0), ("sell>=2", "sell", 2.0),
        ("down<=-2%", "down", -2.0), ("down<=-4%", "down", -4.0),
        ("buy>=1 (control)", "buy", 1.0), ("up>=3% (control)", "up", 3.0)]
# (name, mv60 min, stall max, dd min)
CTX = [("none", -1e9, 1e9, -1e9), ("mv60>=10", 10.0, 1e9, -1e9),
       ("mv60>=25 st<=20", 25.0, 20.0, -1e9), ("mv60>=25 st<=20 dd>=-25", 25.0, 20.0, -25.0),
       ("mv60>=10 st<=60 dd>=-35", 10.0, 60.0, -35.0)]
EXITS = (("cap15", 15.0, None), ("cap60", 60.0, None), ("cap300", 300.0, None),
         ("tr40_c600", 600.0, 40.0))


def pct(a, b):
    with np.errstate(divide="ignore", invalid="ignore"):
        return np.where(b > 0, (a / b) ** 2 - 1.0, np.nan) * 100.0


def book_from(t, v, k, cap, trail=None):
    ei = fill_idx(t, k, LAG)
    v0 = float(v[ei])
    n = len(t)
    e = int(np.searchsorted(t, t[ei] + cap, side="right"))
    if trail is not None and e > ei + 1:
        pr = (v[ei + 1:e] / v0) ** 2
        pk = np.maximum.accumulate(np.maximum(pr, 1.0))
        hit = np.nonzero(pr <= pk * (1.0 - trail / 100.0))[0]
        if len(hit):
            x = fill_idx(t, ei + 1 + int(hit[0]), LAG)
            return net(v0, float(v[x]), B), x
    j = max(min(e - 1, n - 1), ei)
    x = fill_idx(t, j, LAG)
    return net(v0, float(v[x]), B), x


def main() -> None:
    t0 = time.time()
    lab = node_ids()
    NODE = list(lab)
    inv = {v: k for k, v in lab.items()}
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    days = (T.t.max() - T.t.min()) / 86400.0
    is_node = np.isin(T.wallet, NODE)
    w8 = inv["8fStGV"]
    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()
    nruns = len(T.start)
    lim = int(os.environ.get("HOTD_LIMIT", "0"))
    rng = range(nruns if not lim else min(lim, nruns))
    print("tape %s prints  coins %s  %.2f days  %ds"
          % (f"{T.n:,}", f"{nruns:,}", days, time.time() - t0), flush=True)

    rows = []
    seat = []
    for r in rng:
        a, b = T.start[r], T.end[r]
        if b - a < 40 or not np.isfinite(c_s[r]):
            continue
        t = T.t[a:b]; v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]
        wal = T.wallet[a:b]
        mine = is_node[a:b]
        age = t - c_s[r]
        if age[-1] < 60.0:
            continue
        tm = np.maximum.accumulate(t)
        n = len(t)
        # state strictly BEFORE the trigger print k: prints 0..k-1 and the reserve v[k-1]
        vprev = np.concatenate(([np.nan], v[:-1]))
        j60 = np.searchsorted(tm, tm - 60.0, side="left")
        mv60 = pct(vprev, vprev[np.maximum(j60, 1)])
        rmax = np.maximum.accumulate(v)
        newhigh = np.concatenate(([True], v[1:] > rmax[:-1]))
        t_nh = pd.Series(np.where(newhigh, t, np.nan)).ffill().to_numpy()
        stall = np.concatenate(([0.0], t[1:] - t_nh[:-1]))
        dd = pct(vprev, np.concatenate(([np.nan], rmax[:-1])))
        # the trigger print's own move is public the instant it lands
        mv = np.concatenate(([0.0], ((v[1:] / v[:-1]) ** 2 - 1.0) * 100.0))
        pub = ~mine
        base = pub & (age >= 60.0) & np.isfinite(mv60) & np.isfinite(dd)

        kinds = {"sell": pub & (side == -1), "buy": pub & (side == 1),
                 "down": pub & np.ones(n, dtype=bool), "up": pub & np.ones(n, dtype=bool)}
        for ti, (tn, kind, th) in enumerate(TRIG):
            if kind in ("sell", "buy"):
                tr = base & kinds[kind] & (sol >= th)
            elif kind == "down":
                tr = base & (mv <= th)
            else:
                tr = base & (mv >= th)
            if not tr.any():
                continue
            for ci, (cn, m_, s_, d_) in enumerate(CTX):
                m = tr & (mv60 >= m_) & (stall <= s_) & (dd >= d_)
                cand = np.nonzero(m)[0]
                if not len(cand):
                    continue
                last = -1
                for k in cand:
                    k = int(k)
                    if k <= last:
                        continue
                    y = {}
                    xi = 0
                    for nm, cap, trl in EXITS:
                        yy, xx = book_from(t, v, k, cap, trl)
                        y[nm] = yy
                        if nm == "cap15":
                            xi = xx
                    last = xi
                    rows.append((r, int(T.day[a + k]), ti, ci, float(vprev[k]), float(age[k]),
                                 float(sol[k]), float(mv[k]),
                                 y["cap15"], y["cap60"], y["cap300"], y["tr40_c600"]))

        # the seat against 8fStGV: its buys, and the nearest public sell >= 1 before them
        k8 = np.nonzero((wal == w8) & (side == 1))[0]
        if len(k8):
            sidx = np.nonzero(pub & (side == -1) & (sol >= 1.0))[0]
            for k in k8:
                p = int(np.searchsorted(sidx, k, side="left")) - 1
                if p < 0:
                    continue
                sj = int(sidx[p])
                lagms = 1000.0 * (tm[k] - tm[sj])
                if lagms > 300.0:
                    continue
                ei = fill_idx(t, sj, LAG)
                seat.append((r, int(T.day[a + k]), lagms, float(ei < k),
                             book_from(t, v, sj, 15.0)[0]))
        if r % 20000 == 0:
            print("  run %s  fires %s  %ds" % (f"{r:,}", f"{len(rows):,}", time.time() - t0),
                  flush=True)

    F = pd.DataFrame(rows, columns=["run", "day", "ti", "ci", "v", "age", "tsol", "tmv",
                                    "y_cap15", "y_cap60", "y_cap300", "y_tr40"])
    F.to_parquet(data_file("cvx_hot_dump.parquet"), index=False)
    print("\nfires %s over %s coins  %ds"
          % (f"{len(F):,}", f"{F.run.nunique():,}", time.time() - t0), flush=True)

    def rep(d):
        out = {}
        for ycol, lbl in (("y_cap15", "c15"), ("y_cap60", "c60"), ("y_cap300", "c300"),
                          ("y_tr40", "tr40")):
            dd = d.copy(); dd["y"] = dd[ycol]
            c = cell(dd, days=days)
            out[lbl] = c["pct"]
            out[lbl + "_d"] = c["pos"]
        return out

    rows2 = []
    for ti, (tn, _, _) in enumerate(TRIG):
        for ci, (cn, _, _, _) in enumerate(CTX):
            s = F[(F.ti == ti) & (F.ci == ci)]
            if len(s) < 300:
                continue
            rows2.append(dict(trigger=tn, context=cn, n=len(s), nday=round(len(s) / days, 1),
                              **rep(s)))
    show(rows2, "1. THE DUMP PRINT against the PUMP print, full tape, age >= 60 s   (%/trade, days+)")

    rows3 = []
    for ti, (tn, _, _) in enumerate(TRIG):
        for ci, (cn, _, _, _) in enumerate(CTX):
            s = F[(F.ti == ti) & (F.ci == ci)]
            for bn, m in (("v<=50", s.v <= 50), ("v 50-70", (s.v > 50) & (s.v <= 70)),
                          ("v 70-85", (s.v > 70) & (s.v <= 85)), ("v>85", s.v > 85)):
                ss = s[m]
                if len(ss) < 300:
                    continue
                rows3.append(dict(trigger=tn, context=cn, band=bn, n=len(ss), **rep(ss)))
    show(rows3, "2. by reserve band")

    S = pd.DataFrame(seat, columns=["run", "day", "lagms", "ahead", "y"])
    if len(S):
        print("\n=== 3. THE SEAT against 8fStGV: its buys within 300 ms of a public sell >= 1")
        print("  buys %s   its lag from the sell p25/p50/p75 = %.0f / %.0f / %.0f ms"
              % (f"{len(S):,}", S.lagms.quantile(.25), S.lagms.median(), S.lagms.quantile(.75)))
        print("  firing on that sell at our 115 ms, we land AHEAD of 8fStGV on %.1f %%"
              % (100 * S.ahead.mean()))
        show([dict(slice="all", **cell(S, days=days)),
              dict(slice="we land ahead", **cell(S[S.ahead == 1], days=days)),
              dict(slice="we land behind", **cell(S[S.ahead == 0], days=days))],
             "3. our book firing on the sell 8fStGV reacts to, cap15")
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
