"""Hot-tape node, step 5: fire on the LULL, not on the WAKE.

Step 4 measured the thing that closes the reactive version. Firing at the edge of the wake, the
price move between the state a watcher held and our 115 ms fill is:

    p50 +1.91 %   p75 +4.83 %   p90 +10.63 %   mean +5.98 %

**We pay about six percent just to react.** That is the print-anchor penalty of evidence 1.4,
measured directly rather than argued, and the dwell test predicted it (0.216 s of dwell). It
also refutes the idea that lull-then-wake is the node's trigger: if reacting at 115 ms costs
6 %, reacting at their measured 0.4-2.2 s costs more, and they are profitable. They cannot be
firing on this. Coverage said the same thing more quietly - the conjunction covers 1.2 % of
their buys, so 98.8 % of what they do is something else.

But the same table names the way out. The features split into two kinds:

    WAKE side   wake_z, buy2_z, gap_z, vol20_z, n2_z   - transient, dwell 0.2 s, costly to chase
    LULL side   quiet60_z, n60_z, n20_z, dd_peak, age  - a state that lasts TENS of seconds

The lull is the half with dwell. Firing on it is anticipatory, not reactive: nothing has just
happened, so there is nothing to pay for, no race to lose, and our 115 ms is free.

    E   quiet60_z <= Q   this coin has gone unusually quiet for itself
        dd_peak   <= D   it sits below its own peak
        age       >= A
        wakes     >= N   it has woken from a lull at least N times before, so the coin is one
                         that DOES wake - a public, coin-local fact, no wallet named
    X   long enough to be paid by a wake, several families
    R   one position per coin   S 0.2 SOL   seat lag_115

The honest prior against this is on the record: "no deep dip ALONE selects DEAD tokens", and the
quiet deep-age node is red at this seat. So the reaction cost and the DWELL are reported beside
the book, and a control with the lull term removed runs next to it.
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
from kernel import book
from tape import Tape
from cvx_hot_event import expanding_z, features, node_ids

ROUTERS = ("Axiom", "Terminal", "GMGN", "Photon", "Bloom", "Trojan", "BullX", "Padre", "DFlow")

EXITS = {
    "cap60": dict(cap=60.0), "cap300": dict(cap=300.0), "cap900": dict(cap=900.0),
    "tp20_c300": dict(tp=20.0, cap=300.0), "tp30_c900": dict(tp=30.0, cap=900.0),
    "tr25_c600": dict(trail=25.0, cap=600.0),
    "shipped": dict(arm=21.0, trail=36.0, stop=43.75, cap=1200.0),
}
CELLS = {
    #      quiet60_z  dd_peak  age   min_wakes
    "L1": (0.0, 99.0, 0.0, 0),
    "L2": (-0.3, -20.0, 120.0, 0),
    "L3": (-0.3, -20.0, 120.0, 2),
    "L4": (-0.5, -30.0, 300.0, 3),
    "L5": (-0.5, -30.0, 300.0, 5),
    "C0": (99.0, -20.0, 120.0, 2),        # control: the lull term REMOVED
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
    print("tape %s prints  days %.2f  %ds" % (f"{T.n:,}", days, time.time() - t0), flush=True)

    rows = {(c, x): [] for c in CELLS for x in EXITS}
    dwell = {c: [] for c in CELLS}

    for r in range(len(T.start)):
        a, b = T.start[r], T.end[r]
        n = b - a
        if n < 30 or not np.isfinite(c_s[r]):
            continue
        t = T.t[a:b]
        age = t - c_s[r]
        if age[-1] < 120.0:
            continue
        v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]; sl = T.slot[a:b]
        mine = is_node[a:b]
        f = features(t, v, sol, side, age, mine, router_all[a:b])
        for c in ("wake", "quiet60"):
            f[c + "_z"] = expanding_z(f[c])
        vbef = v - sol * np.where(side == 1, 1.0, -1.0)
        day = T.day[a:b]
        # how many times this coin has woken BEFORE each print - a coin-local public fact
        woke = np.nan_to_num(f["wake_z"] >= 2.0, nan=False).astype(bool)
        edge_w = woke & ~np.concatenate(([False], woke[:-1]))
        wakes_before = np.concatenate(([0], np.cumsum(edge_w)[:-1]))

        for cname, (qz, dd, ag, mw) in CELLS.items():
            m = ((f["quiet60_z"] <= qz) & (f["dd_peak"] <= dd) & (f["age"] >= ag)
                 & (wakes_before >= mw))
            m = np.nan_to_num(m, nan=False).astype(bool)
            edges = np.nonzero(m & ~np.concatenate(([False], m[:-1])))[0]
            if not len(edges):
                continue
            # dwell of the condition, for the record
            lastF = np.maximum.accumulate(np.where(~m, np.arange(n), -1))
            ends = np.nonzero(~m & np.concatenate(([False], m[:-1])))[0]
            for e in ends[:50]:
                dwell[cname].append(t[e - 1] - t[max(lastF[e - 1], 0)])
            for xname, xspec in EXITS.items():
                last = -1
                acc = rows[(cname, xname)]
                for k in edges:
                    k = int(k)
                    if k <= last or k < 2 or k >= n - 1:
                        continue
                    y, ei, xi, reason = book(t, v, sl, k, xspec, lag=0.115, B=B)
                    react = (v[ei] / vbef[k]) ** 2 - 1.0
                    acc.append((y, r, int(day[k]), reason, str(cbuild[r]), float(react),
                                float(t[xi] - t[ei]), float(v[ei]), float(age[k])))
                    last = xi
        if r % 20000 == 0:
            print("  run %d  L3 fires %s  %ds"
                  % (r, f"{len(rows[('L3', 'cap300')]):,}", time.time() - t0), flush=True)

    cols = ["y", "run", "day", "reason", "cbuild", "react", "hold", "v", "age"]
    out = []
    for cname in CELLS:
        for xname in EXITS:
            d = pd.DataFrame(rows[(cname, xname)], columns=cols)
            if not len(d):
                out.append(dict(cell=cname, exit=xname, n=0)); continue
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
                REACT=round(100 * float(d.react.mean()), 2),
                hold=round(float(d.hold.median()), 1)))
    show(out, "fired on the LULL at lag_115, one position per coin")

    print("\n=== DWELL of the lull condition (seconds it stays true)")
    for cname in CELLS:
        dd = np.asarray(dwell[cname]) if dwell[cname] else np.array([np.nan])
        print("  %-4s n %6d  p25 %8.1f  p50 %8.1f  p75 %8.1f"
              % (cname, len(dd), np.nanpercentile(dd, 25), np.nanmedian(dd),
                 np.nanpercentile(dd, 75)), flush=True)
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
