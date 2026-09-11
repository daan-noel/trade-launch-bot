"""Hot-tape node, step 4: book the LULL-then-WAKE event on the whole tape.

The event, derived within-mint in steps 2-3 and stated in the coin's OWN units:

    wake_z    >= W    the last 2 s are busier than this coin's own last 60 s have been
    quiet60_z <= Q    and that last minute was unusually QUIET for this coin
    buy2_z    >= B    with real public SOL buying in those 2 s
    dd_peak   <= D    the coin sits below its own peak
    age       >= A

Within-coin lift 4.2-4.5 against non-buy prints on the same coin, which is the design that
holds the door, the narrative and every off-chain factor constant (evidence 1.5).

TWO THINGS THIS RUN HAS TO SETTLE, and they pull against each other:

  * DWELL FAILED. The condition is continuously true for a median of only 0.216 s - a fraction
    of a slot - so it is a print anchor, and evidence 1.4 prices that at -5.59 pp.
  * BUT WE ARE EARLY. The node reacts 0.4-2.2 s after a public trigger (1.5), so a rule firing
    at the edge lands well AHEAD of them, and their own buying afterwards pushes price our way.

The first says reactive is expensive, the second says being reactive still puts us in front of
the people who make this trade work. Only the book decides, so the book is measured with the
REACTION COST reported beside it: the price move from the state a watcher held to the price we
actually fill at 115 ms.

D none yet   P none yet   R one position per coin   S 0.2 SOL   seat lag_115 both legs
X six families, including the ~20 s hold this node actually runs.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd
import pyarrow.parquet as pq

from cvx import B, DAY0, cell, show
from kernel import book, fill_idx
from tape import Tape
from cvx_hot_event import expanding_z, features, node_ids

ROUTERS = ("Axiom", "Terminal", "GMGN", "Photon", "Bloom", "Trojan", "BullX", "Padre", "DFlow")

EXITS = {
    "cap20": dict(cap=20.0), "cap60": dict(cap=60.0), "cap120": dict(cap=120.0),
    "tp10_c60": dict(tp=10.0, cap=60.0), "tp20_c120": dict(tp=20.0, cap=120.0),
    "tr20_c120": dict(trail=20.0, cap=120.0), "tr30_c300": dict(trail=30.0, cap=300.0),
    "shipped": dict(arm=21.0, trail=36.0, stop=43.75, cap=1200.0),
}
CELLS = {
    #        wake_z quiet60_z buy2_z dd_peak  age
    "E2": (1.0, 0.0, 0.5, 99.0, 0.0),
    "E3": (1.0, 0.0, 0.5, -20.0, 0.0),
    "E5": (2.0, 0.0, 0.5, -20.0, 120.0),
    "E6": (2.0, -0.3, 1.0, -20.0, 120.0),
    "E0": (1.0, 99.0, -99.0, 99.0, 0.0),      # control: wake alone, no lull term
}

pd.set_option("display.width", 430)
pd.set_option("display.max_rows", 400)
pd.set_option("display.max_columns", 40)


def main() -> None:
    t0 = time.time()
    lab = node_ids()
    NODE = list(lab)
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id", "build"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    days = (T.t.max() - T.t.min()) / 86400.0
    ix = pq.read_table(str(data_file("cvx_ix.parquet")), columns=["tool"])
    router_all = ix.column("tool").to_pandas().isin(ROUTERS).to_numpy()
    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()
    sw = pd.read_parquet(data_file("cvx_swdoor.parquet")).drop_duplicates("mint").set_index("mint")
    cbuild = sw.reindex(T.mints).c_build.fillna("?").to_numpy()
    is_node = np.isin(T.wallet, NODE)
    print("tape %s prints  tokens %s  days %.2f  %ds"
          % (f"{T.n:,}", f"{len(T.mints):,}", days, time.time() - t0), flush=True)

    rows = {(c, x): [] for c in CELLS for x in EXITS}

    for r in range(len(T.start)):
        a, b = T.start[r], T.end[r]
        n = b - a
        if n < 30 or not np.isfinite(c_s[r]):
            continue
        t = T.t[a:b]
        age = t - c_s[r]
        if age[-1] < 60.0:
            continue
        v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]; sl = T.slot[a:b]
        mine = is_node[a:b]
        f = features(t, v, sol, side, age, mine, router_all[a:b])
        for c in ("wake", "quiet60", "buy2"):
            f[c + "_z"] = expanding_z(f[c])
        vbef = v - sol * np.where(side == 1, 1.0, -1.0)
        day = T.day[a:b]

        for cname, (wz, qz, bz, dd, ag) in CELLS.items():
            m = ((f["wake_z"] >= wz) & (f["quiet60_z"] <= qz) & (f["buy2_z"] >= bz)
                 & (f["dd_peak"] <= dd) & (f["age"] >= ag))
            m = np.nan_to_num(m, nan=False).astype(bool)
            # fire on the RISING EDGE only - the moment the state turns true
            edges = np.nonzero(m & ~np.concatenate(([False], m[:-1])))[0]
            if not len(edges):
                continue
            for xname, xspec in EXITS.items():
                last = -1
                acc = rows[(cname, xname)]
                for k in edges:
                    k = int(k)
                    if k <= last or k < 2 or k >= n - 1:
                        continue
                    y, ei, xi, reason = book(t, v, sl, k, xspec, lag=0.115, B=B)
                    # reaction cost: the state a watcher held at k-1 against our actual fill
                    react = (v[ei] / vbef[k]) ** 2 - 1.0
                    acc.append((y, r, int(day[k]), reason, str(cbuild[r]), float(react),
                                float(t[xi] - t[ei]), float(v[ei]), float(age[k])))
                    last = xi
        if r % 20000 == 0:
            print("  run %d  E3 fires %s  %ds"
                  % (r, f"{len(rows[('E3', 'cap20')]):,}", time.time() - t0), flush=True)

    cols = ["y", "run", "day", "reason", "cbuild", "react", "hold", "v", "age"]
    out = []
    frames = {}
    for cname in CELLS:
        for xname in EXITS:
            d = pd.DataFrame(rows[(cname, xname)], columns=cols)
            frames[(cname, xname)] = d
            if not len(d):
                out.append(dict(cell=cname, exit=xname, n=0))
                continue
            c = cell(d, days=days)
            per_day = d.groupby("day").run.nunique().sort_index()
            byb = d.groupby("cbuild").y.sum().sort_values(ascending=False)
            tot = float(d.y.sum())
            top1 = d.y.nlargest(max(1, int(round(0.01 * len(d))))).sum()
            out.append(dict(
                cell=cname, exit=xname, n=c["n"], mints=c["mints"], sol=c["sol"], pct=c["pct"],
                pos=c["pos"], worst=c["worst"], win=c["win"],
                floor="%d/%d" % (int((per_day >= 50).sum()), len(per_day)),
                clients=int(d.cbuild.nunique()),
                top_client=round(100 * float(byb.iloc[0]) / tot, 1) if tot > 0 else None,
                loo=round(tot - float(byb.iloc[0]), 2),
                top1=round(100 * float(top1) / tot, 1) if tot > 0 else None,
                react=round(100 * float(d.react.mean()), 2)))
    show(out, "LULL-then-WAKE fired on the whole tape at lag_115, one position per coin")

    d = frames[("E3", "cap60")]
    if len(d):
        d.to_parquet(data_file("cvx_hot_book_fires.parquet"), index=False)
        print("\n=== E3 reaction cost: the price move from the watcher's state to our 115 ms fill")
        q = (d.react * 100).quantile([0.1, 0.25, 0.5, 0.75, 0.9])
        print("  p10 %.2f %%  p25 %.2f %%  p50 %.2f %%  p75 %.2f %%  p90 %.2f %%  mean %.2f %%"
              % (q.iloc[0], q.iloc[1], q.iloc[2], q.iloc[3], q.iloc[4], 100 * float(d.react.mean())),
              flush=True)
        for col, edges in (("react", [-1, -0.02, -0.005, 0.005, 0.02, 0.05, 9]),
                           ("v", [0, 33, 42.43, 50, 60, 85, 1e9]),
                           ("age", [60, 120, 300, 600, 1800, 1e9])):
            g = d.groupby(pd.cut(d[col], edges), observed=True).agg(
                n=("y", "size"), sol=("y", "sum"), pct=("y", "mean"))
            g["pct"] = (g["pct"] / B * 100).round(2); g["sol"] = g["sol"].round(2)
            print("\n=== E3 cap60 by %s" % col, flush=True)
            print(g.to_string(), flush=True)
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
