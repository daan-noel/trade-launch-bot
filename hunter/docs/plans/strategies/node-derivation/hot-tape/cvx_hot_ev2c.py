"""Hot-tape node, step 38: the second leg's decision point - its partner, and the public print behind it.

Step 37: operator presence does not separate 49uohd's picks (within-coin rank 0.50-0.52); only the
fall does. 49uohd's strongest reaction is its PARTNER's print (lift 6.4 at 25-50 ms), and the
partner buys ~1 SOL buys that land after a dip (step 35). So the second leg's dip-buy may be a
follow-up to the partner's buy that happens to land near a sell. This run measures, for the sells
49uohd buys against those it ignores on the same coin, the time since (a) the partner's last buy
(diagnostic only) and (b) the last PUBLIC big buy after a dip (a buy >= 0.5 SOL opening a burst
after >= 0.4 s of silence, with the price down over the last 10 s) - the partner's own trigger,
spelled publicly. Then the money: every public sell >= 1 SOL within N s after such a big dip-buy,
full tape, own occupancy, cap15 and the bracket, with and without the established-coin permission.
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
WINS = (1.0, 3.0, 10.0)

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
    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()
    gates = [(w, h, a) for w in WINS for (h, a) in ((0, 0.0), (368, 158.0))]
    rows = []; fires = []
    for r in range(len(T.start)):
        a, b = T.start[r], T.end[r]
        n = b - a
        if n < 60 or not np.isfinite(c_s[r]):
            continue
        t = T.t[a:b]; v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]; wal = T.wallet[a:b]
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
        gap = np.concatenate(([1e9], np.diff(tm)))
        j3 = np.searchsorted(tm, tm - 3.0, side="left")
        j10 = np.searchsorted(tm, tm - 10.0, side="left")
        cbp = np.concatenate(([0.0], np.cumsum(np.where(pub & (side == 1), sol, 0.0))))
        cb3 = cbp[np.arange(n) + 1] - cbp[j3]
        vprev = np.concatenate(([np.nan], v[:-1]))
        mv10 = ((vprev / v[np.maximum(j10 - 1, 0)]) ** 2 - 1.0) * 100.0
        dipbuy = pub & (side == 1) & (sol >= 0.5) & (gap >= 0.4) & (np.nan_to_num(mv10) < 0)
        t_dip = pd.Series(np.where(dipbuy, tm, np.nan)).ffill().shift(1).to_numpy()
        since_dip = np.nan_to_num(tm - t_dip, nan=1e9)
        t_A = pd.Series(np.where((wal == wA) & (side == 1), tm, np.nan)).ffill().shift(1).to_numpy()
        since_A = np.nan_to_num(tm - t_A, nan=1e9)
        hc = np.zeros(n, dtype=np.int32); pos = {}; holders = 0
        for i in range(n):
            hc[i] = holders
            if mine[i]:
                continue
            w = wal[i]; p = pos.get(w, 0.0)
            q = p + tokd[i] if side[i] == 1 else max(p - tokd[i], 0.0)
            holders += int(q > 0) - int(p > 0); pos[w] = q
        bt = tm[(wal == wB) & (side == 1)]
        if len(bt):
            for k in np.nonzero(big)[0]:
                q = int(np.searchsorted(bt, tm[k] + 0.1, side="left"))
                hit = int(q < len(bt) and bt[q] - tm[k] <= 0.5)
                rows.append((r, hit, float(min(since_dip[k], 1e4)), float(min(since_A[k], 1e4)),
                             float(mv10[k])))
        for gi, (win, hmin, amin) in enumerate(gates):
            ok = big & (since_dip <= win) & (hc >= hmin) & (age >= amin)
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
    D = pd.DataFrame(rows, columns=["run", "hit", "since_dip", "since_A", "mv10"])
    C = D[D.hit == 1]; Kc = D[(D.hit == 0) & D.run.isin(set(C.run))]
    print("sells >= 1 on 49uohd's coins %s, it buys %s  %ds" % (f"{len(D):,}", f"{len(C):,}",
                                                                time.time() - t0))
    print(strat_rank(C, Kc, ["since_dip", "since_A", "mv10"]).to_string(index=False))
    for nm, col in (("since the last public big dip-buy", "since_dip"),
                    ("since the partner's last buy (diagnostic)", "since_A")):
        print("  %-44s picked p25/50/75 %s   ignored p25/50/75 %s   picked <=3 s %.1f %%  ignored "
              "<=3 s %.1f %%" % (nm, np.round(C[col].quantile([.25, .5, .75]).values, 2),
                                 np.round(Kc[col].quantile([.25, .5, .75]).values, 2),
                                 100 * (C[col] <= 3).mean(), 100 * (Kc[col] <= 3).mean()))
    F = pd.DataFrame(fires, columns=["run", "gate", "day", "y15", "yb"])
    out = []
    for gi, (win, hmin, amin) in enumerate(gates):
        d = F[F.gate == gi]
        for yc, xl in (("y15", "cap15"), ("yb", "bracket")):
            out.append(dict(gate="sell>=1 within %.0f s of a big dip-buy%s" %
                            (win, " + established" if hmin else ""), exit=xl,
                            **cell(d.assign(y=d[yc]), days=days)))
    show(out, "every public sell >= 1 SOL after a public big dip-buy, full tape")
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
