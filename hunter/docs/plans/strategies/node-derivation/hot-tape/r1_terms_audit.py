"""Rule 1's terms checked against the lake's raw fields, before any re-derivation (step 1 of
hunter/docs/roadmap/hot-tape-rule-1-engine-plan.md).

A second code that shares an idea with the first cannot catch that idea being wrong (the holder
book was float dust in both). Each check here compares a term's input with an INDEPENDENT exact
field of the same print: the reserve change against the SOL amount, the reserve pair against the
constant product, the token change against token_amount, the creation time against the first
print, and the labels and wallets a term keys on.

  python r1_terms_audit.py [DAY ...]    lake days, default 2026-09-03..2026-09-10
"""
from __future__ import annotations

import _paths  # noqa: F401
from _paths import LAKE

import sys

import numpy as np
import pandas as pd
import pyarrow.parquet as pq

from kernel import K

COLS = ["mint", "is_buy", "sol_amount", "token_amount", "slot", "block_time", "leg_index", "vsol",
        "vtok", "venue", "tx_index", "ix_labels", "wallet"]
DAYS = ["2026-09-%02d" % d for d in range(3, 11)]


def day(d: str) -> pd.DataFrame:
    x = pq.read_table(str(LAKE / "trades" / f"dt={d}" / "data.parquet"), columns=COLS).to_pandas()
    return x


def main() -> None:
    days = sys.argv[1:] or DAYS
    tok = pq.read_table(str(LAKE / "tokens" / "tokens.parquet"), columns=["mint", "created_at"]).to_pandas()
    tok = tok.drop_duplicates("mint").set_index("mint")
    acc = {}
    def add(k, n):
        acc[k] = acc.get(k, 0) + n
    first_seen = {}
    for d in days:
        x = day(d)
        add("rows", len(x))
        cur = x[x.venue == "curve"]
        add("curve rows", len(cur))
        add("curve rows, vsol missing", int(cur.vsol.isna().sum()))
        add("curve rows, vtok missing", int(cur.vtok.isna().sum()))
        add("curve rows, wallet missing", int(cur.wallet.isna().sum()))
        add("curve rows, ix_labels missing", int(cur.ix_labels.isna().sum()))
        add("curve rows, ix_labels empty list", int((cur.ix_labels == "[]").sum()))
        c = cur[cur.vsol.notna() & cur.vtok.notna()].sort_values(["mint", "slot", "tx_index", "leg_index"])
        same = (c.mint.values[1:] == c.mint.values[:-1])
        v = c.vsol.to_numpy(); vt = c.vtok.to_numpy(); s = c.sol_amount.to_numpy()
        ta = c.token_amount.to_numpy(); b = c.is_buy.to_numpy(); bt = c.block_time.to_numpy()
        # 1. the reserve change against the SOL amount (does v after - v before equal +-sol?)
        dv = (v[1:] - v[:-1])[same]
        sg = np.where(b[1:], 1.0, -1.0)[same]
        err = dv - sg * s[1:][same]
        add("consecutive pairs", int(same.sum()))
        add("pairs, |dvsol - sol| <= 2 lamports", int((np.abs(err) <= 2e-9).sum()))
        add("pairs, |dvsol - sol| > 1e-6 SOL", int((np.abs(err) > 1e-6).sum()))
        # 2. token change against token_amount
        dvt = np.abs(vt[1:] - vt[:-1])[same]
        terr = dvt - ta[1:][same]
        add("pairs, |dvtok - token_amount| <= 1 unit", int((np.abs(terr) <= 1).sum()))
        # 3. the constant product, and new highs by vsol vs by the engine's spot vsol/vtok
        prod = v * vt
        add("rows, |vsol*vtok/K - 1| > 1e-6", int((np.abs(prod / K - 1) > 1e-6).sum()))
        spot = v / vt
        g = c.mint.to_numpy()
        ch = np.r_[True, g[1:] != g[:-1]]
        grp = np.cumsum(ch) - 1
        vs = pd.Series(v); ss = pd.Series(spot)
        vmax_prev = vs.groupby(grp).cummax().groupby(grp).shift(1).to_numpy()
        smax_prev = ss.groupby(grp).cummax().groupby(grp).shift(1).to_numpy()
        ok = ~np.isnan(vmax_prev)
        nh_v = v[ok] > vmax_prev[ok]; nh_s = spot[ok] > smax_prev[ok]
        add("rows with a prior print", int(ok.sum()))
        add("new high by vsol != new high by spot", int((nh_v != nh_s).sum()))
        # 4. time: backward steps inside a coin; ms rounding vs floor
        dt = (bt[1:] - bt[:-1])[same]
        add("pairs, block_time runs backward", int((dt < 0).sum()))
        add("rows, block_time not on a whole ms", int((bt % 1000 != 0).sum()))
        # 5. legs
        k = c.groupby(["mint", "slot", "tx_index"]).size()
        add("transactions", len(k)); add("transactions with >1 leg", int((k > 1).sum()))
        # 6. first print per coin (for the creation-time check)
        f = c.groupby("mint").block_time.min()
        for m_, t_ in f.items():
            if m_ not in first_seen:
                first_seen[m_] = t_
        print("  %s done" % d, flush=True)
    for k_, n in acc.items():
        print("%-45s %s" % (k_, f"{n:,}"))
    F = pd.Series(first_seen)
    ca = tok.reindex(F.index).created_at
    have = ca.notna()
    lead = (F[have].astype(np.int64) - ca[have].astype(np.int64)) / 1e6  # s, first print - created_at
    print("coins with a first print: %d, with created_at: %d" % (len(F), int(have.sum())))
    print("first print - created_at (s): p1 %.3f  p50 %.3f  p99 %.3f  share < 0: %.4f  share > 5 s: %.4f"
          % (lead.quantile(.01), lead.median(), lead.quantile(.99), float((lead < 0).mean()), float((lead > 5).mean())))


if __name__ == "__main__":
    main()
