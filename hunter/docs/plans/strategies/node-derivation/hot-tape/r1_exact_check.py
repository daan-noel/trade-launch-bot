"""An independent recomputation of r1_exact.py's tickets: every fact, the entry fill and the exit
recomputed per ticket from the raw prints with pandas, sharing no code with r1_exact's walk.

  python r1_exact_check.py [MODE] [TAPE] [N]   default buy holdout_exact 400
"""
from __future__ import annotations

import _paths  # noqa: F401
from _paths import data_file

import sys

import numpy as np
import pandas as pd
import pyarrow.parquet as pq

from toolkit import tapes

MODE = sys.argv[1] if len(sys.argv) > 1 else "buy"   # e.g. buy, any, any_study_exact
TAPE = sys.argv[2] if len(sys.argv) > 2 else "holdout_exact"
N = int(sys.argv[3]) if len(sys.argv) > 3 else 400
import json as _json
_R = _json.loads(data_file("r1x_rule_%s.json" % MODE).read_text())
E = {k: v[1] for k, v in _R["entry"].items()}
TP, SL, CLK = _R["exit"]
CLK_US = int(round(CLK * 1e6))


def exit_rule(t, slot, n, f):
    """find_paper_exit_at, LagMs(115): the last print by fire + 115 ms, fire slot or next slot <= 3 on."""
    post2 = np.arange(f + 1, n)
    later2 = post2[slot[post2] > slot[f]]
    nx2 = slot[later2[0]] if len(later2) else None
    inw2 = (slot[post2] == slot[f]) | ((slot[post2] == nx2) if nx2 is not None and nx2 <= slot[f] + 3 else False)
    re = np.flatnonzero(~inw2)
    cand = post2[:re[0]] if len(re) else post2
    cd = cand[t[cand] <= t[f] + 115_000]
    return int(cd[-1]) if len(cd) else f


def main() -> None:
    F = pd.read_parquet(data_file("r1x_tickets_%s_%s.parquet" % (MODE, TAPE)))
    F = F.sample(min(N, len(F)), random_state=11)
    prints, tokf, wfile, _ = tapes.TAPES[TAPE]
    P = pq.read_table(str(data_file(prints)),
                      filters=[("mint", "in", list(F.mint.unique()))]).to_pandas()
    P = P.sort_values(["mint", "slot", "tx_index"] + (["leg_index"] if "leg_index" in P else []),
                      kind="stable")
    tok = pd.read_parquet(data_file(tokf)).drop_duplicates("mint").set_index("mint")
    cr = pd.read_parquet(data_file("r1_creators.parquet")).set_index("mint")
    W = pd.read_parquet(data_file(wfile)).set_index("wallet_id").address
    global TICK0
    TICK0 = int(pq.read_table(str(data_file(prints)), columns=["t_us" if "t_us" in pq.read_schema(str(data_file(prints))).names else "t_ms"]).column(0).to_numpy().min())
    if "t_us" not in pq.read_schema(str(data_file(prints))).names:
        TICK0 *= 1000
    TICK0 += 200_000
    bad = []
    for _, tk in F.iterrows():
        g = P[P.mint == tk.mint].reset_index(drop=True)
        t = g.t_us.to_numpy() if "t_us" in g else g.t_ms.to_numpy() * 1000
        v = g.reserve_lamports.to_numpy() / 1e9
        spot = v / g.vtok.to_numpy() if "vtok" in g else v * v / 3.219e16
        sol = g.amount_lamports.to_numpy() / 1e9; side = g.side.to_numpy(); wal = g.wallet_id.to_numpy()
        k = int(tk.k)
        cu = (pd.Timestamp(tok.created_at[tk.mint]) - pd.Timestamp(0, tz="UTC")) // pd.Timedelta(microseconds=1)
        creator = cr.creator[tk.mint]
        pre = slice(0, k + 1)
        buys = (side[pre] == 1) & (sol[pre] >= 0) & (t[pre] >= cu)
        addrs = W.reindex(wal[pre][buys]).to_numpy()
        buyers = len(set(a for a in addrs if a != creator))
        mb = np.flatnonzero((side[:k] == 1) & (wal[:k] == wal[k]))
        shold = (t[k] - t[mb[-1]]) / 1e6 if len(mb) else np.nan
        ms = t[:k + 1] // 1000
        nb5 = g.build[:k + 1][(ms >= ms[k] - 5000) & (ms <= ms[k])].nunique()
        run = np.maximum.accumulate(spot[:k + 1])
        hi = np.flatnonzero(np.r_[True, spot[1:k + 1] > run[:-1]])
        stall = max(0.0, (t[k] - t[hi[-1]]) / 1e6)
        age = (t[k] - cu) / 1e6
        facts = dict(ssize=sol[k], shold=shold, nb5=nb5, stall=stall, age=age, hold_n=buyers, vres=v[k])
        ok = (side[k] == -1 and sol[k] >= E["ssize"] and shold <= E["shold"] and nb5 >= E["nb5"]
              and stall <= E["stall"] and age >= E["age"] and buyers >= E["hold_n"] and v[k] <= E["vres"])
        # entry fill, spelled from paper_fill.rs again
        slot = g.slot.to_numpy(); dl = t[k] + 115_000
        post = np.arange(k + 1, len(g))
        if MODE.split("_")[0] == "buy":
            eb = (side[post] == 1) & (sol[post] >= 1e-5)
            later = post[eb & (slot[post] > slot[k])]
            nxt = slot[later[0]] if len(later) else None
            inw = (slot[post] == slot[k]) | ((slot[post] == nxt) if nxt is not None and nxt <= slot[k] + 3 else False)
            q = post[eb & inw]
            qd = q[t[q] <= dl]
            ei = int(qd[-1]) if len(q) and len(qd) else k
        else:
            ei = exit_rule(t, slot, len(g), k)
        # exit: pnl on each print after the entry fill, the clock on a print or the 200 ms tick
        s0 = spot[ei]; t0 = t[ei]; dl2 = t0 + CLK_US
        after = np.arange(ei + 1, len(g))
        pnl = (spot[after] - s0) / s0 * 100.0
        tr = after[(pnl >= TP) | (pnl <= -SL) | (t[after] - t0 >= CLK_US)]
        tau = TICK0 + -(-(dl2 - TICK0) // 200_000) * 200_000
        if len(tr) and t[tr[0]] <= tau:
            f = int(tr[0])
        else:
            f = int(tr[0]) - 1 if len(tr) else len(g) - 1
        post2 = np.arange(f + 1, len(g))
        later2 = post2[slot[post2] > slot[f]]
        nx2 = slot[later2[0]] if len(later2) else None
        inw2 = (slot[post2] == slot[f]) | ((slot[post2] == nx2) if nx2 is not None and nx2 <= slot[f] + 3 else False)
        # the window's prints are the contiguous run after f while the slot stays inside it
        run_end = np.flatnonzero(~inw2)
        cand = post2[:run_end[0]] if len(run_end) else post2
        cd = cand[t[cand] <= t[f] + 115_000]
        x = int(cd[-1]) if len(cd) else f
        diffs = []
        if x != tk.x:
            diffs.append("exit fill %d vs %d" % (x, tk.x))
        for c in facts:
            a, b = facts[c], tk[c]
            if not (np.isclose(a, b, rtol=1e-9, atol=1e-9) or (np.isnan(a) and np.isnan(b))):
                diffs.append("%s %s vs %s" % (c, a, b))
        if not ok:
            diffs.append("rule not satisfied by the independent facts")
        if ei != tk.ei:
            diffs.append("entry fill %d vs %d" % (ei, tk.ei))
        if diffs:
            bad.append((tk.mint, k, "; ".join(diffs)))
    print("%s %s: %d tickets rechecked, %d disagree" % (MODE, TAPE, len(F), len(bad)))
    for b in bad[:20]:
        print("  ", b)


def coin_tickets(g, cu, creator, W, tick0, mode):
    """Every ticket on one coin, from scratch: facts at every sell, fills, exits, occupancy."""
    t = g.t_us.to_numpy() if "t_us" in g else g.t_ms.to_numpy() * 1000
    v = g.reserve_lamports.to_numpy() / 1e9
    spot = v / g.vtok.to_numpy() if "vtok" in g else v * v / 3.219e16
    sol = g.amount_lamports.to_numpy() / 1e9; side = g.side.to_numpy(); wal = g.wallet_id.to_numpy()
    slot = g.slot.to_numpy(); bu = g.build.to_numpy(); n = len(g)
    addr = W.reindex(wal).to_numpy()
    isb = (side == 1) & (sol >= 0) & (t >= cu) & (addr != creator)
    first = pd.Series(np.arange(n)).groupby(wal).transform("min").to_numpy()
    firstbuy = {}
    for i in np.flatnonzero(isb):
        firstbuy.setdefault(wal[i], i)
    newbuyer = np.zeros(n, dtype=int)
    for w_, i in firstbuy.items():
        newbuyer[i] = 1
    buyers = np.cumsum(newbuyer)
    run = np.maximum.accumulate(spot)
    nh = np.r_[True, spot[1:] > run[:-1]]
    t_hi = pd.Series(np.where(nh, t, np.nan)).ffill().to_numpy()
    ms = t // 1000
    fires = []
    last_x = -1; t_ok = -1
    for k in range(n):
        if side[k] != -1 or sol[k] < E["ssize"] or v[k] > E["vres"] or (t[k] - cu) / 1e6 < E["age"] or buyers[k] < E["hold_n"]:
            continue
        mb = np.flatnonzero((side[:k] == 1) & (wal[:k] == wal[k]))
        if not len(mb) or (t[k] - t[mb[-1]]) / 1e6 > E["shold"]:
            continue
        if max(0.0, (t[k] - t_hi[k]) / 1e6) > E["stall"]:
            continue
        if len(set(bu[:k + 1][(ms[:k + 1] >= ms[k] - 5000) & (ms[:k + 1] <= ms[k])])) < E["nb5"]:
            continue
        if k <= last_x or t[k] < t_ok:
            continue
        dl = t[k] + 115_000; post = np.arange(k + 1, n)
        if mode == "buy":
            eb = (side[post] == 1) & (sol[post] >= 1e-5)
            later = post[eb & (slot[post] > slot[k])]
            nxt = slot[later[0]] if len(later) else None
            inw = (slot[post] == slot[k]) | ((slot[post] == nxt) if nxt is not None and nxt <= slot[k] + 3 else False)
            q = post[eb & inw]; qd = q[t[q] <= dl]
            ei = int(qd[-1]) if len(q) and len(qd) else k
        else:
            ei = exit_rule(t, slot, n, k)
        s0 = spot[ei]; t0 = t[ei]; dl2 = t0 + CLK_US
        after = np.arange(ei + 1, n)
        pnl = (spot[after] - s0) / s0 * 100.0
        tr = after[(pnl >= TP) | (pnl <= -SL) | (t[after] - t0 >= CLK_US)]
        tau = tick0 + -(-(dl2 - tick0) // 200_000) * 200_000
        f = int(tr[0]) if len(tr) and t[tr[0]] <= tau else (int(tr[0]) - 1 if len(tr) else n - 1)
        post2 = np.arange(f + 1, n)
        later2 = post2[slot[post2] > slot[f]]
        nx2 = slot[later2[0]] if len(later2) else None
        inw2 = (slot[post2] == slot[f]) | ((slot[post2] == nx2) if nx2 is not None and nx2 <= slot[f] + 3 else False)
        re = np.flatnonzero(~inw2)
        cand = post2[:re[0]] if len(re) else post2
        cd = cand[t[cand] <= t[f] + 115_000]
        x = int(cd[-1]) if len(cd) else f
        fires.append((k, ei, x)); last_x = x; t_ok = t[x]
    return fires


def recall(ncoins=300) -> None:
    F = pd.read_parquet(data_file("r1x_tickets_%s_%s.parquet" % (MODE, TAPE)))
    prints, tokf, wfile, t_min = tapes.TAPES[TAPE]
    names = pq.read_schema(str(data_file(prints))).names
    tcol = "t_us" if "t_us" in names else "t_ms"
    allm = pq.read_table(str(data_file(prints)), columns=["mint", tcol, "amount_lamports", "side"]).to_pandas()
    tick0 = int(allm[tcol].min()) * (1 if tcol == "t_us" else 1000) + 200_000
    big = allm[(allm.side == -1) & (allm.amount_lamports >= 1e9)].groupby("mint").size()
    with_t = pd.Series(F.mint.unique())
    without = pd.Series(big.index.difference(with_t))
    pick = pd.concat([with_t.sample(min(ncoins, len(with_t)), random_state=3),
                      without.sample(min(ncoins, len(without)), random_state=3)])
    P = pq.read_table(str(data_file(prints)), filters=[("mint", "in", list(pick))]).to_pandas()
    P = P.sort_values(["mint", "slot", "tx_index"] + (["leg_index"] if "leg_index" in P else []), kind="stable")
    tok = pd.read_parquet(data_file(tokf)).drop_duplicates("mint").set_index("mint")
    cr = pd.read_parquet(data_file("r1_creators.parquet")).set_index("mint")
    W = pd.read_parquet(data_file(wfile)).set_index("wallet_id").address
    tmin = -np.inf if t_min is None else t_min * 1e6
    diff = 0; nt = 0
    for m, g in P.groupby("mint", sort=False):
        g = g.reset_index(drop=True)
        cu = (pd.Timestamp(tok.created_at[m]) - pd.Timestamp(0, tz="UTC")) // pd.Timedelta(microseconds=1)
        mine = [(k, ei, x) for k, ei, x in coin_tickets(g, cu, cr.creator[m], W, tick0, MODE.split("_")[0])
                if (g.t_us[k] if "t_us" in g else g.t_ms[k] * 1000) >= tmin]
        theirs = [tuple(r) for r in F[F.mint == m][["k", "ei", "x"]].to_numpy()]
        nt += len(mine)
        if mine != theirs:
            diff += 1
            if diff <= 10:
                print("  %s  independent %s  r1_exact %s" % (m, mine[:4], theirs[:4]))
    print("%s %s recall: %d coins (%d with tickets), %d tickets independently, %d coins differ"
          % (MODE, TAPE, P.mint.nunique(), min(ncoins, len(with_t)), nt, diff))


if __name__ == "__main__":
    if len(sys.argv) > 4 and sys.argv[4] == "recall":
        recall()
    else:
        main()
