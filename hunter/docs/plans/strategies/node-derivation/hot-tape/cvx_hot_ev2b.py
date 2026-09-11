"""Hot-tape node, step 37: is the second leg's dip-buy conditioned on the OPERATOR already being in?

Step 36 spells 49uohd's picks as public tape state and every gate is red (-4.2 to -5.1 %/trade;
the terms do not lift the book as E's did), while our seat on its actual picks books +3.50 %.
49uohd also reacts to its partner's print at 25-50 ms (lift 6.4), and the two share one recipe
set: the second leg may add on a dip only while the operator already holds the coin. Spelled
without a wallet (7.4 law 20), as recipe facts at the sell:

  op_recent   a print of the operator's recipe set (the trade-ix's the two legs use, <= 66
              wallets each) bought this coin in the last 30 s
  op_hold     a wallet of that recipe set holds the coin now
  one_op_30   any one-operator recipe (<= 50 wallets, >= 200 prints) bought in the last 30 s

(1) within-coin rank of the sells 49uohd buys against those it ignores, and (2) money on the full
tape: every public sell >= 1 SOL gated on each term, own occupancy, cap15 and the bracket, with
and without the established-coin permission. The operator's own prints are node prints and never
trigger; they only feed the recipe terms.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd

from kernel import K, fill_idx, net
from cvx import DAY0, cell, show
from tape import Tape
from cvx_hot_event import node_ids
from cvx_hot_which import strat_rank
from cvx_hot_exit7 import EXITS, run_exit

B = 0.2
LAG = 0.115
BRACKET = EXITS[0]
FEATS = ["op_recent", "op_hold", "one_op_30", "t_op", "mv10"]
GATES = [("sell>=1", None, 0, 0.0), ("+ op_recent", "op_recent", 0, 0.0),
         ("+ op_hold", "op_hold", 0, 0.0), ("+ one_op_30", "one_op_30", 0, 0.0),
         ("+ op_recent + established", "op_recent", 368, 158.0),
         ("+ op_hold + established", "op_hold", 368, 158.0),
         ("+ one_op_30 + established", "one_op_30", 368, 158.0)]

pd.set_option("display.width", 490)
pd.set_option("display.max_rows", 400)


def main() -> None:
    t0 = time.time()
    lab = node_ids()
    inv = {v: k for k, v in lab.items()}
    wA, wB = inv["AbQcLH"], inv["49uohd"]
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id", "build"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    days = (T.t.max() - T.t.min()) / 86400.0
    is_node = np.isin(T.wallet, list(lab))
    nb = len(T.builds)
    cnt = np.bincount(T.build, minlength=nb).astype(np.int64)
    wpb = pd.DataFrame({"b": T.build, "w": T.wallet}).drop_duplicates()
    nwal = np.bincount(wpb.b.to_numpy(), minlength=nb).astype(np.int64)
    is_op_b = (cnt >= 200) & (nwal <= 50)
    opset = np.unique(T.build[np.isin(T.wallet, [wA, wB])])
    opset = opset[nwal[opset] <= 66]
    is_opset_b = np.zeros(nb, dtype=bool); is_opset_b[opset] = True
    print("operator recipe set: %d trade-ix's, %s wallets on them"
          % (len(opset), f"{int(wpb[wpb.b.isin(opset)].w.nunique()):,}"), flush=True)
    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()

    rows = []; fires = []
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
        mine = is_node[a:b]; pub = ~mine
        big = pub & (side == -1) & (sol >= 1.0) & (age >= 60.0)
        if not big.any():
            continue
        vbef = v - sol * np.where(side == 1, 1.0, -1.0)
        with np.errstate(divide="ignore", invalid="ignore"):
            tokd = np.where(side == 1, K / vbef - K / v, K / v - K / vbef)
        tokd = np.nan_to_num(tokd, nan=0.0, posinf=0.0, neginf=0.0)
        mv = np.concatenate(([0.0], ((v[1:] / v[:-1]) ** 2 - 1.0) * 100.0))
        j3 = np.searchsorted(tm, tm - 3.0, side="left")
        j10 = np.searchsorted(tm, tm - 10.0, side="left")
        cbp = np.concatenate(([0.0], np.cumsum(np.where(pub & (side == 1), sol, 0.0))))
        cb3 = cbp[np.arange(n) + 1] - cbp[j3]
        vprev = np.concatenate(([np.nan], v[:-1]))
        mv10 = ((vprev / v[np.maximum(j10 - 1, 0)]) ** 2 - 1.0) * 100.0
        opb = is_opset_b[bu] & (side == 1)
        oneb = is_op_b[bu] & (side == 1)
        t_opb = pd.Series(np.where(opb, tm, np.nan)).ffill().shift(1).to_numpy()
        t_one = pd.Series(np.where(oneb, tm, np.nan)).ffill().shift(1).to_numpy()
        # operator-set wallets holding, BEFORE each print
        ophold = np.zeros(n, dtype=np.int32)
        pos = {}; nh = 0; hc = np.zeros(n, dtype=np.int32); holders = 0; opw = set()
        for i in range(n):
            ophold[i] = nh; hc[i] = holders
            w = wal[i]
            if is_opset_b[bu[i]]:
                opw.add(w)
            p = pos.get(w, 0.0)
            q = p + tokd[i] if side[i] == 1 else max(p - tokd[i], 0.0)
            if w in opw:
                nh += int(q > 0) - int(p > 0)
            if not mine[i]:
                holders += int(q > 0) - int(p > 0)
            pos[w] = q
        t_op = tm - t_opb
        f_rec = np.nan_to_num(t_op, nan=1e9) <= 30.0
        f_hold = ophold > 0
        f_one = np.nan_to_num(tm - t_one, nan=1e9) <= 30.0
        bt = tm[(wal == wB) & (side == 1)]
        if len(bt):
            for k in np.nonzero(big)[0]:
                q = int(np.searchsorted(bt, tm[k] + 0.1, side="left"))
                hit = int(q < len(bt) and bt[q] - tm[k] <= 0.5)
                rows.append((r, hit, float(f_rec[k]), float(f_hold[k]), float(f_one[k]),
                             float(min(np.nan_to_num(t_op[k], nan=1e4), 1e4)), float(mv10[k])))
        for gi, (nm, col, hmin, amin) in enumerate(GATES):
            ok = big & (hc >= hmin) & (age >= amin)
            if col:
                ok &= {"op_recent": f_rec, "op_hold": f_hold, "one_op_30": f_one}[col]
            last = -1
            for k in np.nonzero(ok)[0]:
                k = int(k)
                if k <= last:
                    continue
                ei = fill_idx(t, k, LAG)
                jx = max(int(np.searchsorted(t, t[ei] + 15.0, side="right") - 1), ei)
                x15 = fill_idx(t, jx, LAG)
                yb, _, _, _ = run_exit(BRACKET, t, v, side, sol, mv, cb3, ei)
                fires.append((r, gi, int(T.day[a + k]), net(float(v[ei]), float(v[x15]), B), yb))
                last = x15
    D = pd.DataFrame(rows, columns=["run", "hit"] + FEATS)
    C = D[D.hit == 1]; Kc = D[(D.hit == 0) & D.run.isin(set(C.run))]
    print("\nsells >= 1 on 49uohd's coins %s, it buys %s  %ds" % (f"{len(D):,}", f"{len(C):,}",
                                                                  time.time() - t0))
    print("=== within-coin rank, picked against ignored")
    print(strat_rank(C, Kc, FEATS).to_string(index=False))
    print(D.groupby("hit")[FEATS].mean().T.round(3).to_string())
    F = pd.DataFrame(fires, columns=["run", "gate", "day", "y15", "yb"])
    out = []
    for gi, g in enumerate(GATES):
        d = F[F.gate == gi]
        for yc, xl in (("y15", "cap15"), ("yb", "bracket")):
            dd = d.assign(y=d[yc])
            s = dd.y.sum(); top = dd.y.nlargest(max(1, int(round(0.01 * len(dd))))).sum()
            c = cell(dd, days=days)
            c["body"] = round(s - top, 2)
            c["top1"] = round(100 * top / s, 1) if s > 0 else np.nan
            out.append(dict(gate=g[0], exit=xl, **c))
    show(out, "every public sell >= 1 SOL gated on the operator terms, full tape")
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
