"""Hot-tape node, step 30: the member's OWN exit, booked on the frozen E.

Step 29 reads the closing sells of the member whose trigger is E, against random public prints of
the same hold. It sells 25-200 ms after a public BUY >= 1 SOL or a +3 % print (lift 5.8), at a
median +14.4 % since entry, 1.2 % off the in-hold peak. The hazard of its next print being the
closing sell, by profit since entry x time held, is a clean bracket:

  profit +10..+40 %          6 to 20 % a print      take profit at about +10 %, into a buy print
  profit -20..+10 %          ~0 until 40 s held     holds through the noise
  profit below -20 %         ~4 % a print           stop at -20 %
  held 40-80 s               1.4 to 2 % a print     time stop near 60 s; nothing held past 80 s

It buys the sell and sells the buy. That bracket, derived, is booked here on E - each branch
resolving to a print index and the smallest index winning (7.4 law 24), the exit fill 115 ms after
the print that trips it, and every exit with its OWN occupancy (one position per coin at a time,
so a longer exit fires on fewer prints). A few neighbours of the derived numbers are printed
beside it to show the shape, not to pick a winner.

  E  evidence 1.12 (cvx_hot_dump2 gate "sell>=1 nb5>=15 buys2>=2 st<=20 flip")
  D  none, and abs_rate <= 0.333 (step 28b: few earlier frenzy-sells made a new high)
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

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
ABS_W = 15.0
ABS_MAX = 0.333

# name, take profit (fraction), stop (fraction or None), time cap (s)
EXITS = [
    ("cap15", None, None, 15.0),
    ("cap60", None, None, 60.0),
    ("DERIVED tp10 sl20 t60", 0.10, 0.20, 60.0),
    ("tp8 sl20 t60", 0.08, 0.20, 60.0),
    ("tp12 sl20 t60", 0.12, 0.20, 60.0),
    ("tp15 sl20 t60", 0.15, 0.20, 60.0),
    ("tp10 sl15 t60", 0.10, 0.15, 60.0),
    ("tp10 sl25 t60", 0.10, 0.25, 60.0),
    ("tp10 nostop t60", 0.10, None, 60.0),
    ("tp10 sl20 t40", 0.10, 0.20, 40.0),
    ("tp10 sl20 t80", 0.10, 0.20, 80.0),
    ("tp10 sl20 t15", 0.10, 0.20, 15.0),
]

pd.set_option("display.width", 490)
pd.set_option("display.max_rows", 400)


def bracket(t, v, ei, tp, sl, cap):
    n = len(t)
    v0 = float(v[ei])
    e = int(np.searchsorted(t, t[ei] + cap, side="right"))
    if tp is not None and e > ei + 1:
        pr = (v[ei + 1:e] / v0) ** 2 - 1.0
        trip = pr >= tp
        if sl is not None:
            trip |= pr <= -sl
        hit = np.nonzero(trip)[0]
        if len(hit):
            j = ei + 1 + int(hit[0])
            x = fill_idx(t, j, LAG)
            return net(v0, float(v[x]), B), x, ("tp" if pr[hit[0]] >= tp else "sl"), t[x] - t[ei]
    j = max(min(e - 1, n - 1), ei)
    x = fill_idx(t, j, LAG)
    return net(v0, float(v[x]), B), x, "time", t[x] - t[ei]


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
    print("tape %s prints  %ds" % (f"{T.n:,}", time.time() - t0), flush=True)

    rows = []
    for r in range(len(T.start)):
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
        big = pub & (side == -1) & (sol >= 1.0)
        if not (big & (age >= 60.0)).any():
            continue
        j2 = np.searchsorted(tm, tm - 2.0, side="left")
        j5 = np.searchsorted(tm, tm - 5.0, side="left")
        cb = np.concatenate(([0.0], np.cumsum(np.where(side == 1, sol, 0.0))))
        buys2 = cb[np.arange(n)] - cb[j2]
        rmax = np.maximum.accumulate(v)
        newhigh = np.concatenate(([True], v[1:] > rmax[:-1]))
        t_nh = pd.Series(np.where(newhigh, t, np.nan)).ffill().to_numpy()
        stall = np.concatenate(([0.0], t[1:] - t_nh[:-1]))
        lastb = {}
        shold = np.full(n, np.nan)
        for i in range(n):
            if side[i] == -1:
                lb = lastb.get(wal[i])
                if lb is not None:
                    shold[i] = tm[i] - lb
            else:
                lastb[wal[i]] = tm[i]
        fz = []
        nb5a = {}
        for k in np.nonzero(big)[0]:
            k = int(k)
            nb5a[k] = len(np.unique(bu[j5[k]:k]))
            if nb5a[k] >= 12:
                fz.append(k)
        if not fz:
            continue
        ab_p = np.array(fz)
        ab_tc = np.empty(len(fz)); ab_ok = np.zeros(len(fz), dtype=bool)
        for q, p in enumerate(fz):
            e = int(np.searchsorted(t, t[p] + ABS_W, side="right"))
            ref = rmax[p - 1] if p > 0 else v[p]
            ab_ok[q] = (e > p + 1) and (float(v[p + 1:e].max()) > ref)
            ab_tc[q] = tm[p] + ABS_W
        ev = [k for k in fz if age[k] >= 60.0 and nb5a[k] >= 15 and buys2[k] >= 2.0
              and stall[k] <= 20.0 and shold[k] <= 30.0]
        if not ev:
            continue
        c8 = int(r in coins8)
        for door in (0, 1):
            for xi_, (nm, tp, sl, cap) in enumerate(EXITS):
                last = -1
                for k in ev:
                    if k <= last:
                        continue
                    if door:
                        m = (ab_p < k) & (ab_tc <= tm[k])
                        if not m.any() or ab_ok[m].mean() > ABS_MAX:
                            continue
                    ei = fill_idx(t, k, LAG)
                    y, x, why, hold = bracket(t, v, ei, tp, sl, cap)
                    rows.append((r, int(T.day[a + k]), c8, door, xi_, y, why, hold))
                    last = x
    F = pd.DataFrame(rows, columns=["run", "day", "c8", "door", "exit", "y", "why", "hold"])
    F.to_parquet(data_file("cvx_hot_exit6.parquet"), index=False)
    print("fires %s  %ds" % (f"{len(F):,}", time.time() - t0), flush=True)

    out = []
    for door in (0, 1):
        for xi_, (nm, tp, sl, cap) in enumerate(EXITS):
            d = F[(F.door == door) & (F.exit == xi_)]
            if not len(d):
                continue
            s = d.y.sum()
            top = d.y.nlargest(max(1, int(round(0.01 * len(d))))).sum()
            c = cell(d, days=days)
            c["body"] = round(s - top, 2)
            c["top1"] = round(100 * top / s, 1) if s > 0 else np.nan
            c["maxcoin"] = round(100 * d.groupby("run").y.sum().max() / s, 1) if s > 0 else np.nan
            c["hold_p50"] = round(float(d.hold.median()), 1)
            for w in ("tp", "sl", "time"):
                c[w] = round(100 * float((d.why == w).mean()), 1)
            c["its"] = round(float(d[d.c8 == 1].y.mean()) / B * 100, 2)
            c["other"] = round(float(d[d.c8 == 0].y.mean()) / B * 100, 2)
            out.append(dict(door="abs_rate<=.33" if door else "none", exit=nm, **c))
    show(out, "the frozen E under each exit, own occupancy  (its / other = %/trade on the coin "
              "list and off it, diagnostic)")
    print("\n per-day SOL, derived exit")
    for door in (0, 1):
        d = F[(F.door == door) & (F.exit == 2)]
        print("  door %d  %s" % (door, d.groupby("day").y.sum().round(2).to_dict()))
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
