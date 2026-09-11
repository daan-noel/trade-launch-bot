"""Hot-tape node, step 3: the conjunction, the DWELL test, and discordance.

Step 2 (`cvx_hot_event.py`) ranked 50 features by how a node buy sits among non-buy prints ON
THE SAME COIN. The mint-local basis won almost everywhere, and the top of the table tells one
story:

  wake_z    0.660 HIGH   the last 2 s are busier than this coin's own last 60 s
  buy2_z    0.640 HIGH   with real SOL in them
  gap_z     0.365 low    prints are arriving fast right now
  quiet60_z 0.418 low    but the last minute was unusually QUIET for this coin
  vol20_z   0.591 HIGH   and unusually volatile
  dd_peak   0.408 low    the coin is deep below its own peak
  age       0.582 HIGH   and old

**LULL, then WAKE.** Not a flush on a busy tape - an aged coin that had gone quiet and has just
started printing hard. It also explains why they can be 0.4-2.2 s slow (1.5): after a lull there
is no crowd to race.

This script asks the three questions that decide whether that is a rule:

  1. CONJUNCTION - within-mint lift of the joint condition, and what share of their buys it covers
  2. DWELL - how many seconds the condition has ALREADY been true when they buy. Evidence 1.4:
     a condition whose dwell is a fraction of a slot is a print anchor in disguise and costs
     -5.59 pp at the seat, however well it discriminates. This is a hard filter, not a preference.
  3. DISCORDANCE - on one coin the condition fires N times and they buy on M. The N-M refusals
     are the sharpest available control: same coin, same state, opposite action.

Their wallets are the instrument. Nothing here becomes a term (7.4 law 20).
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd
import psycopg2
import pyarrow.parquet as pq

from cvx import DAY0, show
from tape import Tape
from cvx_hot_event import FEATS, LOCAL_Z, expanding_z, features, node_ids

ROUTERS = ("Axiom", "Terminal", "GMGN", "Photon", "Bloom", "Trojan", "BullX", "Padre", "DFlow")

pd.set_option("display.width", 420)
pd.set_option("display.max_rows", 400)
pd.set_option("display.max_columns", 40)

# the grid, stated before the run. Each row is (wake_z, quiet60_z, buy2_z, dd_peak, age)
GRID = [
    ("E0  wake only",              (1.0, 99.0, -99.0, 99.0, 0.0)),
    ("E1  wake + quiet",           (1.0, 0.0, -99.0, 99.0, 0.0)),
    ("E2  wake + quiet + sol",     (1.0, 0.0, 0.5, 99.0, 0.0)),
    ("E3  + off peak",             (1.0, 0.0, 0.5, -20.0, 0.0)),
    ("E4  + aged",                 (1.0, 0.0, 0.5, -20.0, 120.0)),
    ("E5  harder wake",            (2.0, 0.0, 0.5, -20.0, 120.0)),
    ("E6  harder wake + quieter",  (2.0, -0.3, 1.0, -20.0, 120.0)),
    ("E7  very hard",              (3.0, -0.3, 1.5, -30.0, 120.0)),
]


def main() -> None:
    t0 = time.time()
    lab = node_ids()
    NODE = list(lab)
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id", "build"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    ix = pq.read_table(str(data_file("cvx_ix.parquet")), columns=["tool"])
    router_all = ix.column("tool").to_pandas().isin(ROUTERS).to_numpy()
    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()
    is_node = np.isin(T.wallet, NODE)
    runs = np.unique(T.run_of[np.nonzero(is_node)[0]])
    print("tape %s prints  node coins %s  %ds"
          % (f"{T.n:,}", f"{len(runs):,}", time.time() - t0), flush=True)

    n_g = len(GRID)
    hit_case = np.zeros(n_g); n_case = 0
    hit_ctrl = np.zeros(n_g); n_ctrl = 0
    dwell = [[] for _ in range(n_g)]
    fires_per_coin = [[] for _ in range(n_g)]
    taken_per_coin = [[] for _ in range(n_g)]
    # within-mint lift needs the per-coin rates, pooled
    coin_lift_num = np.zeros(n_g); coin_lift_den = np.zeros(n_g)

    for r in runs:
        r = int(r)
        a, b = T.start[r], T.end[r]
        n = b - a
        if n < 20 or not np.isfinite(c_s[r]):
            continue
        t = T.t[a:b]; v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]
        mine = is_node[a:b]
        cases = np.nonzero(mine & (side == 1))[0]
        if not len(cases):
            continue
        f = features(t, v, sol, side, t - c_s[r], mine, router_all[a:b])
        for c in ("wake", "quiet60", "buy2"):
            f[c + "_z"] = expanding_z(f[c])
        pub = np.nonzero(~mine)[0]
        n_case += len(cases); n_ctrl += len(pub)

        for gi, (_, (wz, qz, bz, dd, ag)) in enumerate(GRID):
            m = ((f["wake_z"] >= wz) & (f["quiet60_z"] <= qz) & (f["buy2_z"] >= bz)
                 & (f["dd_peak"] <= dd) & (f["age"] >= ag))
            m = np.nan_to_num(m, nan=False).astype(bool)
            hit_case[gi] += int(m[cases].sum())
            hit_ctrl[gi] += int(m[pub].sum())
            # within-coin lift: their buy rate inside the condition vs outside, on this coin
            inside = m & ~mine
            if inside.sum() >= 3 and (~m & ~mine).sum() >= 3:
                pin = m[cases].mean() if len(cases) else 0.0
                base = m[pub].mean()
                if base > 0:
                    coin_lift_num[gi] += pin / base
                    coin_lift_den[gi] += 1
            # dwell: how long has m been continuously true at each case
            if m.any():
                # index of the last False before i
                lastF = np.maximum.accumulate(np.where(~m, np.arange(n), -1))
                for k in cases:
                    if m[k]:
                        j = lastF[k]
                        dwell[gi].append(t[k] - t[max(j, 0)])
                # fires per coin: count of rising edges
                edges = np.nonzero(m & ~np.concatenate(([False], m[:-1])))[0]
                fires_per_coin[gi].append(len(edges))
                taken = 0
                for e in edges:
                    nxt = np.searchsorted(t, t[e] + 5.0, side="right")
                    if np.any(mine[e:nxt] & (side[e:nxt] == 1)):
                        taken += 1
                taken_per_coin[gi].append(taken)
            else:
                fires_per_coin[gi].append(0); taken_per_coin[gi].append(0)
        if r % 3000 == 0:
            print("  run %d  %ds" % (r, time.time() - t0), flush=True)

    out = []
    for gi, (nm, thr) in enumerate(GRID):
        d = np.asarray(dwell[gi]) if dwell[gi] else np.array([np.nan])
        fp = np.asarray(fires_per_coin[gi]); tp = np.asarray(taken_per_coin[gi])
        cov = hit_case[gi] / max(n_case, 1)
        base = hit_ctrl[gi] / max(n_ctrl, 1)
        out.append(dict(
            cell=nm,
            covers_their_buys=round(100 * cov, 2),
            base_rate_on_prints=round(100 * base, 3),
            pooled_lift=round(cov / base, 2) if base > 0 else None,
            within_coin_lift=round(coin_lift_num[gi] / coin_lift_den[gi], 2)
            if coin_lift_den[gi] else None,
            coins_scored=int(coin_lift_den[gi]),
            DWELL_p50=round(float(np.nanmedian(d)), 3),
            DWELL_p25=round(float(np.nanpercentile(d, 25)), 3),
            dwell_over_400ms=round(100 * float(np.nanmean(d > 0.4)), 1),
            fires_per_coin=round(float(fp.mean()), 2),
            taken_pct=round(100 * float(tp.sum()) / max(float(fp.sum()), 1), 1),
        ))
    show(out, "the conjunction: within-mint lift, DWELL, and how often they REFUSE it")

    print("\nDWELL is the hard filter. Evidence 1.4: a condition whose dwell is a fraction of a")
    print("slot is a print anchor in disguise and costs -5.59 pp at the seat. A slot is ~400 ms.")
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
