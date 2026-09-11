"""Hot-tape node, step 33: the permission, booked as a sentence.

Step 32 reads which frenzies die. The facts that pass a walk-forward on days without the door all
say the same thing - the coin is already ESTABLISHED:

  public wallets holding the coin >= 368   folds +0.90 % 4/4 and +1.21 % 3/3 (cuts 368 / 369)
  age >= 123 / 193 s                       folds +0.79 % 4/4 and +1.95 % 3/3

and the stop-out rate falls from 21 % to 13-16 %. Frozen here before booking, from the folds:
holders >= 368, age >= 158 s (the mean of the two fold cuts). Each permission gates the event
BEFORE occupancy, so a skipped fire frees the coin for a later one, exactly as a live rule would.

  E  evidence 1.12      X  bracket tp10 sl25 t60 (1.13)
  D  none, or abs_rate <= 0.333      P  none, holders, age, both
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import os
import time
from pathlib import Path

import numpy as np
import pandas as pd

from kernel import K, fill_idx
from cvx import DAY0, cell, show
from tape import Tape
from cvx_hot_event import node_ids
from cvx_hot_exit7 import EXITS, run_exit

B = 0.2
LAG = 0.115
ABS_W = 15.0
ABS_MAX = 0.333
HOLD_MIN = 368
AGE_MIN = 158.0
BRACKET = EXITS[0]

CELLS = [("E", 0, 0, 0.0), ("E + holders", 0, HOLD_MIN, 0.0), ("E + age", 0, 0, AGE_MIN),
         ("E + holders + age", 0, HOLD_MIN, AGE_MIN), ("E + door", 1, 0, 0.0),
         ("E + door + holders", 1, HOLD_MIN, 0.0), ("E + door + age", 1, 0, AGE_MIN),
         ("E + door + holders + age", 1, HOLD_MIN, AGE_MIN)]

pd.set_option("display.width", 490)


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
    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()
    print("tape %s prints  %ds" % (f"{T.n:,}", time.time() - t0), flush=True)
    F = run(T, c_s, is_node, w8, CELLS)
    F.to_parquet(data_file("cvx_hot_perm2%s.parquet" % os.environ.get("PERM2_TAG", "")), index=False)
    print("fires %s  %ds" % (f"{len(F):,}", time.time() - t0), flush=True)
    report(F, days, CELLS)


def run(T, c_s, is_node, w8, cells, t_min=-np.inf, exits=None):
    """Book every cell of the sentence on tape T; fires only at t >= t_min (a holdout start).
    `exits` books each cell under each exit spec (own occupancy); default the bracket alone."""
    exits = exits or [BRACKET]
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
        mine = is_node[a:b]
        pub = ~mine
        big = pub & (side == -1) & (sol >= 1.0)
        if not (big & (age >= 60.0)).any():
            continue
        vbef = v - sol * np.where(side == 1, 1.0, -1.0)
        with np.errstate(divide="ignore", invalid="ignore"):
            tokd = np.where(side == 1, K / vbef - K / v, K / v - K / vbef)
        tokd = np.nan_to_num(tokd, nan=0.0, posinf=0.0, neginf=0.0)
        j2 = np.searchsorted(tm, tm - 2.0, side="left")
        j3 = np.searchsorted(tm, tm - 3.0, side="left")
        j5 = np.searchsorted(tm, tm - 5.0, side="left")
        cb = np.concatenate(([0.0], np.cumsum(np.where(side == 1, sol, 0.0))))
        cbp = np.concatenate(([0.0], np.cumsum(np.where(pub & (side == 1), sol, 0.0))))
        buys2 = cb[np.arange(n)] - cb[j2]
        cb3 = cbp[np.arange(n) + 1] - cbp[j3]
        mv = np.concatenate(([0.0], ((v[1:] / v[:-1]) ** 2 - 1.0) * 100.0))
        rmax = np.maximum.accumulate(v)
        newhigh = np.concatenate(([True], v[1:] > rmax[:-1]))
        t_nh = pd.Series(np.where(newhigh, t, np.nan)).ffill().to_numpy()
        stall = np.concatenate(([0.0], t[1:] - t_nh[:-1]))
        # the seller's last buy, and the public holder count BEFORE each print
        lastb = {}
        shold = np.full(n, np.nan)
        pos = {}
        hc = np.zeros(n, dtype=np.int32)
        holders = 0
        for i in range(n):
            hc[i] = holders
            w = wal[i]
            if side[i] == -1:
                lb = lastb.get(w)
                if lb is not None:
                    shold[i] = tm[i] - lb
            else:
                lastb[w] = tm[i]
            if mine[i]:
                continue
            p = pos.get(w, 0.0)
            q = p + tokd[i] if side[i] == 1 else max(p - tokd[i], 0.0)
            holders += int(q > 0) - int(p > 0)
            pos[w] = q
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
        for q_, p_ in enumerate(fz):
            e = int(np.searchsorted(t, t[p_] + ABS_W, side="right"))
            ref = rmax[p_ - 1] if p_ > 0 else v[p_]
            ab_ok[q_] = (e > p_ + 1) and (float(v[p_ + 1:e].max()) > ref)
            ab_tc[q_] = tm[p_] + ABS_W
        ev = [k for k in fz if age[k] >= 60.0 and nb5a[k] >= 15 and buys2[k] >= 2.0
              and stall[k] <= 20.0 and shold[k] <= 30.0]
        if not ev:
            continue
        c8 = int(np.any(wal == w8))
        for ci, (nm, door, hmin, amin) in enumerate(cells):
            for xi_, sp in enumerate(exits):
                last = -1
                for k in ev:
                    if k <= last or hc[k] < hmin or age[k] < amin or t[k] < t_min:
                        continue
                    if door:
                        m = (ab_p < k) & (ab_tc <= tm[k])
                        if not m.any() or ab_ok[m].mean() > ABS_MAX:
                            continue
                    ei = fill_idx(t, k, LAG)
                    y, x, why, hold = run_exit(sp, t, v, side, sol, mv, cb3, ei)
                    rows.append((r, ci, xi_, int(T.day[a + k]), y, why, c8, float(v[ei]), hold,
                                 float(v[x])))
                    last = x
    return pd.DataFrame(rows, columns=["run", "cell", "exit", "day", "y", "why", "c8", "v0",
                                       "hold", "v1"])


def report(F, days, cells):
    dl = np.sort(F.day.unique())
    h1, h2 = dl[: (len(dl) + 1) // 2], dl[(len(dl) + 1) // 2:]
    out = []
    for ci, (nm, *_rest) in enumerate(cells):
        d = F[F.cell == ci]
        s = d.y.sum()
        top = d.y.nlargest(max(1, int(round(0.01 * len(d))))).sum()
        c = cell(d, days=days)
        c["body"] = round(s - top, 2)
        c["top1"] = round(100 * top / s, 1) if s > 0 else np.nan
        c["maxcoin"] = round(100 * d.groupby("run").y.sum().max() / s, 1) if s > 0 else np.nan
        c["half1"] = round(float(d[d.day.isin(h1)].y.mean()) / B * 100, 2)
        c["half2"] = round(float(d[d.day.isin(h2)].y.mean()) / B * 100, 2)
        c["sl"] = round(100 * float((d.why == "sl").mean()), 1)
        c["its"] = round(float(d[d.c8 == 1].y.mean()) / B * 100, 2)
        c["other"] = round(float(d[d.c8 == 0].y.mean()) / B * 100, 2)
        out.append(dict(sentence=nm, **c))
    show(out, "the sentence with each permission, bracket, own occupancy  (its / other: "
              "diagnostic)")
    print("\n per-day SOL")
    for ci, (nm, *_rest) in enumerate(CELLS):
        d = F[F.cell == ci]
        print("  %-26s %s" % (nm, d.groupby("day").y.sum().round(2).to_dict()))


if __name__ == "__main__":
    main()
