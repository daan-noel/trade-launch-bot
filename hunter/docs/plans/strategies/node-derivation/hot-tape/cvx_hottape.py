"""Hot-tape re-entry: instrument step. The decision moment in TAPE STATE only.

STORY. A coin is busy. Price flushes. The sellers are being ABSORBED - buy SOL keeps
arriving while price falls - and the fall decelerates. Buy the absorption, not the low.

Why this node. It is the only node in the roster where the 115 ms seat is a tailwind on
BOTH legs (buy into a flush 0.889-0.943, sell into strength 1.023-1.027), and its tell is
a STATE that persists for seconds, not a print that can be raced. Its tickets come from
thousands of unrelated coins, so the client unit is many by construction
(the-client-is-the-out-of-sample-unit).

The known blocker: an at-the-low detector books -7.33 % - the price path alone cannot tell
a turning low from a falling knife (trough-entry-is-the-lever). So this script does not ask
"did price fall". It asks whether ABSORPTION separates the node's buys from the tape.

TWO REFUSALS BUILT IN, because both are ways a study positive is manufactured:
  * every feature is STRICTLY BEFORE the decision print. The state a watcher holds when it
    decides to land at print i is built from prints 0..i-1 and the reserve v[i-1]. Including
    print i puts the node's OWN buy inside its own flow window and invents the lift.
  * a NODE-BLIND copy of every flow feature, with the six node wallets' SOL removed from the
    windows. A term that only survives with their flow in it is "follow the operator" wearing
    a tape-state costume, and is dead by the wallet-axis refusal.

This script does loop [1] only: no book, no exit, no door. Reconstruct every node buy in
public tape state, sample an unbiased base from the same tape, and read which state terms
discriminate. Their wallet list is never a term - it names the moment and then leaves.

D  unnamed        E  named by discrimination here        P  empty
X  not scored     R  one per token   S 0.2   seat lag_115
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd
import psycopg2

from cvx import DAY0, show
from tape import Tape

HUNTER = _paths.HUNTER
LOCAL = HUNTER / "_local"
NODE_NAME = "hot-tape re-entry"
LEAD = "AbQcLH"
BASE_P = 0.02           # unbiased sample rate for the base population
SEED = 20260909

pd.set_option("display.width", 400)
pd.set_option("display.max_rows", 300)
pd.set_option("display.max_columns", 60)


def load_url() -> str:
    for line in (HUNTER / ".env").read_text(encoding="utf-8").splitlines():
        s = line.strip()
        if s.startswith("DATABASE_URL=") and not s.startswith("#"):
            return s.split("=", 1)[1].strip().strip('"').strip("'")
    raise SystemExit("DATABASE_URL missing")


def node_ids() -> dict[int, str]:
    roster = pd.read_csv(
        LOCAL / "solo-traders.csv", usecols=["wallet", "wallet_address", "node", "use_it"]
    )
    roster = roster[(roster.node == NODE_NAME) & (roster.use_it == "yes")]
    conn = psycopg2.connect(load_url())
    conn.autocommit = True
    cur = conn.cursor()
    cur.execute(
        "SELECT id, left(address, 6) FROM public.wallet_dict WHERE address = ANY(%s)",
        (list(roster.wallet_address),),
    )
    rows = cur.fetchall()
    conn.close()
    lab = {int(i): str(p) for i, p in rows}
    want = set(roster.wallet)
    got = set(lab.values())
    if got != want:
        raise SystemExit("wallet_dict map got %s want %s" % (sorted(got), sorted(want)))
    return lab


def pct_from_reserve(v_now, v_then):
    """Price move between two reserves, in percent. price = vsol^2 / K, so the ratio squares."""
    with np.errstate(divide="ignore", invalid="ignore"):
        r = np.where(v_then > 0, (v_now / v_then) ** 2 - 1.0, np.nan)
    return r * 100.0


def run_features(t, v, sol, side, age, mine):
    """State STRICTLY BEFORE each print: built from prints 0..i-1, reserve v[i-1].

    `mine` marks prints belonging to the node wallets. Every flow feature is produced twice:
    once over all prints, once with the node's own SOL removed (suffix _x), so a term can be
    asked whether it is public tape or a proxy for the operator.
    """
    n = len(t)
    idx = np.arange(n)
    buy = side == 1
    pub = ~mine

    def cum(mask):
        return np.concatenate(([0.0], np.cumsum(np.where(mask, sol, 0.0))))

    cb, cs = cum(buy), cum(~buy)
    cbx, csx = cum(buy & pub), cum((~buy) & pub)

    # windows are half-open (t[i]-W, t[i]) and EXCLUDE print i itself
    j2 = np.searchsorted(t, t - 2.0, side="right")
    j5 = np.searchsorted(t, t - 5.0, side="right")
    j10 = np.searchsorted(t, t - 10.0, side="right")
    j20 = np.searchsorted(t, t - 20.0, side="right")
    j60 = np.searchsorted(t, t - 60.0, side="right")

    buy5 = cb[idx] - cb[j5]
    sell5 = cs[idx] - cs[j5]
    buy5p = cb[j5] - cb[j10]          # the PREVIOUS five seconds
    buy20 = cb[idx] - cb[j20]
    sell20 = cs[idx] - cs[j20]
    buy5x = cbx[idx] - cbx[j5]
    sell5x = csx[idx] - csx[j5]
    buy5px = cbx[j5] - cbx[j10]

    n5 = idx - j5
    n20 = idx - j20
    n60 = idx - j60

    # the reserve a watcher holds before print i lands, and the reserve W seconds earlier
    vprev = np.concatenate(([np.nan], v[:-1]))
    p2 = np.maximum(j2 - 1, 0)
    p5 = np.maximum(j5 - 1, 0)
    p10 = np.maximum(j10 - 1, 0)
    fall2 = pct_from_reserve(vprev, v[p2])
    fall5 = pct_from_reserve(vprev, v[p5])
    fall10 = pct_from_reserve(vprev, v[p10])

    peak = np.concatenate(([np.nan], np.maximum.accumulate(v)[:-1]))
    dd = pct_from_reserve(vprev, peak)     # <= 0, percent below the coin's peak so far

    with np.errstate(divide="ignore", invalid="ignore"):
        bshare5 = np.where(buy5 + sell5 > 0, buy5 / (buy5 + sell5), np.nan)
        bshare20 = np.where(buy20 + sell20 > 0, buy20 / (buy20 + sell20), np.nan)
        flow_acc = np.where(buy5p > 1e-9, buy5 / buy5p, np.nan)
        flow_acc_x = np.where(buy5px > 1e-9, buy5x / buy5px, np.nan)
        bshare5_x = np.where(buy5x + sell5x > 0, buy5x / (buy5x + sell5x), np.nan)
        # absorption: SOL bought per percent of price given up over the same five seconds.
        # Only defined while price is DOWN over the window; that is the whole point.
        absorb = np.where(fall5 < 0, buy5 / (-fall5), np.nan)
        # deceleration: the last 2 s of the fall against the pace of the whole 5 s
        decel = np.where(fall5 < 0, (fall2 / fall5) * (5.0 / 2.0), np.nan)

    return dict(
        age=age, v=vprev, n5=n5, n20=n20, n60=n60,
        buy5=buy5, sell5=sell5, buy5x=buy5x,
        bshare5=bshare5, bshare20=bshare20, bshare5_x=bshare5_x,
        flow_acc=flow_acc, flow_acc_x=flow_acc_x,
        fall2=fall2, fall5=fall5, fall10=fall10, dd=dd,
        absorb=absorb, decel=decel, sol=sol, side=side.astype(np.int8),
    )


FEATS = ["age", "v", "n5", "n20", "n60", "buy5", "sell5", "buy5x", "bshare5", "bshare20",
         "bshare5_x", "flow_acc", "flow_acc_x", "fall2", "fall5", "fall10", "dd",
         "absorb", "decel", "side"]


def qtab(d, cols, title, qs=(0.10, 0.25, 0.50, 0.75, 0.90)):
    rows = []
    for c in cols:
        a = pd.to_numeric(d[c], errors="coerce")
        a = a[np.isfinite(a)]
        r = dict(feature=c, n=len(a))
        for qq in qs:
            r["p%d" % int(qq * 100)] = round(float(a.quantile(qq)), 3) if len(a) else None
        rows.append(r)
    return show(rows, title)


def lift_table(node, base, col, edges, title):
    """The node's share of a bin against the tape's share of the same bin. No outcome here."""
    nb = pd.cut(pd.to_numeric(node[col], errors="coerce"), edges)
    bb = pd.cut(pd.to_numeric(base[col], errors="coerce"), edges)
    nc = nb.value_counts().sort_index()
    bc = bb.value_counts().sort_index()
    tot_n, tot_b = float(nc.sum()), float(bc.sum())
    rows = []
    for k in nc.index:
        n_n, n_b = float(nc[k]), float(bc.get(k, 0.0))
        if n_b == 0 and n_n == 0:
            continue
        sn = n_n / tot_n if tot_n else 0.0
        sb = n_b / tot_b if tot_b else 0.0
        rows.append(
            dict(
                bin=str(k),
                node_n=int(n_n),
                node_pct=round(100 * sn, 2),
                base_pct=round(100 * sb, 2),
                lift=round(sn / sb, 2) if sb > 0 else None,
            )
        )
    return show(rows, title)


def main() -> None:
    t0 = time.time()
    lab = node_ids()
    NODE = set(lab)
    print("node %s  ids %s" % (NODE_NAME, {lab[i]: i for i in sorted(NODE)}), flush=True)

    T = Tape(
        str(data_file("cvx_prints.parquet")),
        cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
              "wallet_id", "build"],
    )
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    days = (T.t.max() - T.t.min()) / 86400.0
    print("tape prints %s  tokens %s  days %.2f  %ds"
          % (f"{T.n:,}", f"{len(T.mints):,}", days, time.time() - t0), flush=True)

    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (
        pd.to_datetime(tok.created_at, utc=True, format="ISO8601") - pd.Timestamp(0, tz="UTC")
    ).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()

    for wid, name in sorted(lab.items(), key=lambda x: x[1]):
        m = T.wallet == wid
        print("  %s id %d  prints %s  buys %s  coins %s"
              % (name, wid, f"{int(m.sum()):,}", f"{int((m & (T.side == 1)).sum()):,}",
                 f"{len(np.unique(T.run_of[m])):,}"), flush=True)

    rng = np.random.default_rng(SEED)
    node_chunks, base_chunks = [], []
    node_extra, base_extra = [], []

    is_node = np.isin(T.wallet, list(NODE))
    touched = np.unique(T.run_of[is_node])
    touched_set = set(int(x) for x in touched)
    print("coins the node touches: %s of %s  %ds"
          % (f"{len(touched_set):,}", f"{len(T.mints):,}", time.time() - t0), flush=True)

    for r in range(len(T.start)):
        a, b = T.start[r], T.end[r]
        n = b - a
        if n < 5:
            continue
        t = T.t[a:b]
        v = T.v[a:b]
        sol = T.sol[a:b]
        side = T.side[a:b]
        wal = T.wallet[a:b]
        age = (t - c_s[r]) if np.isfinite(c_s[r]) else np.full(n, np.nan)

        want_node = r in touched_set
        keep = rng.random(n) < BASE_P
        if not want_node and not keep.any():
            continue

        f = run_features(t, v, sol, side, age, is_node[a:b])

        if want_node:
            sel = np.nonzero(is_node[a:b] & (side == 1))[0]
            if len(sel):
                d = {k: f[k][sel] for k in FEATS}
                d["w"] = np.array([lab[int(wal[i])] for i in sel])
                d["run"] = np.full(len(sel), r, dtype=np.int32)
                d["day"] = T.day[a:b][sel]
                d["k"] = sel.astype(np.int32)
                # is this a RE-entry: has that wallet already printed on this coin
                uq, fidx = np.unique(wal, return_index=True)
                fmap = dict(zip(uq.tolist(), fidx.tolist()))
                d["reent"] = np.array([int(i) > fmap[int(wal[i])] for i in sel])
                node_chunks.append(pd.DataFrame(d))

        selb = np.nonzero(keep)[0]
        if len(selb):
            d = {k: f[k][selb] for k in FEATS}
            d["run"] = np.full(len(selb), r, dtype=np.int32)
            d["day"] = T.day[a:b][selb]
            base_chunks.append(pd.DataFrame(d))

        if r % 20000 == 0:
            print("  run %d  node rows %s  base rows %s  %ds"
                  % (r, f"{sum(len(x) for x in node_chunks):,}",
                     f"{sum(len(x) for x in base_chunks):,}", time.time() - t0), flush=True)

    node = pd.concat(node_chunks, ignore_index=True)
    base = pd.concat(base_chunks, ignore_index=True)
    node.to_parquet(data_file("cvx_hottape_node.parquet"), index=False)
    base.to_parquet(data_file("cvx_hottape_base.parquet"), index=False)
    print("\nnode buys %s   base prints %s (%.2f %% sample)   %ds"
          % (f"{len(node):,}", f"{len(base):,}", 100 * BASE_P, time.time() - t0), flush=True)

    # ---- (1) what the moment looks like -------------------------------------------------
    base_b = base[base.side == 1]
    qtab(node, FEATS, "the node's decision moment (state STRICTLY BEFORE their print)")
    qtab(base_b, FEATS, "every BUY on the tape (unbiased %.0f %% sample)" % (100 * BASE_P))
    qtab(base, FEATS, "every PRINT on the tape (unbiased %.0f %% sample)" % (100 * BASE_P))
    lead = node[node.w == LEAD]
    if len(lead):
        qtab(lead, FEATS, "the lead instrument %s alone (n=%s)" % (LEAD, f"{len(lead):,}"))

    rows = []
    for w in sorted(node.w.unique()):
        m = node[node.w == w]
        rows.append(dict(
            w=w, n=len(m), coins=int(m.run.nunique()),
            reentry_pct=round(100 * float(m.reent.mean()), 1),
            age_p50=round(float(m.age.median()), 1),
            v_p50=round(float(m.v.median()), 1),
            n20_p50=round(float(m.n20.median()), 1),
            dd_p50=round(float(m.dd.median()), 2),
            fall5_p50=round(float(m.fall5.median()), 2),
            down5_pct=round(100 * float((m.fall5 < 0).mean()), 1),
            bshare5_p50=round(float(m.bshare5.median()), 3),
            acc_p50=round(float(m.flow_acc.median()), 2),
            acc_x_p50=round(float(m.flow_acc_x.median()), 2),
            absorb_p50=round(float(m.absorb.median()), 3),
        ))
    show(rows, "per-wallet decision moment")

    # ---- (2) which state terms discriminate, against BUYS ---------------------------------
    grids = [
        ("dd", [-101, -50, -30, -20, -10, -5, -0.001, 0.001],
         "drawdown from the coin's peak so far, price percent"),
        ("fall5", [-101, -20, -10, -5, -1, -0.001, 0.001, 1, 5, 10, 1e4],
         "price move over the last 5 s"),
        ("n20", [0, 5, 10, 20, 40, 80, 1e6], "prints in the last 20 s (how busy the tape is)"),
        ("bshare5", [-0.01, 0.2, 0.4, 0.5, 0.6, 0.8, 1.01],
         "buy share of SOL over the last 5 s"),
        ("absorb", [0, 0.1, 0.3, 1, 3, 10, 1e9],
         "ABSORPTION: SOL bought per percent of price given up, last 5 s (falls only)"),
        ("decel", [-1e4, 0, 0.25, 0.5, 1, 2, 1e4],
         "DECELERATION: last 2 s of the fall against the 5 s pace (falls only)"),
        ("flow_acc", [0, 0.5, 1, 2, 5, 1e9],
         "buy SOL last 5 s against the 5 s before it"),
        ("age", [0, 30, 60, 120, 300, 600, 1800, 1e9], "coin age, seconds"),
        ("v", [0, 33, 42.43, 50, 60, 70, 85, 1e9], "reserve, SOL"),
    ]
    for col, edges, title in grids:
        lift_table(node, base_b, col, edges, title + "   [base = every BUY]")

    # ---- (3) THE CONTROL: is the flow term public tape or the operator's own SOL? ---------
    print("\n=== THE NODE-BLIND CONTROL", flush=True)
    print("A term that only survives with the node's own SOL inside the window is",
          "'follow the operator' in a tape-state costume, and is refused.", flush=True)
    lift_table(node, base_b, "flow_acc", [0, 0.5, 1, 2, 5, 1e9],
               "buy-flow acceleration, ALL SOL   [base = every BUY]")
    lift_table(node, base_b, "flow_acc_x", [0, 0.5, 1, 2, 5, 1e9],
               "buy-flow acceleration, NODE SOL REMOVED   [base = every BUY]")
    lift_table(node, base_b, "bshare5", [-0.01, 0.2, 0.4, 0.5, 0.6, 0.8, 1.01],
               "buy share, ALL SOL   [base = every BUY]")
    lift_table(node, base_b, "bshare5_x", [-0.01, 0.2, 0.4, 0.5, 0.6, 0.8, 1.01],
               "buy share, NODE SOL REMOVED   [base = every BUY]")

    # ---- (4) the conjunction, term by term ------------------------------------------------
    def steps(d):
        return [
            ("deep off peak (dd <= -20)", d.dd <= -20.0),
            ("+ aged (age >= 120 s)", (d.dd <= -20.0) & (d.age >= 120.0)),
            ("+ busy (n20 >= 20)", (d.dd <= -20.0) & (d.age >= 120.0) & (d.n20 >= 20)),
            ("+ reserve 42-85", (d.dd <= -20.0) & (d.age >= 120.0) & (d.n20 >= 20)
                                & d.v.between(42.43, 85.0)),
            ("+ flow x2, node-blind", (d.dd <= -20.0) & (d.age >= 120.0) & (d.n20 >= 20)
                                      & d.v.between(42.43, 85.0) & (d.flow_acc_x >= 2.0)),
        ]

    ns, bs = steps(node), steps(base)
    rows = []
    for (name, mn), (_, mb) in zip(ns, bs):
        mn = mn.fillna(False)
        mb = mb.fillna(False)
        sn, sb = float(mn.mean()), float(mb.mean())
        rows.append(dict(term=name, node_n=int(mn.sum()), node_pct=round(100 * sn, 2),
                         base_pct=round(100 * sb, 2),
                         lift=round(sn / sb, 2) if sb > 0 else None,
                         tape_prints_per_day=round(float(mb.sum()) / BASE_P / days, 0),
                         tape_coins_per_day=round(
                             float(base[mb].run.nunique()) / BASE_P / days, 0)))
    show(rows, "the conjunction, term by term (discrimination only - no money here)")

    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
