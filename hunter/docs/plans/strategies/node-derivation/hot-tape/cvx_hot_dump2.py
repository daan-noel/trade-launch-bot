"""Hot-tape node, step 25: the FRENZY-ABSORBED SELL, as a public sentence on the full tape.

Step 24 names what separates the sells 8fStGV buys from the sells it ignores, on the same coin:

  fact at the sell print                          it buys   it ignores   rank
  distinct builds printing in the last 5 s          15          7        0.757 HIGH
  public prints in the last 5 s                     35         13        0.736 HIGH
  buy SOL in the last 2 s                          3.94       0.40       0.731 HIGH
  seconds since the coin made a new high           5.4        80.3       0.308 low
  price move over the last 10 s                  +18.5 %     -0.4 %      0.649 HIGH
  seconds since the SELLER bought this coin        20.5       50.8       0.367 low

A profit-taker's sell lands INSIDE a buying frenzy: many independent machines are buying right
now, the coin is at a fresh high, and the seller is a quick flipper. On its own coins, the single
term "12+ independent builds in the last 5 s" on a sell >= 1 SOL books +1.05 %/trade on 5 of 7
days at our own 115 ms fill - but "its own coins" is a hindsight door.

This run removes the door. Full tape, every public sell >= 1 SOL at age >= 60 s, the frenzy terms
as the conjunction:

  WHO    the frenzy: many independent machines buying this coin in these seconds
  WHY    a flipper takes profit into it; the frenzy is not finished and absorbs the sell
  WHAT   a sell print of size, landing while >= N distinct builds printed in the last 5 s
  DELAY  the frenzy's next buyers have not landed 115 ms after the sell

  D  none (the diagnostic column says how much of it is 8fStGV's coin list)
  E  public sell >= 1 SOL + the frenzy terms       P  age >= 60 s . reserve band
  X  cap15 . cap60 . cap300 . trail40 c600         R one per coin, unlimited     S 0.2
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import os
import time
from pathlib import Path

import numpy as np
import pandas as pd

from kernel import K, fill_idx, net
from cvx import DAY0, cell, show
from tape import Tape
from cvx_hot_event import node_ids

B = 0.2
LAG = 0.115

pd.set_option("display.width", 490)
pd.set_option("display.max_rows", 900)
pd.set_option("display.max_columns", 60)

# name, trigger side, nb5 min, buys2 min, stall max, mv10 min, seller-hold max
GATES = [
    ("sell>=1",                              -1,  0, 0.0, 1e9, -1e9, 1e9),
    ("sell>=1 nb5>=12",                      -1, 12, 0.0, 1e9, -1e9, 1e9),
    ("sell>=1 nb5>=15",                      -1, 15, 0.0, 1e9, -1e9, 1e9),
    ("sell>=1 nb5>=12 st<=20",               -1, 12, 0.0, 20.0, -1e9, 1e9),
    ("sell>=1 nb5>=12 buys2>=2",             -1, 12, 2.0, 1e9, -1e9, 1e9),
    ("sell>=1 nb5>=12 mv10>=5",              -1, 12, 0.0, 1e9, 5.0, 1e9),
    ("sell>=1 nb5>=12 flipper<=30s",         -1, 12, 0.0, 1e9, -1e9, 30.0),
    ("sell>=1 nb5>=15 buys2>=2 st<=20",      -1, 15, 2.0, 20.0, -1e9, 1e9),
    ("sell>=1 nb5>=15 buys2>=2 st<=20 flip", -1, 15, 2.0, 20.0, -1e9, 30.0),
    ("sell>=1 nb5>=12 buys2>=2 mv10>=10",    -1, 12, 2.0, 1e9, 10.0, 1e9),
    ("buy>=1 nb5>=12 (control)",              1, 12, 0.0, 1e9, -1e9, 1e9),
    ("buy>=1 nb5>=15 buys2>=2 st<=20 (ctl)",  1, 15, 2.0, 20.0, -1e9, 1e9),
]
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
    w8 = inv["8fStGV"]
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id", "build"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    days = (T.t.max() - T.t.min()) / 86400.0
    is_node = np.isin(T.wallet, NODE)
    coins8 = set(np.unique(T.run_of[np.nonzero(T.wallet == w8)[0]]).tolist())
    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()
    nruns = len(T.start)
    lim = int(os.environ.get("HOTD2_LIMIT", "0"))
    rng = range(nruns if not lim else min(lim, nruns))
    print("tape %s prints  coins %s  %.2f days  %ds"
          % (f"{T.n:,}", f"{nruns:,}", days, time.time() - t0), flush=True)

    rows = []
    for r in rng:
        a, b = T.start[r], T.end[r]
        n = b - a
        if n < 60 or not np.isfinite(c_s[r]):
            continue
        t = T.t[a:b]; v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]
        wal = T.wallet[a:b]; bu = T.build[a:b]
        tm = np.maximum.accumulate(t)
        age = tm - c_s[r]
        if age[-1] < 60.0:
            continue
        pub = ~is_node[a:b]
        big = pub & (sol >= 1.0) & (age >= 60.0)
        if not big.any():
            continue
        vbef = v - sol * np.where(side == 1, 1.0, -1.0)
        vprev = np.concatenate(([np.nan], v[:-1]))
        j2 = np.searchsorted(tm, tm - 2.0, side="left")
        j5 = np.searchsorted(tm, tm - 5.0, side="left")
        j10 = np.searchsorted(tm, tm - 10.0, side="left")
        rmax = np.maximum.accumulate(v)
        newhigh = np.concatenate(([True], v[1:] > rmax[:-1]))
        t_nh = pd.Series(np.where(newhigh, t, np.nan)).ffill().to_numpy()
        stall = np.concatenate(([0.0], t[1:] - t_nh[:-1]))
        mv10 = pct(vprev, vprev[np.maximum(j10, 1)])
        cb = np.concatenate(([0.0], np.cumsum(np.where(side == 1, sol, 0.0))))
        buys2 = cb[np.arange(n)] - cb[j2]

        # the seller's last buy on this coin, for every print (a Python pass, cheap per coin)
        lastbuy = {}
        s_hold = np.full(n, np.nan)
        for i in range(n):
            w = wal[i]
            if side[i] == -1:
                lb = lastbuy.get(w)
                if lb is not None:
                    s_hold[i] = tm[i] - lb
            else:
                lastbuy[w] = tm[i]

        cand_all = np.nonzero(big)[0]
        nb5 = np.zeros(n)
        for k in cand_all:
            nb5[k] = len(np.unique(bu[j5[k]:k]))

        for gi, (gn, sd, n5, b2, st, m10, fl) in enumerate(GATES):
            m = big & (side == sd) & (nb5 >= n5) & (buys2 >= b2) & (stall <= st) \
                & (np.nan_to_num(mv10, nan=-1e9) >= m10)
            if fl < 1e8:
                m &= np.nan_to_num(s_hold, nan=1e9) <= fl
            cand = np.nonzero(m)[0]
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
                rows.append((r, int(T.day[a + k]), gi, float(vprev[k]), float(age[k]),
                             int(r in coins8),
                             y["cap15"], y["cap60"], y["cap300"], y["tr40_c600"]))
        if r % 20000 == 0:
            print("  run %s  fires %s  %ds" % (f"{r:,}", f"{len(rows):,}", time.time() - t0),
                  flush=True)

    F = pd.DataFrame(rows, columns=["run", "day", "gi", "v", "age", "c8",
                                    "y_cap15", "y_cap60", "y_cap300", "y_tr40"])
    F.to_parquet(data_file("cvx_hot_dump2.parquet"), index=False)
    print("\nfires %s over %s coins  %ds"
          % (f"{len(F):,}", f"{F.run.nunique():,}", time.time() - t0), flush=True)

    def full(d, ycol):
        dd = d.copy(); dd["y"] = dd[ycol]
        s = dd.y.sum()
        top = dd.y.nlargest(max(1, int(round(0.01 * len(dd))))).sum()
        c = cell(dd, days=days)
        c["top1"] = round(100 * top / s, 1) if s > 0 else np.nan
        c["body"] = round(s - top, 2)
        c["maxcoin"] = round(100 * dd.groupby("run").y.sum().max() / s, 1) if s > 0 else np.nan
        return c

    rows2 = []
    for gi, g in enumerate(GATES):
        s = F[F.gi == gi]
        if len(s) < 200:
            continue
        for ycol, lbl in (("y_cap15", "cap15"), ("y_cap60", "cap60"), ("y_cap300", "cap300"),
                          ("y_tr40", "trail40")):
            rows2.append(dict(gate=g[0], exit=lbl, **full(s, ycol)))
    show(rows2, "1. FULL TAPE, age >= 60 s, no door")

    rows3 = []
    for gi, g in enumerate(GATES):
        s = F[F.gi == gi]
        for nm, m in (("on 8fStGV's coins (diag)", s.c8 == 1), ("on every other coin", s.c8 == 0)):
            ss = s[m]
            if len(ss) < 200:
                continue
            rows3.append(dict(gate=g[0], coins=nm, **full(ss, "y_cap15")))
    show(rows3, "2. how much of it is the coin list, cap15")

    rows4 = []
    for gi, g in enumerate(GATES):
        s = F[F.gi == gi]
        for nm, m in (("v<=50", s.v <= 50), ("v 50-70", (s.v > 50) & (s.v <= 70)),
                      ("v 70-85", (s.v > 70) & (s.v <= 85)), ("v>85", s.v > 85)):
            ss = s[m]
            if len(ss) < 200:
                continue
            rows4.append(dict(gate=g[0], band=nm, **full(ss, "y_cap15")))
    show(rows4, "3. reserve band, cap15")
    print("\n per-day tickets on the best gates:")
    for gi in (1, 7, 8):
        s = F[F.gi == gi]
        print("  %-40s %s" % (GATES[gi][0], s.groupby("day").size().to_dict()))
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
