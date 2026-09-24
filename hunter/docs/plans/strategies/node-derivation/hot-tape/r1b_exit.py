"""Rule 1b: rule 1's entry, frozen, with a new exit (evidence 1.24).

The walk, the fills, the seat, the cost and the occupancy are r1_exact's (the reference simulate
books ticket for ticket, evidence 1.23); only the exit decision is new. An exit is an OR of
clauses, each read the way the engine reads it: on every print after the entry fill, and on the
200 ms tick grid between prints (price frozen at the last print, time-driven terms moving). The
exit fill is LagMs(115) from the last folded print, as for rule 1's clock.

Exit terms (pnl, peak and retrace against the entry fill's spot; peak seeds at the fill):
  tp X          pnl >= X                            (m_position.pnl)
  sl X          pnl <= -X                           (m_position.pnl)
  clock X       held >= X s                         (m_position.held)
  ladder        [(a1, r1), (a2, r2), ...]: the since-entry peak has reached a_k (the highest
                tier reached) and the price is r_k % below that peak   (needs a peak-pnl metric)
  gtrail (a, r) pnl >= a and retrace >= r            (m_position arm_above_pct + retrace, today)
  sib (X, Y)    pnl >= X and this print is a buy of >= Y SOL   (m_position.pnl + m_flow_window
                window_size_prints 1 buy)
  head f        pnl >= f x the wall headroom ((115 / vsol_entry)^2 - 1)
  cut           each fires only while pnl < 0 (a cut that fires while ahead is a clock):
                  new N      no new (first-time, non-creator) buyer for N s
                  buys W x   SOL bought in the last W s <= x          (m_flow_window buy)
                  recipes k  distinct recipes in the last 5 s <= k    (m_build_window)
                  stall S    no new lifetime high for S s             (m_price_lifetime.stall)
                  creator    the creator prints a sell

THE BARS, fixed ahead of every comparison. The base is rule 1's exit (tp 20, sl 60,
clock 90) on the study tape, 0.2 SOL, 115 ms.
  1. Folds: the study days split in two; each half picks the family's best spec (most SOL among
     specs with every fit day positive) and that pick must beat the base on the OTHER half by
     more than the chance floor (walkforward.cut_noise of the base book), in both folds.
  2. The family's spec is the one with the most study SOL among specs whose every study day is
     positive, and it must pass r1_exact.bars_fail (all days up, both halves, body > 0, top 1 %
     and the biggest coin <= 15 % of net).
  3. It must beat the base's study SOL at the 200 ms seat and at 0.35 SOL as well.
  4. Survivors combine greedily, best first; a family joins only if the combination passes 1-3
     against the combination without it.
  5. The holdout is booked once, on the final spec, and is read, never used to choose.

  python r1b_exit.py prep TAPE          candidates passing rule 1's entry (cached)
  python r1b_exit.py check TAPE         rule 1's exit through this evaluator = the frozen tickets
  python r1b_exit.py paths              step 1: the path of every study ticket under rule 1
  python r1b_exit.py alone              step 2: each family alone against the base, bars 1-3
  python r1b_exit.py combine            step 3: the survivors combined, bar 4 -> data/r1b_exit_spec.json
  python r1b_exit.py book TAPE SPEC     one spec (a JSON dict) at 115/200/500 ms and 0.35 SOL
  python r1b_exit.py ref TAPE           RULE_B's tickets, frozen for the engine parity
                                        (data/r1b_ref_TAPE.parquet, the columns of r1_ref)
"""
from __future__ import annotations

import _paths  # noqa: F401
from _paths import data_file

import itertools
import json
import sys
from dataclasses import replace

import numpy as np
import pandas as pd
import pyarrow.parquet as pq

import r1_exact as rx
from r1_exact import Tape, Sem, TICK_US, CLIP, y_engine, exit_fill, entry_fill, occupy, summary, bars_fail
from toolkit import tapes, walkforward
from toolkit.book import mask

SEM = Sem(entry="any")
R1 = json.loads(data_file("r1x_rule_any_study_exact.json").read_text())
SPEC = {k: tuple(v) for k, v in R1["entry"].items()}
BASE = {"tp": 20.0, "sl": 60.0, "clock": 90.0}
# Rule 1b's exit: the wall target at 0.4 with rule 1's stop and clock. Picked after the holdout
# was read (evidence 1.24), so only the days after 09-10 certify it.
RULE_B = {"tp": None, "sl": 60.0, "clock": 90.0, "head": 0.4}
CAP_S = 300.0   # the longest clock any spec may carry
WALL = 115.0
INF = np.iinfo(np.int64).max


# ---------------------------------------------------------------- per-coin facts

class Coin:
    """Running facts over one coin's prints, the pre-entry history included."""

    def __init__(self, T: Tape, r: int):
        a, b = int(T.start[r]), int(T.end[r])
        self.a, self.n = a, b - a
        t = T.t_us[a:b]
        self.t = t
        self.tacc = np.maximum.accumulate(t)
        self.spot = T.spot[a:b]
        self.v = T.v[a:b]
        side = T.side[a:b]; sol = T.sol[a:b]; wal = T.wallet[a:b]
        self.side, self.sol = side, sol
        self.build = T.build[a:b]
        creator = int(T.creator[r]); cu = T.created_us[r]
        # time of the latest first-time buyer (not the creator) at or before each print
        isnew = np.zeros(self.n, dtype=bool)
        seen = set()
        for i in range(self.n):
            if side[i] == 1 and wal[i] != creator and t[i] >= cu and wal[i] not in seen:
                seen.add(wal[i]); isnew[i] = True
        self.isnew = isnew
        ln = np.where(isnew, t, -INF)
        self.last_new = np.maximum.accumulate(ln)
        # time of the lifetime high (strictly higher spot) at or before each print
        pk = -np.inf; pat = cu if np.isfinite(cu) else t[0]
        ath = np.empty(self.n, dtype=np.int64)
        for i in range(self.n):
            if self.spot[i] > pk:
                pk = self.spot[i]; pat = t[i]
            ath[i] = pat
        self.ath_t = ath
        self.cbuy = np.r_[0.0, np.cumsum(np.where(side == 1, sol, 0.0))]
        self.ms = t // 1000
        self.creator_sell = (side == -1) & (wal == creator)


# ---------------------------------------------------------------- the evaluator

def next_tick(T: Tape, tau):
    """First tick at or after tau (arrays ok)."""
    return T.tick0 + -(-(tau - T.tick0) // TICK_US) * TICK_US


def window_start(c: Coin, j: np.ndarray, w_s: float) -> np.ndarray:
    """First index inside the closed window [ms - w, ms] at print j (WindowSpec::bounds)."""
    lo_ms = c.ms[j] - int(round(w_s * 1000))
    return np.searchsorted(c.ms, lo_ms, side="left")  # ms is chain order; regressions are rare


def first_trip(T: Tape, c: Coin, ei: int, spec: dict, start: int | None = None):
    """(f, event time, why) for one position entered at local index ei; prints before `start`
    (default: the one after the fill) and the ticks before `start - 1`'s interval are not read."""
    s0 = c.spot[ei]; t0 = int(c.t[ei]); v0 = c.v[ei]
    start = ei + 1 if start is None else start
    clock = spec.get("clock", 90.0)
    clock_us = int(round((clock if clock is not None else 1e6) * 1e6))
    hi = int(np.searchsorted(c.tacc, t0 + int(CAP_S * 1e6) + 1_000_000, side="right"))
    J = np.arange(ei, max(hi, ei + 1))
    tj = c.t[J]
    nx = J + 1
    tn = np.where(nx < c.n, c.t[np.minimum(nx, c.n - 1)], INF)   # the next print's time
    sp = c.spot[J]
    pnl = (sp - s0) / s0 * 100.0
    peak = np.maximum.accumulate(sp)
    peakp = (peak - s0) / s0 * 100.0
    retr = (peak - sp) / peak * 100.0
    held = (tj - t0)
    P = {}   # name -> bool per print (index 0 = the fill print, never read as a print trip)
    Q = {}   # name -> earliest time inside interval j at which it holds (INF = never)
    if spec.get("tp") is not None:
        P["tp"] = pnl >= spec["tp"]
    if spec.get("sl") is not None:
        P["sl"] = pnl <= -spec["sl"]
    if clock is not None:
        P["time"] = held >= clock_us
        Q["time"] = np.full(len(J), t0 + clock_us, dtype=np.int64)
    if spec.get("head") is not None:
        h = ((WALL / v0) ** 2 - 1.0) * 100.0 * spec["head"]
        P["head"] = pnl >= h
    if spec.get("ladder"):
        tiers = sorted(spec["ladder"])
        need = np.full(len(J), np.inf)
        for a_, r_ in tiers:
            need = np.where(peakp >= a_, r_, need)
        P["trail"] = retr >= need
    if spec.get("gtrail"):
        a_, r_ = spec["gtrail"]
        P["gtrail"] = (pnl >= a_) & (retr >= r_)
    if spec.get("sib"):
        x_, y_ = spec["sib"]
        P["sib"] = (pnl >= x_) & (c.side[J] == 1) & (c.sol[J] >= y_)
    under = pnl < 0
    for cut in spec.get("cut", []):
        kind = cut[0]
        name = "cut_" + kind
        if kind == "new":
            tau = c.last_new[J] + int(cut[1] * 1e6)
            P[name] = under & (tj >= tau)
            Q[name] = np.where(under, tau, INF)
        elif kind == "stall":
            tau = c.ath_t[J] + int(cut[1] * 1e6)
            P[name] = under & (tj >= tau)
            Q[name] = np.where(under, tau, INF)
        elif kind == "creator":
            P[name] = under & c.creator_sell[J]
        elif kind == "buys":
            w_s, x_ = cut[1], cut[2]
            st = window_start(c, J, w_s)
            tot = c.cbuy[J + 1] - c.cbuy[st]
            P[name] = under & (tot <= x_)
            # the window only loses buys until the next print: the first eviction that takes the
            # sum to x or below; a print at ms p leaves the window at ms p + w + 1
            m1 = np.searchsorted(c.cbuy, c.cbuy[J + 1] - x_, side="left")  # m1 = index+1 of that print
            m = np.clip(m1 - 1, 0, c.n - 1)
            tau_ms = c.ms[m] + int(round(w_s * 1000)) + 1
            Q[name] = np.where(under, np.where(tot <= x_, tj, tau_ms * 1000), INF)
        elif kind == "recipes":
            k_ = int(cut[1])
            st = window_start(c, J, 5.0)
            val = np.zeros(len(J), dtype=np.int64)
            q = np.full(len(J), INF, dtype=np.int64)
            for ii in np.flatnonzero(under):
                s_, j_ = st[ii], J[ii]
                bw = c.build[s_:j_ + 1]
                u, first_rev = np.unique(bw[::-1], return_index=True)
                val[ii] = len(u)
                if len(u) <= k_:
                    q[ii] = tj[ii]
                else:  # the window only loses recipes until the next print: the (k+1)-th most
                    # recent recipe's last print leaves it at its ms + 5000 + 1
                    last_ms = np.sort(c.ms[s_:j_ + 1][len(bw) - 1 - first_rev])[::-1]
                    q[ii] = (int(last_ms[k_]) + 5000 + 1) * 1000
            P[name] = under & (val <= k_)
            Q[name] = q
        else:
            raise ValueError(cut)
    names = list(P)
    Pm = np.zeros(len(J), dtype=bool)
    for nm in names:
        Pm |= P[nm]
    Pm[:max(start - ei, 1)] = False
    qt = np.full(len(J), INF, dtype=np.int64)
    qwho = np.full(len(J), -1)
    qnames = list(Q)
    for qi, nm in enumerate(qnames):
        tick = next_tick(T, np.maximum(Q[nm], tj + 1))
        ok = (Q[nm] < INF) & (tick < tn) & (J >= start - 1)
        cand = np.where(ok, tick, INF)
        better = cand < qt
        qt = np.where(better, cand, qt); qwho = np.where(better, qi, qwho)
    ev = np.flatnonzero(Pm | (qt < INF))
    if not len(ev):
        return int(J[-1]), INF, "none"
    i = int(ev[0])
    if Pm[i]:
        why = next(nm for nm in ("sl", "tp", "head", "trail", "gtrail", "sib", "time") + tuple(
            "cut_" + c_[0] for c_ in spec.get("cut", [])) if nm in P and P[nm][i])
        return int(J[i]), int(tj[i]), why
    return int(J[i]), int(qt[i]), qnames[int(qwho[i])]


def y_legs(s0, v0, legs, b=CLIP):
    """kernel.rs round_trip_multi_leg: legs = [(fraction of the bag, spot, vsol)]. One
    `buy_fill`, then one `sell_proceeds` a leg on its own depth; only the last leg empties the
    bag, so only it pays the rent-reclaim close."""
    tok, paid = rx.buy_fill(s0, v0, b)
    last = len(legs) - 1
    proceeds = sum(rx.sell_proceeds(tok * q, s1, v1, i == last)
                   for i, (q, s1, v1) in enumerate(legs))
    return proceeds - paid


def book(T: Tape, C: pd.DataFrame, spec: dict, coins: dict, sem: Sem = SEM, b: float = CLIP) -> pd.DataFrame:
    rows = []
    for r, k in zip(C.run.to_numpy(), C.k.to_numpy()):
        c = coins[r]
        ei = entry_fill(T, c.a, c.n, int(k), sem)
        s0, v0 = c.spot[ei], c.v[ei]
        stage = spec.get("stage")
        if stage is None:
            f, _te, why = first_trip(T, c, ei, spec)
            x = exit_fill(T, c.a, c.n, f, sem)
            s1, v1 = c.spot[x], c.v[x]
            rows.append((ei, x, c.t[x], (c.t[x] - c.t[ei]) / 1e6, y_engine(s0, v0, s1, v1, b), v1, why))
            continue
        # a scale-out: global exits (sl, clock) close everything; the stage sells q of the bag
        # at its target, then the remainder runs under `rest` plus the global exits, read from
        # the first event after the stage's fill (no decision while a sell is in flight)
        glob = {"tp": None, "sl": spec.get("sl"), "clock": spec.get("clock")}
        g, tg, gwhy = first_trip(T, c, ei, glob)
        sf, ts, _ = first_trip(T, c, ei, {"tp": stage["tp"], "sl": None, "clock": None})
        if ts >= tg:
            x = exit_fill(T, c.a, c.n, g, sem)
            rows.append((ei, x, c.t[x], (c.t[x] - c.t[ei]) / 1e6,
                         y_engine(s0, v0, c.spot[x], c.v[x], b), c.v[x], gwhy))
            continue
        x1 = exit_fill(T, c.a, c.n, sf, sem)
        rest = dict(spec["rest"], sl=spec.get("sl"), clock=spec.get("clock"))
        f, _te, why = first_trip(T, c, ei, rest, start=x1 + 1)
        x = exit_fill(T, c.a, c.n, f, sem)
        q = stage["q"]
        y = y_legs(s0, v0, [(q, c.spot[x1], c.v[x1]), (1.0 - q, c.spot[x], c.v[x])], b)
        rows.append((ei, x, c.t[x], (c.t[x] - c.t[ei]) / 1e6, y, c.v[x], "stage+" + why))
    D = C.copy()
    o = pd.DataFrame(rows, columns=["ei", "x", "t_x", "hold", "y", "v1", "why"], index=C.index)
    for col in o:
        D[col] = o[col]
    D["y_curve"] = D.y
    D["t"] = D.t_us / 1e6
    D["grad"] = (D.v1 >= 114.0) & (D.x == np.array([coins[r].n - 1 for r in D.run]))
    return D


def fires(D: pd.DataFrame) -> pd.DataFrame:
    return occupy(D, np.ones(len(D), dtype=bool))


# ---------------------------------------------------------------- data

def prep(name: str) -> None:
    T = Tape(name)
    C = rx.candidates(T, SEM)
    C.to_parquet(data_file("r1b_cands_%s.parquet" % name), index=False)
    print(name, "candidates", len(C), "passing rule 1's entry", int(mask(C, SPEC).sum()))


def load(name: str):
    T = Tape(name)
    C = pd.read_parquet(data_file("r1b_cands_%s.parquet" % name))
    C = C[mask(C, SPEC)].sort_values(["run", "k"]).reset_index(drop=True)
    coins = {r: Coin(T, int(r)) for r in np.unique(C.run.to_numpy())}
    return T, C, coins


def check(name: str) -> None:
    T, C, coins = load(name)
    F = fires(book(T, C, BASE, coins))
    ref = pd.read_parquet(data_file("r1_ref_%s.parquet" % name))
    F = F.assign(mint=T.mints[F.run.to_numpy()])
    m = F.merge(ref, on=["mint", "k"], how="outer", suffixes=("", "_ref"), indicator=True)
    both = m[m._merge == "both"]
    print(name, "tickets", len(F), "reference", len(ref), "same trigger", len(both),
          "this only", int((m._merge == "left_only").sum()), "reference only", int((m._merge == "right_only").sum()))
    print("  entry fill differs", int((both.ei != both.ei_ref).sum()), " exit print differs",
          int((both.x != both.x_ref).sum()), " reason differs", int((both.why != both.why_ref).sum()),
          " max |SOL diff| %.3g" % float((both.y - both.pnl_sol_engine_kernel).abs().max()))
    print("  ", summary(F, T.days))


# ---------------------------------------------------------------- step 1: paths

def paths() -> None:
    pd.set_option("display.width", 250)
    T, C, coins = load("study_exact")
    F = fires(book(T, C, BASE, coins))
    out = []
    for r, ei, x, why in zip(F.run, F.ei, F.x, F.why):
        c = coins[r]
        s0 = c.spot[ei]; t0 = c.t[ei]
        seg = slice(ei, x + 1)
        pn = (c.spot[seg] - s0) / s0 * 100
        ip = int(np.argmax(pn))
        # after the exit: how far would it have run in the next 120 s
        hi = int(np.searchsorted(c.tacc, c.t[x] + 120_000_000, side="right"))
        post = (c.spot[x:hi] - s0) / s0 * 100 if hi > x else np.array([pn[-1]])
        # the first 10 s of the hold
        h10 = int(np.searchsorted(c.tacc, t0 + 10_000_000, side="right"))
        n10 = int(((c.side[ei + 1:h10] == 1)).sum())
        sb10 = float(c.sol[ei + 1:h10][c.side[ei + 1:h10] == 1].sum())
        new10 = int(c.isnew[ei + 1:h10].sum())
        out.append(dict(why=why, pnl=pn[-1], peak=pn.max(), t_peak=(c.t[ei + ip] - t0) / 1e6,
                        trough=pn.min(), post_max=post.max(), post_min=post.min(),
                        buys10=n10, buysol10=sb10, new10=new10, pnl10=(c.spot[max(h10 - 1, ei)] - s0) / s0 * 100))
    P = pd.DataFrame(out)
    P["y"] = F.y.to_numpy()
    print("study tickets under rule 1's exit:", len(P), " SOL %.2f" % P.y.sum())
    g = P.groupby("why").agg(n=("y", "size"), sol=("y", "sum"), peak_med=("peak", "median"),
                             t_peak_med=("t_peak", "median"), trough_med=("trough", "median"))
    print(g.round(2).to_string())
    tm = P[P.why == "time"]
    print("\nclock exits (%d): how high did they get first" % len(tm))
    for lvl in (3, 5, 8, 10, 15):
        s = tm[tm.peak >= lvl]
        print("  peak >= +%2d %%: %3d (%.0f %%), their SOL %+.2f, exit pnl median %+.1f %%" % (
            lvl, len(s), 100 * len(s) / len(tm), s.y.sum(), s.pnl.median() if len(s) else np.nan))
    s = tm[tm.peak < 3]
    print("  never +3 %%   : %3d (%.0f %%), their SOL %+.2f, exit pnl median %+.1f %%, trough median %+.1f %%" % (
        len(s), 100 * len(s) / len(tm), s.y.sum(), s.pnl.median(), s.trough.median()))
    tp = P[P.why == "tp"]
    print("\ntake profits (%d): the next 120 s from the exit" % len(tp))
    for lvl in (25, 30, 40, 50, 75, 100):
        print("  reaches +%3d %%: %3d (%.0f %%)" % (lvl, int((tp.post_max >= lvl).sum()), 100 * (tp.post_max >= lvl).mean()))
    print("  post-exit max median %+.1f %%, min median %+.1f %%" % (tp.post_max.median(), tp.post_min.median()))
    print("\nfirst 10 s of the hold, by outcome (median): winners = tp, losers = time with peak < 3 or sl")
    W = P[P.why == "tp"]; L = P[((P.why == "time") & (P.peak < 3)) | (P.why == "sl")]
    for col in ("pnl10", "buys10", "buysol10", "new10"):
        print("  %-9s winners %7.2f  losers %7.2f" % (col, W[col].median(), L[col].median()))
    P.to_parquet(data_file("r1b_paths_study.parquet"), index=False)


# ---------------------------------------------------------------- step 2: families alone

def families() -> dict:
    B = dict(BASE)
    fam = {}
    fam["sl"] = [dict(B, sl=v) for v in (25.0, 30.0, 40.0, 50.0)]
    lad = []
    for a1, r1 in itertools.product((5.0, 10.0, 15.0), (3.0, 5.0, 8.0)):
        for tp in (20.0, 30.0, 50.0, None):
            lad.append(dict(B, tp=tp, ladder=[(a1, r1)]))
            for a2, r2 in itertools.product((20.0, 30.0, 50.0), (5.0, 10.0, 15.0)):
                if a2 > a1 and (tp is None or a2 < tp):
                    lad.append(dict(B, tp=tp, ladder=[(a1, r1), (a2, r2)]))
    fam["ladder"] = lad
    fam["gtrail"] = [dict(B, gtrail=(a, r)) for a, r in itertools.product((3.0, 5.0, 8.0, 10.0, 15.0), (3.0, 5.0, 8.0, 10.0))]
    fam["sib"] = [dict(B, sib=(x, y)) for x, y in itertools.product((3.0, 5.0, 8.0, 10.0, 15.0), (0.5, 1.0, 2.0, 3.0))]
    fam["head"] = [dict(B, tp=None, head=f) for f in (0.3, 0.4, 0.5, 0.6, 0.75)]
    fam["cut_new"] = [dict(B, cut=[("new", n)]) for n in (3.0, 5.0, 8.0, 12.0, 20.0, 30.0)]
    fam["cut_buys"] = [dict(B, cut=[("buys", w, x)]) for w, x in itertools.product((3.0, 5.0, 10.0), (0.25, 0.5, 1.0, 2.0))]
    fam["cut_recipes"] = [dict(B, cut=[("recipes", k)]) for k in (3, 5, 8, 10)]
    fam["cut_stall"] = [dict(B, cut=[("stall", s)]) for s in (20.0, 30.0, 45.0, 60.0)]
    fam["cut_creator"] = [dict(B, cut=[("creator",)])]
    fam["clock"] = [dict(B, clock=v) for v in (45.0, 60.0, 120.0, 180.0)]
    return fam


def families2() -> dict:
    """Round 2, which follows round 1's results (marked as such): step 1 shows winners running
    past +20 % with pullbacks wider than round 1's trails, and adds idea 3 (half out). Same bars."""
    fam = {}
    wide = []
    for clock in (90.0, 180.0):
        for a1, r1 in itertools.product((15.0, 20.0, 30.0), (15.0, 20.0, 25.0, 30.0)):
            wide.append(dict(tp=None, sl=60.0, clock=clock, ladder=[(a1, r1)]))
            for r2 in (20.0, 30.0):
                if r2 != r1:
                    wide.append(dict(tp=None, sl=60.0, clock=clock, ladder=[(a1, r1), (50.0, r2)]))
    fam["ladder_wide"] = wide
    half = []
    rests = [{"head": 0.4, "tp": None}, {"head": 0.6, "tp": None}, {"tp": 40.0},
             {"tp": None, "ladder": [(20.0, 20.0)]}, {"tp": None, "ladder": [(30.0, 25.0)]}]
    for q, x1, clock, rest in itertools.product((0.5, 0.75), (15.0, 20.0), (90.0, 180.0), rests):
        half.append(dict(tp=None, sl=60.0, clock=clock, stage={"q": q, "tp": x1}, rest=rest))
    fam["half_out"] = half
    fam["head_clock"] = [dict(tp=None, sl=60.0, clock=c_, head=f) for f, c_ in itertools.product(
        (0.3, 0.4, 0.5, 0.6), (90.0, 120.0, 180.0))]
    return fam


def key(spec: dict) -> str:
    return json.dumps(spec, sort_keys=True)


def judge(T, C, coins, specs, base_F, days, noise, label):
    """Bars 1-2 for one family; returns (row, best spec, its book)."""
    fl = walkforward.folds(base_F)
    books = {}
    for sp in specs:
        books[key(sp)] = fires(book(T, C, sp, coins))
    picks = []
    for fit, test in fl:
        ok = [k_ for k_, F in books.items() if walkforward.allpos(F, fit)]
        best = max(ok, key=lambda k_: walkforward.solsum(books[k_], fit)) if ok else None
        g = (walkforward.solsum(books[best], test) - walkforward.solsum(base_F, test)) if best else np.nan
        picks.append((best, g))
    alld = [k_ for k_, F in books.items() if walkforward.allpos(F, np.sort(base_F.day.unique()))]
    top = max(alld, key=lambda k_: books[k_].y.sum()) if alld else max(books, key=lambda k_: books[k_].y.sum())
    Ft = books[top]
    fold_ok = all(p[0] is not None and p[1] > nz for p, nz in zip(picks, noise))
    why = bars_fail(Ft, days)
    row = dict(family=label, specs=len(specs), fold1_gain=round(picks[0][1], 2), fold2_gain=round(picks[1][1], 2),
               folds=fold_ok, best=top, **{k_: v for k_, v in summary(Ft, days).items() if k_ in (
                   "n", "pct", "sol", "pos", "top1", "maxcoin", "win")}, bars=why or "pass")
    return row, json.loads(top), Ft, books


def alone(round_: str = "1") -> None:
    pd.set_option("display.width", 320); pd.set_option("display.max_colwidth", 120)
    T, C, coins = load("study_exact")
    base_F = fires(book(T, C, BASE, coins))
    noise = walkforward.cut_noise(base_F)
    base_sol = base_F.y.sum()
    b200 = fires(book(T, C, BASE, coins, sem=replace(SEM, lag_ms=200))).y.sum()
    b35 = fires(book(T, C, BASE, coins, b=0.35)).y.sum()
    print("base", summary(base_F, T.days), "noise per test half", noise,
          "SOL at 200 ms %.2f, at 0.35 SOL %.2f" % (b200, b35), flush=True)
    rows = []
    fams = families() if round_ == "1" else families2()
    for label, specs in fams.items():
        row, top, Ft, _ = judge(T, C, coins, specs, base_F, T.days, noise, label)
        s200 = fires(book(T, C, top, coins, sem=replace(SEM, lag_ms=200))).y.sum()
        s35 = fires(book(T, C, top, coins, b=0.35)).y.sum()
        row.update(gain=round(row["sol"] - base_sol, 2), s200=round(s200 - b200, 2), s35=round(s35 - b35, 2))
        row["pass"] = bool(row["folds"] and row["bars"] == "pass" and s200 > b200 and s35 > b35)
        rows.append(row)
        print(pd.DataFrame([row]).drop(columns=["best"]).to_string(index=False, header=len(rows) == 1),
              "\n   best:", row["best"], flush=True)
    R = pd.DataFrame(rows)
    R.to_parquet(data_file("r1b_alone%s.parquet" % ("" if round_ == "1" else round_)), index=False)


def merge(base: dict, add: dict) -> dict:
    """`add`'s own terms laid over `base`; cut lists join."""
    out = dict(base)
    for k_, v in add.items():
        if k_ == "cut":
            out["cut"] = list(base.get("cut", [])) + list(v)
        elif v != BASE.get(k_):
            out[k_] = v
    return out


def combine() -> None:
    """Bar 4: the survivors of `alone`, best first; each joins only if the combination passes
    bars 1-3 against the combination without it (the family's grid laid over the current spec)."""
    pd.set_option("display.width", 320); pd.set_option("display.max_colwidth", 160)
    T, C, coins = load("study_exact")
    R = pd.read_parquet(data_file("r1b_alone.parquet"))
    order = R[R["pass"]].sort_values("gain", ascending=False).family.tolist()
    fam = families()
    cur = dict(BASE)
    for label in order:
        F0 = fires(book(T, C, cur, coins))
        noise = walkforward.cut_noise(F0)
        specs = [merge(cur, {k_: v for k_, v in sp.items() if k_ == "cut" or v != BASE.get(k_)}) for sp in fam[label]]
        row, top, Ft, _ = judge(T, C, coins, specs, F0, T.days, noise, label)
        s200 = fires(book(T, C, top, coins, sem=replace(SEM, lag_ms=200))).y.sum()
        b200 = fires(book(T, C, cur, coins, sem=replace(SEM, lag_ms=200))).y.sum()
        s35 = fires(book(T, C, top, coins, b=0.35)).y.sum()
        b35 = fires(book(T, C, cur, coins, b=0.35)).y.sum()
        ok = bool(row["folds"] and row["bars"] == "pass" and s200 > b200 and s35 > b35
                  and row["sol"] > F0.y.sum())
        print("%-12s on %s\n   -> %s  sol %.2f vs %.2f  folds %+.2f/%+.2f (noise %s)  200ms %+.2f  0.35 %+.2f  bars %s  %s"
              % (label, json.dumps(cur), json.dumps(top), row["sol"], F0.y.sum(), row["fold1_gain"],
                 row["fold2_gain"], noise, s200 - b200, s35 - b35, row["bars"], "JOINS" if ok else "stays out"),
              flush=True)
        if ok:
            cur = top
    print("\n=== rule 1b exit:", json.dumps(cur))
    data_file("r1b_exit_spec.json").write_text(json.dumps(cur) + "\n")
    F = fires(book(T, C, cur, coins))
    print("study", summary(F, T.days), F.why.value_counts().to_dict())


def book_one(name: str, spec_json: str) -> None:
    T, C, coins = load(name)
    spec = json.loads(spec_json)
    for lag in (115, 200, 500):
        F = fires(book(T, C, spec, coins, sem=replace(SEM, lag_ms=lag)))
        print(name, lag, summary(F, T.days), F.why.value_counts().to_dict())
    F = fires(book(T, C, spec, coins, b=0.35))
    print(name, "0.35 SOL", summary(F, T.days, 0.35))


def ref(name: str) -> None:
    """RULE_B's tickets with every print named by (slot, tx_index, leg), as r1_ref names rule 1's."""
    T, C, coins = load(name)
    F = fires(book(T, C, RULE_B, coins)).reset_index(drop=True)
    P = pq.read_table(str(data_file(tapes.TAPES[name][0])), columns=["slot", "tx_index", "leg_index"])
    slot = P.column("slot").to_numpy().astype(np.int64)
    tx = P.column("tx_index").to_numpy().astype(np.int32)
    leg = P.column("leg_index").to_numpy().astype(np.int32)
    a = T.start[F.run.to_numpy()]
    out = pd.DataFrame({"mint": T.mints[F.run.to_numpy()], "k": F.k.to_numpy(), "ei": F.ei.to_numpy(),
                        "x": F.x.to_numpy(), "why": F.why.to_numpy(), "hold": F.hold.to_numpy(),
                        "pnl_sol_engine_kernel": F.y.to_numpy(), "day": T.day[a + F.k.to_numpy()].astype(np.int64)})
    for pre, idx in (("trigger", F.k.to_numpy()), ("fill", F.ei.to_numpy()), ("exit", F.x.to_numpy())):
        g = a + idx
        out[pre + "_slot"] = slot[g]; out[pre + "_tx"] = tx[g]; out[pre + "_leg"] = leg[g]
        out[pre + "_t_us"] = T.t_us[g]
    out.to_parquet(data_file("r1b_ref_%s.parquet" % name), index=False)
    print(name, "rule B tickets", len(out), summary(F, T.days), F.why.value_counts().to_dict())


if __name__ == "__main__":
    what = sys.argv[1]
    {"prep": prep, "check": check, "paths": paths, "alone": alone, "combine": combine, "book": book_one,
     "ref": ref}[what](*sys.argv[2:])
