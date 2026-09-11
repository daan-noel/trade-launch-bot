"""RECONCILIATION: replay the node's OWN buy and sell decisions through OUR kernel.

The check that should have come first, and did not. Every node verdict in this program compares
a reconstructed rule against a roster number computed by a different pipeline. If the two
pipelines disagree on the SAME trades, every one of those verdicts is unsafe.

So: take each wallet's actual episodes on this tape, price them three ways, and see whether our
machinery reproduces what the roster says they earn.

  THEIRS      their own SOL in and out, fee charged the way the roster charges it
              (net = sol_out*0.9875 - sol_in*1.0125), which should reproduce margin_pct
  OURS_0      our kernel, our 0.2 SOL clip, entering and exiting at THEIR OWN prints, zero lag
  OURS_115    the same, with both legs filled at the last print landed by decision + 115 ms

If THEIRS and OURS_0 agree, the kernel is sound and any gap to a reconstructed rule is the
rule's fault. If they disagree, the cost model is wrong and every book in this program is
mis-stated by that amount.

Two things the roster does that this script reports separately, because both flatter:
  * `census.rb_ep2 ... AND NOT is_bag` - positions never closed are EXCLUDED from margin_pct
  * margin is net over CURVE-SIDE spend, not over the SOL that actually left the wallet
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd
import psycopg2

import kernel
from kernel import K, fill_idx, net
from cvx import DAY0, show
from tape import Tape

HUNTER = _paths.HUNTER
LOCAL = HUNTER / "_local"
NODES = ("hot-tape re-entry", "mid-tape one-shot")
FEE_OUT, FEE_IN = 0.9875, 1.0125
B = 0.2

pd.set_option("display.width", 400)
pd.set_option("display.max_rows", 300)
pd.set_option("display.max_columns", 40)


def load_url() -> str:
    for line in (HUNTER / ".env").read_text(encoding="utf-8").splitlines():
        s = line.strip()
        if s.startswith("DATABASE_URL=") and not s.startswith("#"):
            return s.split("=", 1)[1].strip().strip('"').strip("'")
    raise SystemExit("DATABASE_URL missing")


def node_ids() -> tuple[dict[int, str], dict[int, str]]:
    roster = pd.read_csv(
        LOCAL / "solo-traders.csv",
        usecols=["wallet", "wallet_address", "node", "use_it", "margin_pct", "trades"])
    roster = roster[roster.node.isin(NODES) & (roster.use_it == "yes")]
    conn = psycopg2.connect(load_url())
    conn.autocommit = True
    cur = conn.cursor()
    cur.execute("SELECT id, left(address, 6) FROM public.wallet_dict WHERE address = ANY(%s)",
                (list(roster.wallet_address),))
    rows = cur.fetchall()
    conn.close()
    lab = {int(i): str(p) for i, p in rows}
    node = {int(i): str(roster.loc[roster.wallet == str(p), "node"].iloc[0]) for i, p in rows}
    return lab, node


def main() -> None:
    t0 = time.time()
    lab, node_of = node_ids()
    NODE = list(lab)
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id", "build"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    print("tape prints %s  tokens %s  %ds"
          % (f"{T.n:,}", f"{len(T.mints):,}", time.time() - t0), flush=True)

    mine = np.isin(T.wallet, NODE)
    idx = np.nonzero(mine)[0]
    runs = np.unique(T.run_of[idx])
    print("node prints %s on %s coins  %ds"
          % (f"{len(idx):,}", f"{len(runs):,}", time.time() - t0), flush=True)

    rows = []
    for r in runs:
        r = int(r)
        a, b = T.start[r], T.end[r]
        n = b - a
        t = T.t[a:b]
        v = T.v[a:b]
        sol = T.sol[a:b]
        side = T.side[a:b]
        wal = T.wallet[a:b]
        loc = np.nonzero(np.isin(wal, NODE))[0]
        for w in np.unique(wal[loc]):
            w = int(w)
            p = np.nonzero(wal == w)[0]
            buys = p[side[p] == 1]
            sells = p[side[p] == -1]
            if len(buys) == 0:
                continue
            k_in = int(buys[0])
            sol_in = float(sol[buys].sum())
            if sol_in <= 0:
                continue
            bag = len(sells) == 0
            k_out = int(sells[-1]) if not bag else n - 1
            sol_out = float(sol[sells].sum()) if not bag else 0.0
            their_net = sol_out * FEE_OUT - sol_in * FEE_IN
            # our kernel on THEIR two decisions, our clip
            y0 = net(v[k_in], v[k_out], B) if not bag else -B
            ei = fill_idx(t, k_in, 0.115)
            xi = fill_idx(t, k_out, 0.115)
            y115 = net(v[ei], v[xi], B) if not bag else -B
            rows.append((
                lab[w], node_of[w], r, int(T.day[a + k_in]), bag,
                sol_in, sol_out, their_net, 100 * their_net / sol_in,
                100 * (sol_out / sol_in - 1.0),
                y0, 100 * y0 / B, y115, 100 * y115 / B,
                100 * ((v[k_out] / v[k_in]) ** 2 - 1.0),
                float(v[k_in]), float(t[k_out] - t[k_in]),
                len(buys), len(sells),
            ))
        if r % 4000 == 0:
            print("  run %d  eps %s  %ds" % (r, f"{len(rows):,}", time.time() - t0), flush=True)

    d = pd.DataFrame(rows, columns=[
        "w", "node", "run", "day", "bag", "sol_in", "sol_out", "their_net", "their_pct",
        "their_move", "y0", "our_pct0", "y115", "our_pct115", "move", "v_in", "hold",
        "n_buys", "n_sells"])
    d.to_parquet(data_file("cvx_hottape_replay.parquet"), index=False)
    print("\nepisodes %s  bags %s (%.1f %%)  %ds"
          % (f"{len(d):,}", f"{int(d.bag.sum()):,}", 100 * float(d.bag.mean()),
             time.time() - t0), flush=True)

    ok = d[~d.bag]

    print("\n=== 1. DOES OUR KERNEL REPRODUCE THEIR ROSTER MARGIN, on their own decisions?")
    out = []
    for nm, g in list(ok.groupby("node")) + [("ALL", ok)]:
        gb = d[d.node == nm] if nm != "ALL" else d
        out.append(dict(
            node=nm, episodes=len(g), bags=int(gb.bag.sum()),
            their_margin_closed=round(100 * float(g.their_net.sum()) / float(g.sol_in.sum()), 2),
            their_margin_with_bags=round(
                100 * float(gb.their_net.sum() - gb[gb.bag].sol_in.sum() * FEE_IN)
                / float(gb.sol_in.sum()), 2),
            their_move_mean=round(float(g.their_move.mean()), 2),
            our_move_same_prints=round(float(g.move.mean()), 2),
            ours_lag0=round(float(g.our_pct0.mean()), 2),
            ours_lag115=round(float(g.our_pct115.mean()), 2),
        ))
    show(out, "their decisions, priced by them and by us")

    print("\n=== 2. PER WALLET (closed episodes only)")
    out = []
    ros = pd.read_csv(LOCAL / "solo-traders.csv", usecols=["wallet", "margin_pct", "trades"])
    ros = ros.set_index("wallet")
    for w, g in ok.groupby("w"):
        gb = d[d.w == w]
        out.append(dict(
            w=w, node=g.node.iloc[0], eps=len(g),
            roster_margin=float(ros.margin_pct.get(w, np.nan)),
            here_margin=round(100 * float(g.their_net.sum()) / float(g.sol_in.sum()), 2),
            bag_pct=round(100 * float(gb.bag.mean()), 1),
            with_bags=round(100 * float(gb.their_net.sum() - gb[gb.bag].sol_in.sum() * FEE_IN)
                            / float(gb.sol_in.sum()), 2),
            their_clip=round(float(g.sol_in.median()), 3),
            their_move=round(float(g.their_move.mean()), 2),
            our_move=round(float(g.move.mean()), 2),
            ours_lag0=round(float(g.our_pct0.mean()), 2),
            ours_lag115=round(float(g.our_pct115.mean()), 2),
            hold_p50=round(float(g.hold.median()), 1),
        ))
    show(out, "per wallet: roster margin vs the same episodes priced here and by our kernel")

    print("\n=== 3. WHAT THE 115 ms COSTS ON THEIR OWN DECISIONS")
    print("  lag 0   %.2f %%   lag 115   %.2f %%   difference  %.2f pp"
          % (float(ok.our_pct0.mean()), float(ok.our_pct115.mean()),
             float(ok.our_pct115.mean() - ok.our_pct0.mean())), flush=True)
    clean = ok[(ok.n_buys == 1) & (ok.n_sells == 1)]
    print("  on ONE-buy-ONE-sell episodes only (n=%s): their move %.2f %%, our move %.2f %%,"
          " ours lag0 %.2f %%, ours lag115 %.2f %%"
          % (f"{len(clean):,}", float(clean.their_move.mean()), float(clean.move.mean()),
             float(clean.our_pct0.mean()), float(clean.our_pct115.mean())), flush=True)

    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
