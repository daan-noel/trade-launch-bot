"""Rule 1 replayed print by print, the way a live engine meets the tape: the parity reference.

It shares no logic with the candidate-table path (toolkit facts / candidates / exits / book). Each
coin is walked once in chain order. The walk keeps every piece of state incrementally, from prints
already seen:
  - each wallet's bag and its last buy;
  - the holder count;
  - the recipes printed in the last 5 s;
  - the time of the last new high.
Rule 1 is decided on each print from that state alone. A position books its own exit by scanning
forward. Only the tape arrays (toolkit.tapes) and the curve constants (kernel) are reused.

  wallets "node": "public" leaves out the node's six wallets, exactly as the study did. Must
                  reproduce the toolkit's tickets (data/cvx_r1u_final_*.parquet) one for one.
          "all":  every wallet counts. This is the only spelling a live engine can run, and the
                  number the engine must match.
  lag     the fill on both legs: the last print landed by decision + lag (s)
  clock   "engine": the time exit fills at the state landed by deadline + lag
          "toolkit": it fills from the last print inside the window, + lag (the study's reading)

  python r1_replay.py [TAPE ...]  the audit tables (default study and holdout, every variant);
                                 holdout_legs is the holdout with every leg of a transaction
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import sys
from collections import Counter, deque
from dataclasses import dataclass

import numpy as np
import pandas as pd

from kernel import FEE, FIX, K
from toolkit import tapes
from cvx_hot_event import NODE_NAME

# rule 1 (hot-tape-rule-1.md section 1), every number in one place
SELL_MIN = 1.0        # SOL, the trigger sell
AGE_MIN = 158.0       # s since creation (the E floor of 60 s is inside it)
SELLER_HOLD = 30.0    # s since the seller's last buy on this coin
RECIPES_W = 5.0       # s window of distinct build_core recipes, before the print
RECIPES_MIN = 15
HIGH_MAX = 20.0       # s since the coin's last new high
HOLDERS_MIN = 368     # wallets holding the coin before the print
RES_MAX = 100.0       # vsol after the sell
TP, SL, CLOCK = 0.15, 0.40, 90.0
LAG = 0.115
CLIP = 0.2


def pnl(v0: float, v1: float, b: float = CLIP, shifted: bool = False) -> float:
    """Net SOL of buying b at reserve v0 and selling the bag at v1 on the curve.
    shifted=False: sell into the tape's v1 (the study kernel: our SOL is not in the curve at exit,
    so our impact is charged twice - conservative). shifted=True: sell into v1 + our SOL (the
    curve after our buy, others' flows unchanged)."""
    s = b / (1.0 + FEE)
    tok = K / v0 - K / (v0 + s)
    r = v1 + s if shifted else v1
    return (r - K / (K / r + tok)) * (1.0 - FEE) - b - 2.0 * FIX


def triggers(S, blind: bool) -> dict[int, list[int]]:
    """Per coin (run), the prints where rule 1's E and P hold, from state built print by print."""
    T = S.T
    out: dict[int, list[int]] = {}
    for r in range(len(T.start)):
        c = S.c_s[r]
        if not np.isfinite(c):
            continue
        a, b = int(T.start[r]), int(T.end[r])
        t = T.t[a:b].tolist(); v = T.v[a:b].tolist(); sol = T.sol[a:b].tolist()
        side = T.side[a:b].tolist(); wal = T.wallet[a:b].tolist(); bu = T.build[a:b].tolist()
        node = S.is_node[a:b].tolist()
        bag: dict = {}; lastbuy: dict = {}; holders = 0
        win: deque = deque(); rec: Counter = Counter()
        tm = -np.inf; runmax = -np.inf; t_high = t[0]
        hits = []
        for i in range(b - a):
            tm = max(tm, t[i])
            while win and win[0][0] < tm - RECIPES_W:
                bb = win.popleft()[1]
                rec[bb] -= 1
                if rec[bb] == 0:
                    del rec[bb]
            w = wal[i]
            # decide on print i from the state before it (and the reserve the print leaves)
            if (side[i] == -1 and sol[i] >= SELL_MIN and t[i] >= S.t_min
                    and (blind or not node[i])):
                lb = lastbuy.get(w)
                if (tm - c >= AGE_MIN and lb is not None and tm - lb <= SELLER_HOLD
                        and len(rec) >= RECIPES_MIN and t[i] - t_high <= HIGH_MAX
                        and holders >= HOLDERS_MIN and v[i] <= RES_MAX):
                    hits.append(i)
            # then fold print i into the state
            p = bag.get(w, 0.0)
            if side[i] == 1:
                vb = v[i] - sol[i]
                q = p + (K / vb - K / v[i])
                lastbuy[w] = tm
            else:
                vb = v[i] + sol[i]
                q = max(p - (K / v[i] - K / vb), 0.0)
            bag[w] = q
            if blind or not node[i]:
                holders += (q > 0) - (p > 0)
            win.append((tm, bu[i])); rec[bu[i]] += 1
            if v[i] > runmax:
                runmax = v[i]; t_high = t[i]
        if hits:
            out[r] = hits
    return out


def landed(t: list, j: int, when: float) -> int:
    """The last print landed by `when`, never before j."""
    x = j
    while x + 1 < len(t) and t[x + 1] <= when:
        x += 1
    return x


@dataclass
class Seat:
    lag: float = LAG
    clock: str = "engine"
    clip: float = CLIP


def book(S, hits: dict[int, list[int]], seat: Seat) -> pd.DataFrame:
    """One position per coin; the next fire must come after the previous exit's fill."""
    T = S.T
    rows = []
    for r, ks in hits.items():
        a, b = int(T.start[r]), int(T.end[r])
        t = T.t[a:b].tolist(); v = T.v[a:b].tolist(); n = b - a
        free = -1
        for k in ks:
            if k <= free:
                continue
            ei = landed(t, k, t[k] + seat.lag)
            v0 = v[ei]; deadline = t[ei] + CLOCK
            j = None; why = "time"; i = ei + 1
            while i < n and t[i] <= deadline:
                pr = (v[i] / v0) ** 2 - 1.0
                if pr >= TP:
                    j, why = i, "tp"; break
                if pr <= -SL:
                    j, why = i, "sl"; break
                i += 1
            if j is not None:
                x = landed(t, j, t[j] + seat.lag)
            elif seat.clock == "toolkit":
                jj = max(i - 1, ei)
                x = landed(t, jj, t[jj] + seat.lag)
            else:
                x = landed(t, ei, deadline + seat.lag)
            free = x
            rows.append((r, k, int(T.day[a + k]), t[k], ei, x, v0, v[x], why, t[x] - t[ei],
                         pnl(v0, v[x], seat.clip), pnl(v0, v[x], seat.clip, shifted=True),
                         x == n - 1 and v[x] >= 114.0))
    return pd.DataFrame(rows, columns=["run", "k", "day", "t", "ei", "x", "v0", "v1", "why", "hold",
                                       "y", "y_shift", "grad"])


def ledger(F: pd.DataFrame, days: float, clip: float = CLIP, yc: str = "y") -> dict:
    y = F[yc].to_numpy(); s = float(y.sum())
    per = F.groupby("day")[yc].sum()
    top = float(np.sort(y)[::-1][:max(1, round(0.01 * len(y)))].sum())
    dl = np.sort(F.day.unique()); h = (len(dl) + 1) // 2
    return dict(n=len(F), nday=round(len(F) / days, 1), pct=round(100 * y.mean() / clip, 2),
                sol=round(s, 2), solday=round(s / days, 3),
                pos="%d/%d" % ((per > 0).sum(), per.size), worst=round(float(per.min()), 2),
                body=round(s - top, 2), top1=round(100 * top / s, 1) if s > 0 else np.nan,
                maxcoin=round(100 * F.groupby("run")[yc].sum().max() / s, 1) if s > 0 else np.nan,
                h1=round(100 * F[F.day.isin(dl[:h])][yc].mean() / clip, 2),
                h2=round(100 * F[F.day.isin(dl[h:])][yc].mean() / clip, 2),
                sl=round(100 * (F.why == "sl").mean(), 1), grad=int(F.grad.sum()))


def per_day(F: pd.DataFrame, yc: str = "y") -> str:
    g = F.groupby("day").agg(n=(yc, "size"), sol=(yc, "sum"))
    return "  ".join("d%d:%d/%+.2f" % (d, n, s) for d, (n, s) in g.iterrows())


def concurrency(F: pd.DataFrame) -> int:
    ev = sorted([(t, 1) for t in F.t] + [(t + h + LAG, -1) for t, h in zip(F.t, F.hold)])
    c = m = 0
    for _, d in ev:
        c += d; m = max(m, c)
    return m


def compare(F: pd.DataFrame, G: pd.DataFrame) -> str:
    """Ticket-by-ticket: replay F against the toolkit's tickets G (keys run, k)."""
    a = set(zip(F.run, F.k)); b = set(zip(G.run, G.k))
    M = F.merge(G[["run", "k", "x", "y"]], on=["run", "k"], suffixes=("", "_tk"))
    return ("replay %d, toolkit %d, both %d, replay only %d, toolkit only %d; on both: exit index "
            "differs %d, |dy| max %.2e"
            % (len(a), len(b), len(a & b), len(a - b), len(b - a), int((M.x != M.x_tk).sum()),
               float((M.y - M.y_tk).abs().max()) if len(M) else 0.0))


def main() -> None:
    pd.set_option("display.width", 320)
    roster = tapes.roster(NODE_NAME)
    for name in sys.argv[1:] or ("study", "holdout"):
        S = tapes.load(name, roster)
        start = float(S.T.t.min())
        hits = {w: triggers(S, blind=(w == "all")) for w in ("node", "all")}
        rows = []
        g = data_file("cvx_r1u_final_%s.parquet" % name)
        G = pd.read_parquet(g) if g.exists() else None
        for w in ("node", "all"):
            for seat in (Seat(clock="toolkit"), Seat(), Seat(lag=0.2), Seat(lag=0.3), Seat(lag=0.5),
                         Seat(clip=0.35)):
                F = book(S, hits[w], seat)
                L = ledger(F, S.days, seat.clip)
                rows.append(dict(wallets=w, lag=seat.lag, clock=seat.clock, clip=seat.clip, **L,
                                 pct_shift=round(100 * F.y_shift.mean() / seat.clip, 2),
                                 conc=concurrency(F)))
                if seat == Seat(clock="toolkit") and w == "node" and G is not None:
                    print("[%s] node wallets, toolkit clock vs the toolkit's tickets: %s"
                          % (name, compare(F, G)), flush=True)
                if seat == Seat():
                    old = F.run.map(lambda r: S.c_s[r] < start)
                    print("[%s] %s wallets, engine seat: per day %s" % (name, w, per_day(F)))
                    print("[%s] %s wallets: %d tickets on coins created before the tape starts "
                          "(holder book incomplete), SOL %+.2f"
                          % (name, w, int(old.sum()), float(F.y[old.to_numpy()].sum())))
                    F.to_parquet(data_file("r1_replay_%s_%s.parquet" % (name, w)), index=False)
        print(pd.DataFrame(rows).to_string(index=False), flush=True)


if __name__ == "__main__":
    main()
