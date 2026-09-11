"""Hot-tape node, step 27: on 8fStGV's coins, does the frenzy-sell event pay only when it arrives?

Splits the full event of cvx_hot_dump2.py, on the coins 8fStGV trades, by whether 8fStGV - or any of
the six - buys inside our 15 s hold. Evidence 1.12.
"""
import sys, numpy as np, pandas as pd
import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file
from kernel import fill_idx, net
from cvx import DAY0, cell
from tape import Tape
from cvx_hot_event import node_ids
lab = node_ids(); inv = {v: k for k, v in lab.items()}; w8 = inv["8fStGV"]; NODE = list(lab)
T = Tape(str(data_file("cvx_prints.parquet")), cols=["mint","slot","t_ms","reserve_lamports","amount_lamports","side","wallet_id","build"])
T.day = ((T.t - DAY0) // 86400).astype(np.int16); days = (T.t.max() - T.t.min()) / 86400.0
is_node = np.isin(T.wallet, NODE)
tok = pd.read_parquet(str(data_file("cvx_tok.parquet"))).drop_duplicates("mint").set_index("mint")
tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601") - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
c_s = tok.reindex(T.mints).c_s.to_numpy()
runs8 = np.unique(T.run_of[np.nonzero(T.wallet == w8)[0]])
rows = []
for r in runs8:
    r = int(r); a, b = T.start[r], T.end[r]; n = b - a
    if n < 60 or not np.isfinite(c_s[r]): continue
    t = T.t[a:b]; v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]; wal = T.wallet[a:b]; bu = T.build[a:b]
    tm = np.maximum.accumulate(t); age = tm - c_s[r]
    pub = ~is_node[a:b]
    big = pub & (sol >= 1.0) & (side == -1) & (age >= 60.0)
    if not big.any(): continue
    j2 = np.searchsorted(tm, tm - 2.0, side="left"); j5 = np.searchsorted(tm, tm - 5.0, side="left")
    cb = np.concatenate(([0.0], np.cumsum(np.where(side == 1, sol, 0.0)))); buys2 = cb[np.arange(n)] - cb[j2]
    rmax = np.maximum.accumulate(v); nh = np.concatenate(([True], v[1:] > rmax[:-1]))
    tnh = pd.Series(np.where(nh, t, np.nan)).ffill().to_numpy(); stall = np.concatenate(([0.0], t[1:] - tnh[:-1]))
    lastbuy = {}; sh = np.full(n, np.nan)
    for i in range(n):
        w = wal[i]
        if side[i] == -1:
            lb = lastbuy.get(w)
            if lb is not None: sh[i] = tm[i] - lb
        else: lastbuy[w] = tm[i]
    b8 = np.nonzero((wal == w8) & (side == 1))[0]; nodeb = np.nonzero(is_node[a:b] & (side == 1))[0]
    last = -1
    for k in np.nonzero(big)[0]:
        k = int(k)
        if k <= last: continue
        nb5 = len(np.unique(bu[j5[k]:k]))
        if not (nb5 >= 15 and buys2[k] >= 2 and stall[k] <= 20 and np.isfinite(sh[k]) and sh[k] <= 30): continue
        ei = fill_idx(t, k, 0.115); j = max(int(np.searchsorted(t, t[ei] + 15.0, side="right") - 1), ei); x = fill_idx(t, j, 0.115)
        y = net(float(v[ei]), float(v[x]), 0.2)
        in8 = bool(((b8 > ei) & (b8 <= x)).any()); innode = bool(((nodeb > ei) & (nodeb <= x)).any())
        rows.append((r, int(T.day[a + k]), y, int(in8), int(innode)))
        last = x
D = pd.DataFrame(rows, columns=["run", "day", "y", "in8", "innode"])
print("tight fires on 8fStGV's coins:", len(D))
for nm, m in (("all", slice(None)), ("8fStGV buys inside our 15 s hold", D.in8 == 1), ("it does not", D.in8 == 0),
              ("any of the six buys inside our hold", D.innode == 1), ("none of the six", D.innode == 0)):
    d = D if isinstance(m, slice) else D[m]
    c = cell(d, days=days); print("%-40s n=%5d  %+.2f %%/trade  days %s  share %.1f %%" % (nm, c["n"], c["pct"], c["pos"], 100 * len(d) / len(D)))
