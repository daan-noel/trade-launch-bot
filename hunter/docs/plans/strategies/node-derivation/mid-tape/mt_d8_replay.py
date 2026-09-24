"""Rule 3b audited, then booked on every lake day from 09-01 (derive 12.10-12.11).

Independent of the study: no import of toolkit/, kernel.py or the candidate tables. It reads the
sealed lake (every curve row, every leg, every wallet), rebuilds every term per coin from the prints
before the fire, books each fill and exit itself, and first reproduces the study's tickets one for
one (the OLD spelling), then changes one thing at a time.

Rule 3b (mt_d8_e/d/x/p): a buy that is larger than its build recipe's last buy on the coin, with
  SOL >= 0.15 (the candidate table's floor), own price step >= 4.4 %, vsol after it <= 40, a wallet
  with no earlier print on the coin, age >= 1 s  (E)
  holders before it <= 46  (D)
  sells >= 1 SOL in the 30 s before it <= 4, prints in the 5 s before it <= 25  (P)
  take profit +40 %, stop -30 %, clock 17 s  (X);  one entry per coin (R);  0.2 SOL (S)

Spellings, OLD (the study, reproduced) -> NEW (a live engine):
  trigger    not one of the 26 roster wallets             -> any wallet
  build      missing labels are one recipe ("")           -> missing labels are no recipe
  holders    reserve-sized float bags > 0, roster out     -> distinct buyers before it, creator out
             (also read: exact token_amount holders, every wallet)
  nbig30     roster sells out                             -> every sell
  universe   coin needs >= 60 prints and >= 60 s of life  -> no filter on the coin's future
             (the candidate table's floor, read at the END of the coin's life)
  fills      last print landed by t + lag (ms clock)      -> the engine's LagMs rule on both legs:
                                                             last print landed by t + lag in the
                                                             fire's slot or the next observed slot
                                                             <= 3 slots on; none -> the fire print
  clock      from the last print before entry + 17 s,    -> held >= 17 s on a print, or the 200 ms
             filled at that print + lag                     tick after it; filled by LagMs from the
                                                             last folded print (r1_exact.py)
  cost       exact curve on vsol, 125 bps a leg,         -> the engine kernel on spot vsol/vtok
             0.000225 SOL a leg                             (r1_exact.y_engine)
  dead coin  none                                         -> a fire after the engine's dead verdict
                                                             (real SOL < 30 and no trade >= 0.1 SOL
                                                             for 300 s; deadness.rs) cannot happen

  python mt_d8_replay.py slim        one slim file per lake day (build codes, wallet hashes)
  python mt_d8_replay.py cands       per bucket: every E print with every spelling of every term,
                                     and the exits of each coin's first fire under any spelling
  python mt_d8_replay.py repro       the OLD spelling against the study's ticket list
  python mt_d8_replay.py book        the corrections one at a time, then the wide book
"""
from __future__ import annotations

import _paths  # noqa: F401
from _paths import DATA, LAKE, LOCAL

import hashlib
import json
import sys
import time
from datetime import datetime, timezone

import numpy as np
import pandas as pd
import pyarrow as pa
import pyarrow.parquet as pq

pd.set_option("display.width", 320)
pd.set_option("display.max_columns", 40)
pd.set_option("display.max_rows", 200)

SLIM = DATA / "mt_d8_lake"
NB = 16
DAYS = ["2026-09-%02d" % d for d in range(1, 15)]
UTC = timezone.utc
LAKE0 = datetime(2026, 9, 1, tzinfo=UTC).timestamp()
STUDY_END = datetime(2026, 9, 6, 12, tzinfo=UTC).timestamp()
HOLD_END = datetime(2026, 9, 11, tzinfo=UTC).timestamp()
FIRE_END = datetime(2026, 9, 14, tzinfo=UTC).timestamp()        # 09-14 is partial: exits only
DAY0 = datetime(2026, 8, 30, tzinfo=UTC).timestamp()             # the study's day index origin

K = 3.219e16
FEE = 0.0125
FIX = 0.000225
CLIP = 0.2
TP, SL, CLOCK = 40.0, 30.0, 17.0
MAX_WAIT_SLOTS = 3
TICK_US = 200_000
LAGS_MS = (50, 83, 115, 200, 300)
DEAD_LIQ, DEAD_QUIET, DEAD_MEAN = 30.0, 300.0, 0.1


# ------------------------------------------------------------------ slim

def _core(lab):
    """(study build, engine build) of one ix_labels JSON: md5 of the labels without account
    setup / teardown / memo (lake_export.build_core, engine flow_ix::build_hash)."""
    if lab is None:
        return 1, -1
    labs = json.loads(lab)
    keep = [x for x in labs if not (x.startswith("Associated Token: Create") or x.endswith(": CloseAccount")
                                    or x.startswith("Memo Program"))]
    h = int(hashlib.md5("|".join(keep).encode()).hexdigest()[:15], 16) + 2
    return h, (h if len(labs) else -1)


def _hash(a):
    return pd.util.hash_array(np.asarray(a, dtype=object)).view(np.int64)


def slim():
    SLIM.mkdir(exist_ok=True)
    roster = pd.read_csv(LOCAL / "solo-traders.csv", usecols=["wallet_address", "node", "use_it"])
    roster = roster[(roster.node == "mid-tape one-shot") & (roster.use_it == "yes")].wallet_address
    node = set(_hash(roster.to_numpy()).tolist())
    cols = ["mint", "is_buy", "sol_amount", "token_amount", "slot", "block_time", "leg_index", "tx_index",
            "vsol", "vtok", "venue", "ix_labels", "wallet"]
    for day in DAYS:
        out = SLIM / ("%s.parquet" % day)
        if out.exists() and day != DAYS[-1]:
            continue
        t0 = time.time()
        pf = pq.ParquetFile(str(LAKE / "trades" / ("dt=%s" % day) / "data.parquet"))
        cache = {}
        w = None
        n = 0
        for b in pf.iter_batches(batch_size=300_000, columns=cols):
            d = b.to_pandas()
            d = d[(d.venue == "curve") & (d.vsol > 0) & (d.vtok > 0)]
            codes, uniq = pd.factorize(d.ix_labels, use_na_sentinel=True)
            cc = []
            for u in uniq:
                if u not in cache:
                    cache[u] = _core(u)
                cc.append(cache[u])
            cc = np.array(cc + [(1, -1)], dtype=np.int64)      # the sentinel row: missing labels
            cc = cc[np.where(codes >= 0, codes, len(uniq))]
            wal = d.wallet.fillna("").to_numpy()
            wh = _hash(wal)
            mint = d.mint.to_numpy()
            T = pa.table({
                "mint": pa.array(mint).dictionary_encode(),
                "bucket": pa.array((_hash(mint).view(np.uint64) % NB).astype(np.int8)),
                "slot": d.slot.to_numpy(np.int64), "txi": d.tx_index.to_numpy(np.int32),
                "leg": d.leg_index.to_numpy(np.int32), "t_us": d.block_time.to_numpy(np.int64),
                "vsol": d.vsol.to_numpy(np.float64), "vtok": d.vtok.to_numpy(np.float64),
                "sol": d.sol_amount.to_numpy(np.float64), "tok": d.token_amount.to_numpy(np.float64),
                "side": np.where(d.is_buy.to_numpy(), 1, -1).astype(np.int8),
                "wal": wh, "nowal": (wal == ""), "node": np.isin(wh, list(node)),
                "btk": cc[:, 0], "beng": cc[:, 1]})
            if w is None:
                w = pq.ParquetWriter(str(out), T.schema)
            w.write_table(T)
            n += len(d)
        w.close()
        print("slim %s  %d rows  %.0fs" % (day, n, time.time() - t0), flush=True)


# ------------------------------------------------------------------ per bucket

def load_bucket(b):
    parts = [pq.read_table(str(SLIM / ("%s.parquet" % d)), filters=[("bucket", "=", b)]) for d in DAYS]
    D = pa.concat_tables(parts).to_pandas()
    D["mint"] = D.mint.astype(str)
    D = D.sort_values(["mint", "slot", "txi", "leg"], kind="stable").reset_index(drop=True)
    return D


def tokens():
    T = pq.read_table(str(LAKE / "tokens" / "tokens.parquet"), columns=["mint", "created_at",
                                                                          "fp_initial_buy_sol"]).to_pandas()
    return T.drop_duplicates("mint").set_index("mint")


def _grp_first(key_a, key_b):
    """First occurrence flag of (a, b) pairs, in row order."""
    return ~pd.DataFrame({"a": key_a, "b": key_b}).duplicated().to_numpy()


def coin_facts(D, tok):
    """Vectorised facts over one bucket (rows in chain order, one run per coin)."""
    codes, mints = pd.factorize(D.mint, sort=False)
    n = len(D)
    chg = np.r_[0, np.nonzero(np.diff(codes))[0] + 1]
    start = chg; end = np.r_[chg[1:], n]
    run = codes.astype(np.int64)
    first = np.zeros(n, dtype=bool); first[start] = True
    tk = tok.reindex(mints)
    created_us = tk.created_at.to_numpy(np.float64)
    init_buy = tk.fp_initial_buy_sol.to_numpy(np.float64)
    t_us = D.t_us.to_numpy(np.int64)
    # the study's arrays: lamport-rounded reserve and amount, ms clock
    v_old = np.round(D.vsol.to_numpy() * 1e9) / 1e9
    sol_old = np.round(D.sol.to_numpy() * 1e9) / 1e9
    t_ms = (t_us + 500) // 1000
    t_old = t_ms.astype(np.float64) / 1000.0
    big = np.int64(2 ** 31)
    key = run * big + (t_ms - int(LAKE0 * 1000))              # integer ms: exact window edges
    tm_key = np.maximum.accumulate(key)                       # monotone clock inside each coin
    tm = (tm_key - run * big + int(LAKE0 * 1000)).astype(np.float64) / 1000.0
    v_prev = np.r_[v_old[0], v_old[:-1]]
    mvk = np.where(first, 0.0, ((v_old / v_prev) ** 2 - 1.0) * 100.0)
    side = D.side.to_numpy(np.int8)
    wal = D.wal.to_numpy(np.int64)
    node = D.node.to_numpy(bool)
    wal_n = pd.DataFrame({"r": run, "w": wal}).groupby(["r", "w"], sort=False).cumcount().to_numpy()
    step = {}
    for nm, col in (("tk", "btk"), ("eng", "beng")):
        bu = D[col].to_numpy(np.int64)
        buys = np.nonzero(side == 1)[0]
        B = pd.DataFrame({"r": run[buys], "b": bu[buys], "s": sol_old[buys]})
        prev = B.groupby(["r", "b"], sort=False).s.shift().to_numpy()
        st = np.zeros(n, dtype=bool)
        st[buys] = np.isfinite(prev) & (sol_old[buys] > np.nan_to_num(prev, nan=np.inf))
        if nm == "eng":
            st &= bu != -1
        step[nm] = st
    # windows on the monotone clock: [tm - w, tm), this print left out
    j5 = np.searchsorted(tm_key, tm_key - 5000, side="left")
    j30 = np.searchsorted(tm_key, tm_key - 30000, side="left")
    idx = np.arange(n)
    bigsell = (side == -1) & (sol_old >= 1.0)
    cb_all = np.r_[0, np.cumsum(bigsell)]
    cb_pub = np.r_[0, np.cumsum(bigsell & ~node)]
    # distinct buyers before the print (every wallet), creator (the coin's first print when it is the
    # create-and-buy) left out
    fb = (side == 1) & _grp_first(run, np.where(side == 1, wal, -7))
    fb &= side == 1
    cfb = np.cumsum(fb) - fb                                   # exclusive
    cfb_coin = cfb - cfb[start][run]
    creator = np.where((side[start] == 1) & (init_buy > 0), wal[start], 0)[run]
    cr_bought = (wal == creator) & fb
    ccr = np.cumsum(cr_bought) - cr_bought
    ccr_coin = ccr - ccr[start][run]
    F = pd.DataFrame(dict(
        run=run, k=idx - start[run], mint_i=run, t_us=t_us, t=t_old, tm=tm, slot=D.slot.to_numpy(),
        txi=D.txi.to_numpy(), leg=D.leg.to_numpy(), side=side, ssize=sol_old, mvk=mvk, vres=v_old,
        wal_n=wal_n, node=node, step_tk=step["tk"], step_eng=step["eng"],
        age_old=tm - created_us[run] / 1e6, age_new=(t_us - created_us[run]) / 1e6,
        np5=idx - j5, nbig30_all=cb_all[idx] - cb_all[j30], nbig30_pub=cb_pub[idx] - cb_pub[j30],
        buyers=cfb_coin - ccr_coin, buyers_cr=cfb_coin))
    return F, mints, start, end, created_us


def loops(D, start, end, cand_runs, cand_k):
    """Per coin, up to its last candidate: the study's float holder book (roster out) and the exact
    token_amount holder book (every wallet), both BEFORE each print."""
    v = np.round(D.vsol.to_numpy() * 1e9) / 1e9
    sol = np.round(D.sol.to_numpy() * 1e9) / 1e9
    side = D.side.to_numpy(); wal = D.wal.to_numpy(); node = D.node.to_numpy(); tk = D.tok.to_numpy()
    vbef = v - sol * side
    with np.errstate(divide="ignore", invalid="ignore"):
        tokd = np.where(side == 1, K / vbef - K / v, K / v - K / vbef)
    tokd = np.nan_to_num(tokd, nan=0.0, posinf=0.0, neginf=0.0)
    out = {}
    for r, ks in zip(cand_runs, cand_k):
        a = int(start[r]); last = int(max(ks)); want = set(int(x) for x in ks)
        fpos = {}; epos = {}; fh = 0; eh = 0
        s_ = side[a:a + last + 1].tolist(); w_ = wal[a:a + last + 1].tolist()
        nd = node[a:a + last + 1].tolist(); td = tokd[a:a + last + 1].tolist(); ta = tk[a:a + last + 1].tolist()
        for i in range(last + 1):
            if i in want:
                out[(r, i)] = (fh, eh)
            w = w_[i]
            p = fpos.get(w, 0.0)
            q = p + td[i] if s_[i] == 1 else max(p - td[i], 0.0)
            fpos[w] = q
            if not nd[i]:
                fh += int(q > 0) - int(p > 0)
            p = epos.get(w, 0.0)
            q = p + ta[i] if s_[i] == 1 else max(p - ta[i], 0.0)
            epos[w] = q
            eh += int(q > 0) - int(p > 0)
    return out


# ------------------------------------------------------------------ fills, exits, cost

def net_curve(v0, v1, b=CLIP):
    s = b / (1.0 + FEE)
    tok = K / v0 - K / (v0 + s)
    return (v1 - K / (K / v1 + tok)) * (1.0 - FEE) - b - 2.0 * FIX


def y_engine(s0, v0, s1, v1, b=CLIP):
    """kernel.rs `round_trip_with_costs`, spelled in spot prices as `net_curve` above spells it
    in reserves: the venue fee comes OFF THE TOP of the buy, so only `b / (1 + FEE)` reaches the
    curve. Charging it at the end instead leaves the bag 1.25 % too large, which the engine's own
    replay of rule 1 prices at about 2 % of a book."""
    curve_sol = b / (1.0 + FEE)
    tok = curve_sol / (s0 * (1.0 + curve_sol / v0))
    value = max(tok * s1, 0.0)
    return (value / (1.0 + value / v1)) * (1.0 - FEE) - b - 2.0 * FIX


def exit_old(t, v, k, lag):
    """The study's reading: fills are the last print landed by t + lag; the clock ends at the last
    print before entry + 17 s and fills lag after THAT print (mt_d8_x.its_cut)."""
    n = len(t)
    ei = max(k, int(np.searchsorted(t, t[k] + lag, side="right")) - 1)
    v0 = v[ei]
    e = int(np.searchsorted(t, t[ei] + CLOCK, side="right"))
    jj = max(e - 1, ei)
    why = "time"
    for j in range(ei + 1, e):
        pr = (v[j] / v0) ** 2 - 1.0
        if pr >= TP / 100 or pr <= -SL / 100:
            jj = j; why = "tp" if pr > 0 else "sl"; break
    x = max(jj, int(np.searchsorted(t, t[jj] + lag, side="right")) - 1)
    return net_curve(v0, v[x]), ei, x, why, t[x] - t[ei]


def lagms(tu, s, f, lag_us):
    """The engine's LagMs fill (paper_fill.rs find_paper_exit_at; the entry leg uses it since 09-11)."""
    n = len(tu)
    dl = tu[f] + lag_us
    if f + 1 >= n:
        return f
    nxt = None
    i = f + 1
    while i < n:
        if s[i] > s[f]:
            nxt = s[i]; break
        i += 1
    win_hi = nxt if (nxt is not None and nxt <= s[f] + MAX_WAIT_SLOTS) else s[f]
    last = f
    i = f + 1
    while i < n and s[i] <= win_hi:
        if tu[i] <= dl:
            last = i
        i += 1
    return last


def exit_new(tu, s, v, sp, k, lag_ms, clock="tick"):
    """The engine's reading: LagMs on both legs, pnl on spot, clock on a print or the 200 ms tick."""
    n = len(tu)
    lag_us = int(lag_ms * 1000)
    ei = lagms(tu, s, k, lag_us)
    s0 = sp[ei]; t0 = tu[ei]; cl = int(CLOCK * 1e6)
    istar = None; pstar = 0.0
    for i in range(ei + 1, n):
        pnl = (sp[i] - s0) / s0 * 100.0
        if pnl >= TP or pnl <= -SL or tu[i] - t0 >= cl:
            istar = i; pstar = pnl; break
    if clock == "tick":
        dl = t0 + cl
        tau = -(-dl // TICK_US) * TICK_US
        if istar is not None and tu[istar] <= tau:
            f = istar
            why = "sl" if pstar <= -SL else ("tp" if pstar >= TP else "time")
        else:
            f = (istar - 1) if istar is not None else n - 1
            why = "time"
        x = lagms(tu, s, f, lag_us)
    else:  # deadline: the state landed by entry + 17 s + lag
        if istar is not None and tu[istar] - t0 < cl:
            f = istar; why = "tp" if pstar >= TP else "sl"
            x = lagms(tu, s, f, lag_us)
        else:
            why = "time"
            x = max(ei, int(np.searchsorted(tu, t0 + cl + lag_us, side="right")) - 1)
    return y_engine(s0, v[ei], sp[x], v[x]), ei, x, why, (tu[x] - t0) / 1e6


def dead_after(tu, sol, vsol, created_us):
    """The first instant the engine's dead verdict holds (deadness.rs): real SOL (vsol - 30) < 30
    and no trade >= 0.1 SOL for 300 s. Read at each quiet stretch's 300 s mark, with the reserve of
    the last print landed by then. inf when never."""
    mt = tu[sol >= DEAD_MEAN]
    anchors = np.r_[created_us, mt] if np.isfinite(created_us) else mt
    if not len(anchors):
        return np.inf
    nxt = np.r_[anchors[1:], np.inf]
    gap = nxt - anchors
    for a, g in zip(anchors[gap >= DEAD_QUIET * 1e6], gap[gap >= DEAD_QUIET * 1e6]):
        at = a + DEAD_QUIET * 1e6
        j = int(np.searchsorted(tu, at, side="right")) - 1
        if j >= 0 and vsol[j] - 30.0 < DEAD_LIQ:
            return at
    return np.inf


def cands(b, lag_draws):
    t0 = time.time()
    D = load_bucket(b)
    tok = tokens()
    F, mints, start, end, created_us = coin_facts(D, tok)
    nrun = len(start)
    # the coin's future, as the study table read it (prints on the study tape, to 09-07 00:00)
    life_n = np.zeros(nrun, dtype=np.int64); life_age = np.zeros(nrun)
    st_end = datetime(2026, 9, 7, tzinfo=UTC).timestamp()
    in_study = F.t.to_numpy() < st_end
    cnt = np.bincount(F.run.to_numpy()[in_study], minlength=nrun)
    life_n[:] = cnt
    last_tm = pd.Series(F.tm.to_numpy()[in_study]).groupby(F.run.to_numpy()[in_study]).max()
    life_age[:] = np.nan
    life_age[last_tm.index.to_numpy()] = last_tm.to_numpy() - created_us[last_tm.index.to_numpy()] / 1e6
    born_in = np.isfinite(created_us) & (created_us >= LAKE0 * 1e6)
    E = F[(F.side == 1) & (F.step_tk | F.step_eng) & (F.ssize >= 0.15) & (F.mvk >= 4.4) & (F.vres <= 40.0)
          & (F.wal_n == 0) & ((F.age_old >= 1.0) | (F.age_new >= 1.0)) & (F.t >= LAKE0) & (F.t < FIRE_END)]
    E = E[born_in[E.run.to_numpy()]].copy()
    E["life_ok"] = (life_n[E.run.to_numpy()] >= 60) & (life_age[E.run.to_numpy()] >= 60.0)
    # the same future read on every lake day (a diagnostic: which fires the filter removes, anywhere)
    n_all = np.bincount(F.run.to_numpy(), minlength=nrun)
    last_all = pd.Series(F.tm.to_numpy()).groupby(F.run.to_numpy()).max().reindex(range(nrun)).to_numpy()
    E["life_n_all"] = n_all[E.run.to_numpy()]
    E["life_age_all"] = last_all[E.run.to_numpy()] - created_us[E.run.to_numpy()] / 1e6
    E["n_after"] = E.life_n_all - E.k - 1
    g = E.groupby("run").k.apply(list)
    H = loops(D, start, end, g.index.to_numpy(), g.to_list())
    hh = np.array([H[(r, k)] for r, k in zip(E.run.to_numpy(), E.k.to_numpy())]).reshape(-1, 2)
    E["hold_float"] = hh[:, 0]; E["hold_exact"] = hh[:, 1]
    E["mint"] = mints[E.run.to_numpy()]
    # exits for each coin's first fire under any spelling (the union), every lag
    firsts = set()
    for sp in SPELLS.values():
        m = mask(E, sp)
        firsts |= set(E[m].groupby("run").head(1).index.tolist())
    E["first_any"] = E.index.isin(list(firsts))
    tu_all = D.t_us.to_numpy(np.int64); s_all = D.slot.to_numpy(np.int64)
    v_all = D.vsol.to_numpy(np.float64); sp_all = (D.vsol / D.vtok).to_numpy(np.float64)
    vo_all = np.round(v_all * 1e9) / 1e9
    to_all = F.t.to_numpy()
    sol_all = D.sol.to_numpy(np.float64)
    res = {}
    dead = {}
    rng = np.random.default_rng(20260914 + b)
    for i in np.nonzero(E.first_any.to_numpy())[0]:
        row = E.iloc[i]; r = int(row.run); k = int(row.k)
        a, z = int(start[r]), int(end[r])
        if r not in dead:
            dead[r] = dead_after(tu_all[a:z], sol_all[a:z], v_all[a:z], created_us[r])
        tu = tu_all[a:z]; s = s_all[a:z]; v = v_all[a:z]; sp = sp_all[a:z]
        o = {"dead": int(tu[k] >= dead[r])}
        y, ei, x, why, hold = exit_old(to_all[a:z], vo_all[a:z], k, 0.083)
        o.update(y_old=y, x_old=x, why_old=why)
        for L in LAGS_MS:
            y, ei, x, why, hold = exit_new(tu, s, v, sp, k, L)
            o.update({"y%d" % L: y, "why%d" % L: why, "hold%d" % L: hold, "x%d" % L: x,
                      "grad%d" % L: int(x == z - a - 1 and v[x] >= 114.0)})
        y, *_ = exit_new(tu, s, v, sp, k, 83, clock="deadline")
        o["y83_deadline"] = y
        y, ei, x, why, hold = exit_new(tu, s, v, sp, k, float(rng.choice(lag_draws)))
        o["y_real"] = y
        # the old fill with the engine's cost and spot: isolates the fill / clock change
        y, ei, x, why, hold = exit_old(to_all[a:z], vo_all[a:z], k, 0.083)
        o["y_old_engcost"] = y_engine(sp[ei], v[ei], sp[x], v[x])
        res[E.index[i]] = o
    X = pd.DataFrame.from_dict(res, orient="index")
    E = E.join(X)
    E["bucket"] = b
    E.drop(columns=["mint_i"]).to_parquet(DATA / ("mt_d8_replay_b%d.parquet" % b), index=False)
    print("bucket %d  prints %d  coins %d (born from 09-01 %d)  E prints %d  first-fire rows %d  %.0fs" % (
        b, len(D), nrun, int(born_in.sum()), len(E), int(E.first_any.sum()), time.time() - t0), flush=True)


# ------------------------------------------------------------------ spellings and the book

E_OLD = {"trigger": "pub", "step": "step_tk", "hold": ("hold_float", 46), "nbig": "nbig30_pub",
         "life": True, "age": "age_old"}
SPELLS = {
    "old": E_OLD,
    "trigger any wallet": dict(E_OLD, trigger="all"),
    "missing labels no recipe": dict(E_OLD, step="step_eng"),
    "nbig30 every sell": dict(E_OLD, nbig="nbig30_all"),
    "no future life filter": dict(E_OLD, life=False),
    "holders -> buyers (creator out) <= 46": dict(E_OLD, hold=("buyers", 46)),
    "holders -> exact token book <= 46": dict(E_OLD, hold=("hold_exact", 46)),
    "new: all of the above, buyers": dict(trigger="all", step="step_eng", hold=("buyers", 46),
                                          nbig="nbig30_all", life=False, age="age_new"),
    "new: all of the above, exact holders": dict(trigger="all", step="step_eng", hold=("hold_exact", 46),
                                                 nbig="nbig30_all", life=False, age="age_new"),
}


def mask(E, sp):
    m = E[sp["step"]].to_numpy() & (E[sp["age"]].to_numpy() >= 1.0)
    if sp["trigger"] == "pub":
        m &= ~E.node.to_numpy()
    col, cap = sp["hold"]
    m &= E[col].to_numpy() <= cap
    m &= E[sp["nbig"]].to_numpy() <= 4
    m &= E.np5.to_numpy() <= 25
    if sp["life"]:
        m &= E.life_ok.to_numpy()
    return m


def load_all():
    return pd.concat([pd.read_parquet(DATA / ("mt_d8_replay_b%d.parquet" % b)) for b in range(NB)],
                     ignore_index=True)


def firsts(E, sp, t_lo, t_hi, drop_dead=False):
    """One entry per coin over every day (R = 1), then the window: a coin's first fire decides."""
    m = mask(E, sp)
    if drop_dead:
        m &= E.dead.fillna(1).to_numpy() == 0
    F = E[m].sort_values(["bucket", "run", "k"]).groupby(["bucket", "run"]).head(1)
    return F[(F.t >= t_lo) & (F.t < t_hi)]


def book(F, ycol, days):
    if not len(F):
        return dict(n=0)
    y = F[ycol].to_numpy()
    day = ((F.t.to_numpy() - DAY0) // 86400).astype(int)
    ag = pd.Series(y).groupby(day).sum()
    s = float(y.sum())
    top = float(np.sort(y)[::-1][:max(1, int(round(0.01 * len(y))))].sum())
    per_coin = pd.Series(y).groupby(F.mint.to_numpy()).sum()
    return dict(n=len(F), nday=round(len(F) / days, 1), pct=round(100 * float(y.mean()) / CLIP, 2),
                sol=round(s, 2), pos="%d/%d" % (int((ag > 0).sum()), ag.size), worst=round(float(ag.min()), 2),
                top1=round(100 * top / s, 1) if s > 0 else np.nan,
                maxcoin=round(100 * float(per_coin.max()) / s, 1) if s > 0 else np.nan,
                win=round(100 * float((y > 0).mean()), 1))


def repro():
    """The OLD spelling on the study window against mt_d8_p's gates book, ticket for ticket."""
    sys.path.insert(0, str(_paths.ROOT))
    E = load_all()
    F = firsts(E, E_OLD, LAKE0, STUDY_END)
    ref = pd.read_parquet(DATA / "mt_d8_p_table.parquet")
    ref = ref[(ref.nbig30 <= 4) & (ref.np5 <= 25)].sort_values(["run", "k"]).groupby("run").head(1)
    Pq = pq.read_table(str(DATA / "cvx_studyexact_prints.parquet"), columns=["mint", "slot", "tx_index",
                                                                              "leg_index"],
                       read_dictionary=["mint"]).to_pandas()
    mc = Pq.mint.cat.codes.to_numpy()
    chg = np.r_[0, np.nonzero(np.diff(mc))[0] + 1]           # run r of the study tape starts at chg[r]
    mints = Pq.mint.cat.categories.to_numpy()[mc[chg]]
    gi = chg[ref.run.to_numpy()] + ref.k.to_numpy()
    R = pd.DataFrame({"mint": mints[ref.run.to_numpy()].astype(str), "slot": Pq.slot.to_numpy()[gi],
                      "txi": Pq.tx_index.to_numpy()[gi], "leg": Pq.leg_index.to_numpy()[gi],
                      "y_ref": ref.y.to_numpy(), "why_ref": ref.why.to_numpy()})
    del Pq
    M = F[["mint", "slot", "txi", "leg", "y_old", "why_old"]].merge(R, on=["mint", "slot", "txi", "leg"],
                                                                     how="outer", indicator=True)
    print("study window, OLD spelling: replay %d tickets, study %d" % (len(F), len(R)))
    print(M._merge.value_counts().to_string())
    both = M[M._merge == "both"]
    d = (both.y_old - both.y_ref).abs()
    print("same trigger print: %d   |SOL replay - study| max %.2e   same exit reason %.2f %%" % (
        len(both), d.max(), 100 * (both.why_old == both.why_ref).mean()))
    print("SOL replay %.2f  study %.2f" % (F.y_old.sum(), R.y_ref.sum()))
    M.to_parquet(DATA / "mt_d8_replay_repro.parquet", index=False)


def main_book():
    E = load_all()
    E["t"] = E.t.astype(float)
    wins = [("study 09-01..09-06 12:00 (fitted here)", LAKE0, STUDY_END, 5.5),
            ("holdout 09-06 12:00..09-10 (never read)", STUDY_END, HOLD_END, 4.5),
            ("new days 09-11..09-13 (never read)", HOLD_END, FIRE_END, 3.0),
            ("all 09-01..09-13", LAKE0, FIRE_END, 13.0)]
    print("=== one change at a time: study window, first fire per coin, 83 ms ===")
    rows = []
    for nm, sp in SPELLS.items():
        F = firsts(E, sp, LAKE0, STUDY_END)
        rows.append(dict(spelling=nm, fill="old", **book(F, "y_old", 5.5)))
    print(pd.DataFrame(rows).to_string(index=False))
    sp = SPELLS["new: all of the above, buyers"]
    print("\n=== the engine spelling, fills and cost one at a time (study window, 83 ms) ===")
    F = firsts(E, sp, LAKE0, STUDY_END)
    rows = [dict(step="old fill + clock + curve cost", **book(F, "y_old", 5.5)),
            dict(step="old fill + clock, engine cost on spot", **book(F, "y_old_engcost", 5.5)),
            dict(step="engine LagMs fills, clock at the deadline", **book(F, "y83_deadline", 5.5)),
            dict(step="engine LagMs fills, 200 ms tick clock", **book(F, "y83", 5.5)),
            dict(step="+ drop fires after the dead verdict", **book(F[F.dead == 0], "y83", 5.5))]
    print(pd.DataFrame(rows).to_string(index=False))
    print("\n=== every window: the terms as derived (roster out, float holders; no future filter) and "
          "the engine spellings; engine fills, dead fires out ===")
    derived = dict(E_OLD, life=False)
    rows = []
    for wn, lo, hi, days in wins:
        for spn, sp, seats in (("as derived", derived, (83,)),
                               ("engine, buyers <= 46", SPELLS["new: all of the above, buyers"], LAGS_MS),
                               ("engine, exact holders <= 46", SPELLS["new: all of the above, exact holders"],
                                (83,))):
            F = firsts(E, sp, lo, hi, drop_dead=True)
            for yc, lab in [("y%d" % L, "%d ms" % L) for L in seats] + [("y_real", "real lags")]:
                rows.append(dict(window=wn[:22], terms=spn, seat=lab, **book(F, yc, days)))
    print(pd.DataFrame(rows).to_string(index=False))
    sp = SPELLS["new: all of the above, buyers"]
    F = firsts(E, sp, LAKE0, FIRE_END, drop_dead=True)
    day = pd.to_datetime(F.t, unit="s").dt.date
    print("\n=== engine spelling, per day, 83 ms and real lags ===")
    print(F.assign(day=day).groupby("day").agg(n=("y83", "size"), sol83=("y83", "sum"),
                                               pct83=("y83", lambda s: round(100 * s.mean() / CLIP, 2)),
                                               sol_real=("y_real", "sum")).round(2).to_string())
    print("\nexit reasons at 83 ms: %s" % F.why83.value_counts().to_dict())
    print("graduation exits at 83 ms: %d" % int(F.grad83.sum()))
    rng = np.random.default_rng(7)
    for wn, lo, hi, days in wins:
        G = F[(F.t >= lo) & (F.t < hi)]
        per = G.groupby("mint").y83.sum().to_numpy()
        bs = np.array([per[rng.integers(0, len(per), len(per))].sum() for _ in range(2000)]) / len(G) / CLIP * 100
        print("  %-42s coin bootstrap %%/trade p2.5 / p50 / p97.5: %+.2f / %+.2f / %+.2f" % (
            wn, *np.quantile(bs, [0.025, 0.5, 0.975])))
    F.to_parquet(DATA / "mt_d8_replay_tickets.parquet", index=False)


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "slim":
        slim()
    elif cmd == "cands":
        lags = pd.read_csv(DATA / "mt_d8_fillcheck.csv").lag_ms.dropna().to_numpy()
        for b in ([int(x) for x in sys.argv[2:]] or range(NB)):
            cands(b, lags)
    elif cmd == "repro":
        repro()
    elif cmd == "book":
        main_book()
