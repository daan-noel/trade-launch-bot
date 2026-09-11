"""Rule 1 under the engine's exact semantics: the corrected reference (step 1 of
hunter/docs/roadmap/hot-tape-rule-1-engine-plan.md).

Every term, fill and clock is spelled the way the engine computes it (file references are the
code each line mirrors):

  sell        this print's SOL, a sell (m_flow_window, 1-print window)
  shold       seconds since the wallet behind this print last bought this coin (new metric)
  nb5         distinct build_core recipes in the closed window [now_ms - 5000, now_ms], the print
              INCLUDED, positions in floor-ms (WindowSpec::bounds, WindowSpec::pos)
  stall       max(0, now - time of the last strictly higher spot) (price_lifetime.rs)
  age         now - created_at (state.rs)
  buyers      distinct wallets, not the creator, with a buy at or after creation
              (crowd_after_age.rs, after_age_sec 0)
  vres        vsol after the print (liquidity = vsol - 30 <= 70)
  entry fill  LagMs(115) entry (paper_fill.rs find_paper_entry_at): the last non-dust BUY landed
              by trigger + 115 ms, in the trigger slot or the next slot holding such a buy when it
              is at most 3 slots on; none in that window, or none by the deadline -> the trigger
  exit        take profit / stop on pnl % against the entry print's spot, on every print after
              the entry fill; the clock (held >= clock) on a print or on the 200 ms tick grid
              (replay.rs: first event + 200 ms); a tick fires from the last folded print
  exit fill   LagMs(115) exit (find_paper_exit_at): the last print landed by fire + 115 ms, in the
              fire slot or the next observed slot when it is at most 3 slots on; none -> the fire
  re-entry    after the exit fill, at a print at or after its time (reentry cooldown 0)
  cost        the engine kernel (kernel.rs round_trip_multi_leg, pumpfun_impact): impact B/vsol
              on each leg at that leg's print, fee 125 bps on notional + proceeds, 0.000225 SOL a
              leg; y_curve is the study kernel's exact curve for comparison

Switches rebuild the old reading one piece at a time (the cost of each correction):
  holders "float" (the old bag-from-reserve book) | "buyers";  recipes_incl False | True;
  entry "last" (last print of either side, no slot window: the old reading) | "buy" (the
  engine's LagMs entry today) | "any" (the engine's LagMs EXIT rule applied to the entry leg);
  clock "deadline" | "tick"

  python r1_exact.py audit        the book after each correction, rule 1's thresholds held
  python r1_exact.py derive       re-derive every threshold and the exit on the study tape
"""
from __future__ import annotations

import _paths  # noqa: F401
from _paths import data_file

import bisect
import json
import sys
from collections import Counter
from dataclasses import dataclass, replace
from datetime import datetime, timezone

import numpy as np
import pandas as pd
import pyarrow.parquet as pq

from kernel import K, net
from cvx import DAY0
from toolkit import tapes, walkforward
from toolkit.book import ledger, mask

FEE = 0.0125
FIX = 0.000225
CLIP = 0.2
TICK_US = 200_000
MAX_FILL_WAIT_SLOTS = 3
MIN_TRADE_SOL = 10_000 / 1e9

# fire window ends (us); a tape not listed fires to its last print
T_MAX = {"study_exact": datetime(2026, 9, 6, 12, 0, tzinfo=timezone.utc).timestamp() * 1e6}

# candidate floor: the widest cut on any grid below, so a variant never needs a row cut here
FLOOR = dict(ssize=0.5, age=60.0, vres=116.0, nb5=8, stall=90.0, shold=300.0)


@dataclass(frozen=True)
class Sem:
    holders: str = "buyers"
    recipes_incl: bool = True
    entry: str = "buy"
    clock: str = "tick"
    lag_ms: int = 115


OLD = Sem(holders="float", recipes_incl=False, entry="last", clock="deadline")
NEW = Sem()


@dataclass(frozen=True)
class Exit:
    tp: float = 15.0
    sl: float = 40.0
    clock: float = 90.0


# ---------------------------------------------------------------- data

class Tape:
    """One tape's prints in chain order, one run per coin, with the per-coin creation and creator."""

    def __init__(self, name: str):
        prints, tokf, wfile, t_min = tapes.TAPES[name]
        cols = pq.read_schema(str(data_file(prints))).names
        want = ["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side", "wallet_id",
                "build"] + [c for c in ("t_us", "vtok") if c in cols]
        T = pq.read_table(str(data_file(prints)), columns=want).to_pandas()
        codes, mints = pd.factorize(T.mint, sort=False)
        chg = np.nonzero(np.diff(codes))[0] + 1
        self.start = np.r_[0, chg]; self.end = np.r_[chg, len(codes)]
        assert len(self.start) == len(mints), "a coin is split across runs"
        self.mints = np.asarray(mints)
        self.t_us = (T.t_us.to_numpy().astype(np.int64) if "t_us" in T
                     else T.t_ms.to_numpy().astype(np.int64) * 1000)
        self.slot = T.slot.to_numpy().astype(np.int64)
        self.v = T.reserve_lamports.to_numpy().astype(np.float64) / 1e9
        self.spot = (self.v / T.vtok.to_numpy().astype(np.float64) if "vtok" in T
                     else self.v * self.v / K)
        self.sol = T.amount_lamports.to_numpy().astype(np.float64) / 1e9
        self.side = T.side.to_numpy().astype(np.int8)
        self.wallet = T.wallet_id.to_numpy().astype(np.int64)
        bc, _ = pd.factorize(T.build, sort=False)
        self.build = bc.astype(np.int64)
        self.day = ((self.t_us // 1_000_000 - DAY0) // 86400).astype(np.int16)
        tok = pd.read_parquet(data_file(tokf)).drop_duplicates("mint").set_index("mint")
        c_us = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                - pd.Timestamp(0, tz="UTC")) // pd.Timedelta(microseconds=1)
        self.created_us = c_us.reindex(self.mints).to_numpy()
        cr = pd.read_parquet(data_file("r1_creators.parquet")).set_index("mint")
        if wfile is None:  # study tape: wallet_dict ids
            cw = cr.creator_wd
        else:              # lake tape: a code per address
            W = pd.read_parquet(data_file(wfile))
            code = pd.Series(W.wallet_id.to_numpy(), index=W.address.to_numpy())
            cw = cr.creator.map(code)
        self.creator = cw.reindex(self.mints).fillna(-1).to_numpy().astype(np.int64)
        self.t_min_us = -np.inf if t_min is None else t_min * 1e6
        self.t_max_us = T_MAX.get(name, np.inf)
        born_before = self.created_us < self.t_us.min()
        self.created_us = np.where(born_before, np.nan, self.created_us.astype(np.float64))
        # the replay's tick grid starts at its first event + one tick
        self.tick0 = int(self.t_us.min()) + TICK_US
        lo = max(self.t_us.min(), self.t_min_us)
        self.days = (min(self.t_us.max(), self.t_max_us) - lo) / 86400e6
        self.name = name


# ---------------------------------------------------------------- the walk: facts at candidate sells

def candidates(T: Tape, sem: Sem) -> pd.DataFrame:
    rows = []
    for r in range(len(T.start)):
        cu = T.created_us[r]
        if not np.isfinite(cu):
            continue
        a, b = int(T.start[r]), int(T.end[r])
        tu = T.t_us[a:b].tolist(); v = T.v[a:b].tolist(); sp = T.spot[a:b].tolist()
        sol = T.sol[a:b].tolist(); side = T.side[a:b].tolist(); wal = T.wallet[a:b].tolist()
        bu = T.build[a:b].tolist()
        creator = int(T.creator[r])
        lastbuy: dict = {}; buyers: set = set(); bag: dict = {}; fh = 0
        wpos: list = []; wbu: list = []; cnt: Counter = Counter()
        peak = -np.inf; peak_at = cu; ms_max = -1
        for i in range(b - a):
            t = tu[i]; ms = t // 1000; w = wal[i]
            while wpos and wpos[0] < ms - 5000:  # eviction at this print, permanent as in the engine
                wpos.pop(0); x = wbu.pop(0); cnt[x] -= 1
                if cnt[x] == 0:
                    del cnt[x]
            # recipes: the window BEFORE this print (old) or with it (engine)
            if not sem.recipes_incl:
                pre = _distinct(wpos, wbu, cnt, ms, ms_max)
            # fold print i
            if side[i] == 1:
                lastbuy[w] = t
                if sol[i] >= 0 and w != creator and t >= cu:
                    buyers.add(w)
            p = bag.get(w, 0.0)
            if sem.holders == "float":
                if side[i] == 1:
                    q = p + (K / (v[i] - sol[i]) - K / v[i])
                else:
                    q = max(p - (K / v[i] - K / (v[i] + sol[i])), 0.0)
                bag[w] = q
                hold_before = fh
                fh += (q > 0) - (p > 0)
            j = bisect.bisect_right(wpos, ms)
            wpos.insert(j, ms); wbu.insert(j, bu[i]); cnt[bu[i]] += 1
            ms_max = max(ms_max, ms)
            if sp[i] > peak:
                peak = sp[i]; peak_at = t
            # read on a candidate sell
            if side[i] != -1 or sol[i] < FLOOR["ssize"] or t < T.t_min_us or t >= T.t_max_us:
                continue
            age = (t - cu) / 1e6
            if age < FLOOR["age"] or v[i] > FLOOR["vres"]:
                continue
            lb = lastbuy.get(w)
            if lb is None:
                continue
            shold = (t - lb) / 1e6
            stall = max(0.0, (t - peak_at) / 1e6)
            if shold > FLOOR["shold"] or stall > FLOOR["stall"]:
                continue
            nb5 = _distinct(wpos, wbu, cnt, ms, ms_max) if sem.recipes_incl else pre
            if nb5 < FLOOR["nb5"]:
                continue
            hn = hold_before if sem.holders == "float" else len(buyers)
            rows.append((r, i, t, int(T.day[a + i]), sol[i], shold, nb5, stall, age, hn, v[i]))
    return pd.DataFrame(rows, columns=["run", "k", "t_us", "day", "ssize", "shold", "nb5", "stall",
                                       "age", "hold_n", "vres"])


def _distinct(wpos, wbu, cnt, ms, ms_max) -> int:
    """Distinct recipes with position in [ms - 5000, ms]; entries past `ms` (a regressed clock)
    are left out by an explicit scan, the rare case."""
    if ms >= ms_max:
        return len(cnt)
    j = bisect.bisect_right(wpos, ms)
    return len(set(x for p, x in zip(wpos[:j], wbu[:j]) if p >= ms - 5000))


# ---------------------------------------------------------------- fills and exits

def entry_fill(T: Tape, a: int, n: int, k: int, sem: Sem) -> int:
    t = T.t_us; s = T.slot
    dl = t[a + k] + sem.lag_ms * 1000
    if sem.entry == "last":  # the old reading: the last print of either side landed by the deadline
        x = k
        while x + 1 < n and t[a + x + 1] <= dl:
            x += 1
        return x
    if sem.entry == "any":  # the exit leg's rule (find_paper_exit_at) on the entry leg
        return exit_fill(T, a, n, k, replace(sem, clock="tick"))
    sk = s[a + k]; nxt = None; last = None; any_q = False
    i = k + 1
    while i < n and s[a + i] <= sk + MAX_FILL_WAIT_SLOTS:
        si = s[a + i]
        eb = T.side[a + i] == 1 and T.sol[a + i] >= MIN_TRADE_SOL
        if eb and si > sk and nxt is None:
            nxt = si
        if eb and (si == sk or si == nxt):
            any_q = True
            if t[a + i] <= dl:
                last = i
        if nxt is not None and si > nxt:
            break
        i += 1
    if not any_q or last is None:
        return k
    return last


def exit_fill(T: Tape, a: int, n: int, f: int, sem: Sem) -> int:
    t = T.t_us; s = T.slot
    dl = t[a + f] + sem.lag_ms * 1000
    if sem.clock == "deadline":  # the old reading
        x = f
        while x + 1 < n and t[a + x + 1] <= dl:
            x += 1
        return x
    sf = s[a + f]
    if f + 1 >= n:
        return f
    nxt = next((s[a + i] for i in range(f + 1, n) if s[a + i] > sf), None)
    win = {sf} | ({nxt} if nxt is not None and nxt <= sf + MAX_FILL_WAIT_SLOTS else set())
    last = None
    i = f + 1
    while i < n and s[a + i] in win:
        if t[a + i] <= dl:
            last = i
        i += 1
    return last if last is not None else f


def y_engine(s0, v0, s1, v1, b=CLIP):
    eff_in = s0 * (1.0 + b / v0)
    tok = b / eff_in
    proceeds = tok * s1 * max(1.0 - b / v1, 0.0)
    return proceeds - b - ((b + proceeds) * FEE + 2 * FIX)


def book_exits(T: Tape, C: pd.DataFrame, ex: Exit, sem: Sem, b: float = CLIP) -> pd.DataFrame:
    """Entry fill, exit decision and exit fill for every candidate row (occupancy comes later)."""
    out = np.zeros((len(C), 7))
    why = np.empty(len(C), dtype=object)
    clock_us = int(round(ex.clock * 1e6))
    runs = C.run.to_numpy(); ks = C.k.to_numpy()
    for idx in range(len(C)):
        r = runs[idx]; k = int(ks[idx])
        a = int(T.start[r]); n = int(T.end[r]) - a
        ei = entry_fill(T, a, n, k, sem)
        s0 = T.spot[a + ei]; t0 = T.t_us[a + ei]
        dl = t0 + clock_us
        lo = a + ei + 1
        hi = a + n
        # first print that trips: pnl, or held >= clock (scanned in chunks until one trips)
        istar = None; pstar = 0.0; i0 = lo
        while i0 < hi:
            i1 = min(i0 + 512, hi)
            pnl = (T.spot[i0:i1] - s0) / s0 * 100.0
            trip = (pnl >= ex.tp) | (pnl <= -ex.sl) | (T.t_us[i0:i1] - t0 >= clock_us)
            nz = np.flatnonzero(trip)
            if len(nz):
                istar = i0 - a + int(nz[0]); pstar = float(pnl[nz[0]]); break
            i0 = i1
        if sem.clock == "deadline":
            if istar is not None and T.t_us[a + istar] - t0 < clock_us:
                f = istar; w = "tp" if pstar >= ex.tp else "sl"
                x = exit_fill(T, a, n, f, sem)
            else:  # the state landed by deadline + lag, never before the entry
                w = "time"; x = ei
                while x + 1 < n and T.t_us[a + x + 1] <= dl + sem.lag_ms * 1000:
                    x += 1
        else:
            tau = T.tick0 + -(-(dl - T.tick0) // TICK_US) * TICK_US
            if istar is not None and T.t_us[a + istar] <= tau:
                f = istar
                w = "sl" if pstar <= -ex.sl else ("tp" if pstar >= ex.tp else "time")
            else:
                f = (istar - 1) if istar is not None else n - 1
                w = "time"
            x = exit_fill(T, a, n, f, sem)
        s1 = T.spot[a + x]; v0 = T.v[a + ei]; v1 = T.v[a + x]
        out[idx] = (ei, x, T.t_us[a + x], (T.t_us[a + x] - t0) / 1e6,
                    y_engine(s0, v0, s1, v1, b), net(v0, v1, b), v1)
        why[idx] = w
    D = C.copy()
    D["ei"] = out[:, 0].astype(np.int64); D["x"] = out[:, 1].astype(np.int64)
    D["t_x"] = out[:, 2].astype(np.int64); D["hold"] = out[:, 3]
    D["y"] = out[:, 4]; D["y_curve"] = out[:, 5]; D["v1"] = out[:, 6]; D["why"] = why
    D["t"] = D.t_us / 1e6
    D["grad"] = (D.v1 >= 114.0) & (D.x == (T.end[D.run.to_numpy()] - T.start[D.run.to_numpy()] - 1))
    return D


def occupy(C: pd.DataFrame, m: np.ndarray) -> pd.DataFrame:
    """One position per coin; a fire needs a print after the exit fill, at or after its time."""
    run = C.run.to_numpy(); k = C.k.to_numpy(); x = C.x.to_numpy()
    t = C.t_us.to_numpy(); tx = C.t_x.to_numpy()
    keep = np.zeros(len(C), dtype=bool)
    cur = -1; last = -1; t_ok = -1
    for i in np.flatnonzero(m):
        if run[i] != cur:
            cur = run[i]; last = -1; t_ok = -1
        if k[i] <= last or t[i] < t_ok:
            continue
        keep[i] = True; last = x[i]; t_ok = tx[i]
    return C[keep]


def fires(C, spec):
    return occupy(C, mask(C, spec))


walkforward.fires = fires  # the keep rule, booked with this module's occupancy

RULE1 = {"ssize": (">=", 1.0), "nb5": (">=", 15), "stall": ("<=", 20.0), "shold": ("<=", 30.0),
         "age": (">=", 158.0), "hold_n": (">=", 368), "vres": ("<=", 100.0)}
GRID = {
    "ssize": [0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 2.5, 3.0, 4.0],
    "nb5": [8, 10, 12, 13, 14, 15, 16, 17, 18, 20, 22, 25, 30],
    "stall": [2.0, 5.0, 10.0, 15.0, 20.0, 30.0, 45.0, 60.0, 90.0],
    "shold": [5.0, 10.0, 15.0, 20.0, 30.0, 45.0, 60.0, 90.0, 120.0, 180.0, 300.0],
    "age": [60.0, 90.0, 120.0, 158.0, 200.0, 250.0, 300.0, 400.0, 600.0],
    "hold_n": [0, 50, 100, 150, 200, 250, 300, 368, 450, 550, 700, 900],
    "vres": [85.0, 90.0, 95.0, 100.0, 104.0, 107.2, 110.0, 116.0],
}
EXIT_GRID = {"tp": [10.0, 12.5, 15.0, 17.5, 20.0, 25.0], "sl": [25.0, 30.0, 40.0, 50.0, 60.0],
             "clock": [60.0, 90.0, 120.0, 180.0]}


def summary(F: pd.DataFrame, days: float, b: float = CLIP) -> dict:
    L = ledger(F, days, b)
    return {k: L.get(k) for k in ("n", "nday", "pct", "sol", "pos", "worst", "body", "top1",
                                  "maxcoin", "h1", "h2", "sl", "win", "cap_sol")} | {
        "grad": int(F.grad.sum()) if len(F) else 0,
        "pct_curve": round(100 * float(F.y_curve.mean()) / b, 2) if len(F) else np.nan}


def cache_path(name, sem, ex):
    return data_file("r1x_%s_%s_%s_%s_%s_%d_%g_%g_%g.parquet" % (
        name, sem.holders, int(sem.recipes_incl), sem.entry, sem.clock, sem.lag_ms,
        ex.tp, ex.sl, ex.clock))


def table(T: Tape, sem: Sem, ex: Exit, cands: dict) -> pd.DataFrame:
    key = (sem.holders, sem.recipes_incl)
    if key not in cands:
        cands[key] = candidates(T, sem)
    return book_exits(T, cands[key], ex, sem)


def audit(names=("study", "holdout", "holdout_legs")) -> None:
    steps = [("old reading (evidence 1.21)", OLD),
             ("+ holders -> distinct buyers", replace(OLD, holders="buyers")),
             ("+ recipes include the print", replace(OLD, holders="buyers", recipes_incl=True)),
             ("+ engine entry fill (buy only)", replace(OLD, holders="buyers", recipes_incl=True, entry="buy")),
             ("+ engine clock and exit fill", NEW)]
    for name in names:
        T = Tape(name); cands = {}
        rows = []
        for label, sem in steps:
            C = table(T, sem, Exit(), cands)
            F = fires(C, RULE1)
            rows.append(dict(tape=name, step=label, **summary(F, T.days)))
        print(pd.DataFrame(rows).to_string(index=False), flush=True)


def bootstrap(F: pd.DataFrame, b=CLIP, draws=4000, seed=7) -> tuple:
    g = F.groupby("run").y.agg(["sum", "size"])
    s = g["sum"].to_numpy(); c = g["size"].to_numpy()
    rng = np.random.default_rng(seed)
    idx = rng.integers(0, len(s), size=(draws, len(s)))
    m = s[idx].sum(1) / c[idx].sum(1) / b * 100
    return round(float(np.percentile(m, 2.5)), 2), round(float(np.percentile(m, 97.5)), 2), \
        float((m <= 0).mean())


def bars_fail(F: pd.DataFrame, days: float) -> str:
    """The ship bars on the study book: every day positive, both halves and the body above zero,
    top 1 % and the biggest coin at most 15 % of net. Empty string = pass."""
    L = ledger(F, days)
    p, n = L["pos"].split("/")
    bad = []
    if p != n: bad.append("days %s" % L["pos"])
    if not (L["h1"] > 0 and L["h2"] > 0): bad.append("halves")
    if not L["body"] > 0: bad.append("body")
    if not (L["top1"] <= 15): bad.append("top1 %s" % L["top1"])
    if not (L["maxcoin"] <= 15): bad.append("maxcoin %s" % L["maxcoin"])
    return ", ".join(bad)


def entry_rounds(C: pd.DataFrame, days: float, spec: dict, rounds: int = 4) -> dict:
    """walkforward's keep rule, one round after another; a taken change must also beat the
    5 % chance floor on both test halves (cut_noise) and keep the ship bars."""
    for rnd in range(1, rounds + 1):
        W, _ = walkforward.thresholds(C, days, spec, GRID)
        Fb = fires(C, spec)
        noise = walkforward.cut_noise(Fb)
        print("\n--- entry round %d from %s  (noise per test half %s)" % (
            rnd, {k: v[1] for k, v in spec.items()}, noise))
        print(W.to_string(index=False))
        changed = False
        for _, row in W[W["take"]].iterrows():
            gains = []
            for fs in (row.fold1, row.fold2):
                a_, b_ = fs.split("test ")[1].split(" vs ")
                gains.append(float(a_) - float(b_))
            new = dict(spec, **{row.term: (spec[row.term][0], row.value)})
            why = bars_fail(fires(C, new), days)
            if gains[0] <= noise[0] or gains[1] <= noise[1]:
                print("  %s -> %g refused: test gains %+.2f / %+.2f inside the chance floor"
                      % (row.term, row.value, gains[0], gains[1]))
            elif why:
                print("  %s -> %g refused: bars %s" % (row.term, row.value, why))
            else:
                print("  %s -> %g TAKEN (test gains %+.2f / %+.2f)" % (row.term, row.value, gains[0], gains[1]))
                spec = new; changed = True
        if not changed:
            break
    return spec


def derive(mode: str = "buy", study: str = "study") -> None:
    pd.set_option("display.width", 320); pd.set_option("display.max_rows", 400)
    NEW = replace(globals()["NEW"], entry=mode)
    T = Tape(study); cands = {}
    spec = dict(RULE1); ex = Exit()
    log = []
    for rnd in range(1, 4):
        C = table(T, NEW, ex, cands)
        spec = entry_rounds(C, T.days, spec)
        log.append(("entry", rnd, {k: v[1] for k, v in spec.items()}))
        # the exit, one axis at a time, on the new pool
        base = fires(C, spec)
        fl = walkforward.folds(base)
        cur = [ex.tp, ex.sl, ex.clock]; changed_any = False
        books = {}

        def bk(key):
            if key not in books:
                Ck = table(T, NEW, Exit(*key), cands)
                books[key] = fires(Ck, spec)
            return books[key]
        for _ in range(3):
            changed = False
            for ai, (nm, g) in enumerate(EXIT_GRID.items()):
                picks = []
                for fit, test in fl:
                    ok = [val for val in g if walkforward.allpos(bk(tuple(cur[:ai] + [val] + cur[ai + 1:])), fit)]
                    best = max(ok, key=lambda val: walkforward.solsum(
                        bk(tuple(cur[:ai] + [val] + cur[ai + 1:])), fit)) if ok else cur[ai]
                    picks.append((best, walkforward.solsum(bk(tuple(cur[:ai] + [best] + cur[ai + 1:])), test),
                                  walkforward.solsum(bk(tuple(cur)), test)))
                same = all(p[0] > cur[ai] for p in picks) or all(p[0] < cur[ai] for p in picks)
                beats = all(p[1] > p[2] for p in picks)
                print("exit %s now %g: fold1 %s  fold2 %s  take %s" % (nm, cur[ai], picks[0], picks[1], same and beats))
                if same and beats:
                    val = walkforward._snap(g, [p[0] for p in picks], cur[ai])
                    why = bars_fail(bk(tuple(cur[:ai] + [val] + cur[ai + 1:])), T.days)
                    if why:
                        print("  exit %s -> %g refused: bars %s" % (nm, val, why))
                    else:
                        cur[ai] = val; changed = True; changed_any = True
            if not changed:
                break
        ex = Exit(*cur)
        log.append(("exit", rnd, ex))
        if not changed_any:
            break
    C = table(T, NEW, ex, cands)
    F = fires(C, spec)
    print("\n=== derived: %s  exit %s" % ({k: v for k, v in spec.items()}, ex))
    print("study", summary(F, T.days), "noise", walkforward.cut_noise(F))
    data_file("r1x_rule_%s%s.json" % (mode, "" if study == "study" else "_" + study)).write_text(json.dumps(
        {"entry": {k: list(v) for k, v in spec.items()}, "exit": [ex.tp, ex.sl, ex.clock]}))


def confirm(mode: str = "buy", study: str = "study", *names) -> None:
    names = names or ("study", "holdout", "holdout_legs", "holdout_exact")
    NEW = replace(globals()["NEW"], entry=mode)
    tag = "" if study == "study" else "_" + study
    R = json.loads(data_file("r1x_rule_%s%s.json" % (mode, tag)).read_text())
    spec = {k: tuple(v) for k, v in R["entry"].items()}; ex = Exit(*R["exit"])
    print("rule", spec, ex)
    for name in names:
        T = Tape(name); cands = {}
        rows = []
        for lag in (115, 200, 300, 500):
            sem = replace(NEW, lag_ms=lag)
            C = table(T, sem, ex, cands)
            F = fires(C, spec)
            lo, hi, p0 = bootstrap(F)
            rows.append(dict(tape=name, lag=lag, **summary(F, T.days), ci="%+.2f..%+.2f" % (lo, hi), p0=p0))
            if lag == 115:
                g = F.groupby("day").agg(n=("y", "size"), sol=("y", "sum"))
                print("[%s] per day %s" % (name, "  ".join("d%d:%d/%+.2f" % (d, n_, s_) for d, (n_, s_) in g.iterrows())))
                F.assign(mint=T.mints[F.run.to_numpy()]).to_parquet(
                    data_file("r1x_tickets_%s%s_%s.parquet" % (mode, tag, name)), index=False)
                C35 = book_exits(T, cands[(sem.holders, sem.recipes_incl)], ex, sem, b=0.35)
                F35 = fires(C35, spec)
                print("[%s] at 0.35 SOL %s" % (name, summary(F35, T.days, 0.35)))
        print(pd.DataFrame(rows).to_string(index=False), flush=True)


if __name__ == "__main__":
    what = sys.argv[1] if len(sys.argv) > 1 else "audit"
    args = sys.argv[2:]
    {"audit": audit, "derive": derive, "confirm": confirm}[what](*args)
