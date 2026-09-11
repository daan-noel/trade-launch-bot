"""Hot-tape node, step 7: the SAME within-mint design, with the control sampling artifact removed.

THE ARTIFACT. In steps 2-6 the controls were public PRINTS on the same coin. A control print's
`gap` (time since the previous public print) is the FULL inter-arrival interval. A node buy lands
at some point INSIDE such an interval, so its gap is a fraction of one - about half on average,
by construction and regardless of behaviour. `gap_z` came out as the second-strongest feature in
the model, and part of that weight is arithmetic, not information. The same bias touches every
window feature slightly, because a moment sampled inside an interval is not the same object as
the print that ends it.

THE FIX. Cases and controls must be the same KIND of object: a moment in time. Controls are now
uniformly random TIMESTAMPS on the same coin within +/- 60 s of a case, inserted into the coin's
stream as zero-size virtual prints that are invisible to every public window. The state is then
computed at that moment from prints strictly before it - exactly as for a node buy, and exactly
as a live rule would evaluate it. The coin's own z-score baseline is built from REAL prints only,
so the virtual ones cannot move it.

Everything else is unchanged: stratum is the mint, so the door, the narrative, the creator and
every off-chain factor are constant inside a comparison, and the model can only learn the moment.

Their wallets are the instrument (7.4 law 20).
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd
import pyarrow.parquet as pq

from cvx import DAY0
from tape import Tape
from cvx_hot_event import FEATS, LOCAL_Z, features, node_ids

ROUTERS = ("Axiom", "Terminal", "GMGN", "Photon", "Bloom", "Trojan", "BullX", "Padre", "DFlow")
N_CTRL = 4
CTRL_WIN = 60.0
SEED = 20260910

pd.set_option("display.width", 420)
pd.set_option("display.max_rows", 400)


def expanding_z_masked(x, real):
    """(x - prior mean) / prior sd over that coin's REAL earlier prints only."""
    n = len(x)
    ok = (np.isfinite(x) & real).astype(np.float64)
    xf = np.where(np.isfinite(x) & real, x, 0.0)
    c1 = np.concatenate(([0.0], np.cumsum(xf)))
    c2 = np.concatenate(([0.0], np.cumsum(xf * xf)))
    cn = np.concatenate(([0.0], np.cumsum(ok)))
    cnt = cn[:n]
    with np.errstate(divide="ignore", invalid="ignore"):
        mu = c1[:n] / cnt
        var = c2[:n] / cnt - mu * mu
        sd = np.sqrt(np.maximum(var, 0.0))
        z = (x - mu) / sd
    z[cnt < 8] = np.nan
    z[~np.isfinite(z)] = np.nan
    return z


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
    rng = np.random.default_rng(SEED)
    print("tape %s prints  node coins %s  %ds"
          % (f"{T.n:,}", f"{len(runs):,}", time.time() - t0), flush=True)

    cols = FEATS + [c + "_z" for c in LOCAL_Z]
    case_rows, ctrl_rows = [], []

    for r in runs:
        r = int(r)
        a, b = T.start[r], T.end[r]
        n0 = b - a
        if n0 < 20 or not np.isfinite(c_s[r]):
            continue
        t0r = T.t[a:b]; v0 = T.v[a:b]; sol0 = T.sol[a:b]; side0 = T.side[a:b]
        mine0 = is_node[a:b]; rt0 = router_all[a:b]
        cases0 = np.nonzero(mine0 & (side0 == 1))[0]
        if not len(cases0):
            continue

        # control MOMENTS: uniform times near a case, never a print
        lo_t = t0r[np.maximum(np.searchsorted(t0r, t0r[cases0] - CTRL_WIN, side="left"), 0)]
        hi_t = t0r[np.minimum(np.searchsorted(t0r, t0r[cases0] + CTRL_WIN, side="right"),
                              n0 - 1)]
        m = min(N_CTRL * len(cases0), 4 * len(cases0))
        pick = rng.integers(0, len(cases0), size=m)
        u = rng.uniform(lo_t[pick], hi_t[pick])
        u = u[np.isfinite(u)]
        if len(u) < 4:
            continue

        # splice the virtual moments in as zero-size, node-flagged prints
        order = np.argsort(np.concatenate([t0r, u]), kind="stable")
        t = np.concatenate([t0r, u])[order]
        virt = np.concatenate([np.zeros(n0, bool), np.ones(len(u), bool)])[order]
        real = ~virt
        n = len(t)
        v = np.empty(n); sol = np.zeros(n); side = np.full(n, -1, np.int8)
        mine = np.ones(n, bool); rt = np.zeros(n, bool)
        v[real] = v0; sol[real] = sol0; side[real] = side0
        mine[real] = mine0; rt[real] = rt0
        # a virtual moment carries the last real reserve; ffill
        idx_real = np.nonzero(real)[0]
        if not len(idx_real) or idx_real[0] != 0:
            continue
        fill_from = np.maximum.accumulate(np.where(real, np.arange(n), 0))
        v[virt] = v[fill_from[virt]]

        f = features(t, v, sol, side, t - c_s[r], mine, rt)
        for c in LOCAL_Z:
            f[c + "_z"] = expanding_z_masked(f[c], real)
        M = np.column_stack([f[c] for c in cols]).astype(np.float32)

        pos_case = np.zeros(n, bool)
        pos_case[np.nonzero(real)[0][cases0]] = True
        ci = np.nonzero(pos_case)[0]
        ki = np.nonzero(virt)[0]
        case_rows.append(np.column_stack([np.full(len(ci), r, np.float32), M[ci]]))
        ctrl_rows.append(np.column_stack([np.full(len(ki), r, np.float32), M[ki]]))
        if r % 3000 == 0:
            print("  run %d  cases %s  %ds"
                  % (r, f"{sum(len(x) for x in case_rows):,}", time.time() - t0), flush=True)

    C = pd.DataFrame(np.vstack(case_rows), columns=["run"] + cols)
    K = pd.DataFrame(np.vstack(ctrl_rows), columns=["run"] + cols)
    C.to_parquet(data_file("cvx_hot_event3_cases.parquet"), index=False)
    K.to_parquet(data_file("cvx_hot_event3_ctrl.parquet"), index=False)
    print("\ncases %s  control MOMENTS %s  strata %s  %ds"
          % (f"{len(C):,}", f"{len(K):,}", f"{C.run.nunique():,}", time.time() - t0), flush=True)

    print("\n=== what the fix did to the gap feature (mean, cases vs controls)")
    for c in ("gap", "gap_z", "n2", "n2_z", "wake_z", "quiet60_z", "buy2_z"):
        print("  %-10s cases %9.3f   controls %9.3f"
              % (c, float(np.nanmean(C[c])), float(np.nanmean(K[c]))), flush=True)
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
