"""Convexity census and the demonstrated-episode door.

Universe: last-leg week tape (aa.pxf). An up-episode is a zigzag swing low to swing high,
confirmed by a 15% price retracement. Price is exact on the curve: p_i/p_j = (v_i/v_j)^2.

Door: this mint has already COMPLETED one episode of >= 50%, counted only from the print
where the retracement confirms the peak (no lookahead). Create-time 3ix / dead / dump-factory
are a second door, reported beside the behavioural one.

Event: the zigzag UP-TURN after the door — the first print that stands 15% above the new
swing low. That is the completing print of "the next up-move has started".

Fill: last print landed by decision + 115 ms, both legs, kernel.py costs. Re-entry unlimited.
"""
from __future__ import annotations

import time
from datetime import datetime, timezone
from pathlib import Path

import numpy as np
import pandas as pd

from kernel import B_DEFAULT, book, daily, fill_idx, net
from tape import Tape

HERE = Path(__file__).resolve().parent
pd.set_option("display.width", 280)
pd.set_option("display.max_rows", 400)

R_ZZ = 0.15
D_DOOR = 0.50
B = B_DEFAULT  # 0.2
DAY0 = datetime(2026, 8, 30, tzinfo=timezone.utc).timestamp()

EXITS = {
    "clock 15": dict(cap=15),
    "clock 30": dict(cap=30),
    "clock 60": dict(cap=60),
    "clock 120": dict(cap=120),
    "trail15 c120": dict(cap=120, trail=15),
    "trail30 c300": dict(cap=300, trail=30),
    "arm10 t20 c300": dict(cap=300, trail=20, arm=10),
    "trail40 c600": dict(cap=600, trail=40),
}


def swings(v, R):
    """(lo_i, hi_i, turn_i) up-swings. turn_i is the print that crossed +R from the low."""
    lo_i = hi_i = turn_i = 0
    lo = hi = v[0]
    up = True
    out = []
    for i in range(1, len(v)):
        x = v[i]
        if up:
            if x > hi:
                hi = x
                hi_i = i
            elif (x / hi) ** 2 <= 1.0 - R:
                if hi_i > lo_i:
                    out.append((lo_i, hi_i, turn_i))
                up = False
                lo = x
                lo_i = i
        else:
            if x < lo:
                lo = x
                lo_i = i
            elif (x / lo) ** 2 - 1.0 >= R:
                up = True
                hi = x
                hi_i = i
                turn_i = i
    if up and hi_i > lo_i:
        out.append((lo_i, hi_i, turn_i))
    return out


def q(s, qs=(0.25, 0.5, 0.75)):
    s = np.asarray(s, dtype=float)
    s = s[np.isfinite(s)]
    if s.size == 0:
        return "nan"
    return "  ".join("%.3g" % x for x in np.quantile(s, qs))


def cell(d, b=B, days=None):
    if d is None or len(d) == 0:
        return dict(n=0, nday=0, mints=0, sol=0.0, pct=0.0, pos="0/0", worst=0.0, win=0.0)
    ag = d.groupby("day").y.sum()
    n = len(d)
    nday = round(n / days, 1) if days else n
    return dict(
        n=n,
        nday=nday,
        mints=int(d.run.nunique()),
        sol=round(float(d.y.sum()), 2),
        pct=round(float(d.y.mean()) / b * 100, 2),
        pos="%d/%d" % (int((ag > 0).sum()), ag.size),
        worst=round(float(ag.min()), 2),
        win=round(float((d.y > 0).mean()) * 100, 1),
    )


def show(rows, title):
    print("\n=== %s" % title)
    df = pd.DataFrame(rows)
    print(df.to_string(index=False), flush=True)
    return df


def main() -> None:
    t0 = time.time()
    T = Tape(
        str(HERE / "cvx_prints.parquet"),
        cols=[
            "mint",
            "slot",
            "t_ms",
            "reserve_lamports",
            "amount_lamports",
            "side",
            "wallet_id",
            "build",
        ],
    )
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    days = (T.t.max() - T.t.min()) / 86400.0
    print(
        "tape prints %s  tokens %s  days %.2f  %ds"
        % (f"{T.n:,}", f"{len(T.mints):,}", days, time.time() - t0),
        flush=True,
    )

    tok = pd.read_parquet(HERE / "cvx_tok.parquet")
    tok["excl"] = tok["excl"].fillna("keep")
    tok = tok.drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (
        pd.to_datetime(tok.created_at, utc=True, format="ISO8601") - pd.Timestamp(0, tz="UTC")
    ).dt.total_seconds()
    aligned = tok.reindex(T.mints)
    c_s = aligned.c_s.to_numpy()
    excl = aligned.excl.fillna("unknown").to_numpy()
    his = aligned.his.fillna(False).to_numpy().astype(bool)
    dump1 = (
        aligned.peak_in_first_run.fillna(False).to_numpy().astype(bool)
        & (aligned.end_off.fillna(1.0).to_numpy() < 0.5)
    )
    creator = aligned.creation_slot.to_numpy()  # placeholder length
    # creator wallet is on the print table
    import pyarrow.parquet as pq

    creator_id = (
        pq.read_table(str(HERE / "cvx_prints.parquet"), columns=["creator_id"])
        .column(0)
        .to_pandas()
        .fillna(-1)
        .to_numpy()
        .astype(np.int64)
    )

    mach = pd.read_parquet(HERE / "cvx_mach.parquet")
    role_of = mach.drop_duplicates("build").set_index("build")["role"].to_dict()
    bclass = np.array([role_of.get(b, "other") for b in T.builds])

    print("tok aligned  excl", pd.Series(excl).value_counts().to_dict(), flush=True)

    ep_rows = []
    tok_rows = []
    fire_rows = []
    n_scanned = 0

    for r in range(len(T.start)):
        a, b = T.start[r], T.end[r]
        n = b - a
        if n < 4:
            continue
        v = T.v[a:b]
        t = T.t[a:b]
        sl = T.slot[a:b]
        side = T.side[a:b]
        sol = T.sol[a:b]
        wal = T.wallet[a:b]
        bld = T.build[a:b]
        if not np.isfinite(v).all() or (v <= 0).any():
            continue
        n_scanned += 1
        kind = excl[r]
        keep = kind == "keep"
        is_his = bool(his[r])
        is_dump1 = bool(dump1[r])
        cr = int(creator_id[a])
        age0 = t - c_s[r] if np.isfinite(c_s[r]) else np.full(n, np.nan)
        cr_sold = np.zeros(n, bool)
        if cr >= 0:
            cr_sold = np.concatenate(([False], np.cumsum((wal == cr) & (side == -1))[:-1] > 0))

        eps = swings(v, R_ZZ)
        nep = {g: 0 for g in (0.10, 0.20, 0.50, 1.00)}
        confirmed = []  # (hi_i, rise, builds_in_ep, hi_v, hi_t)
        n_ep_at = np.zeros(n, np.int16)
        best_at = np.zeros(n)
        pk_v = np.full(n, np.nan)
        pk_t = np.full(n, np.nan)
        cnt = 0
        bst = 0.0
        last_pk = np.nan
        last_pt = np.nan
        last_builds = set()

        # walk once for confirmed-before-i state AND episode list
        lo_i = hi_i = turn_i = 0
        lo = hi = v[0]
        up = True
        pending_builds = set()
        for i in range(n):
            n_ep_at[i] = cnt
            best_at[i] = bst
            pk_v[i] = last_pk
            pk_t[i] = last_pt
            x = v[i]
            if up:
                pending_builds.add(int(bld[i]))
                if x > hi:
                    hi = x
                    hi_i = i
                elif (x / hi) ** 2 <= 1.0 - R_ZZ:
                    if hi_i > lo_i:
                        rise = (hi / lo) ** 2 - 1.0
                        if rise >= 0.05:
                            for g in nep:
                                if rise >= g:
                                    nep[g] += 1
                            ei = fill_idx(t, turn_i, 0.115)
                            if ei > hi_i:
                                ei = hi_i
                            left = (v[hi_i] / v[ei]) ** 2 - 1.0
                            lagc = (v[ei] / v[turn_i]) ** 2 - 1.0
                            peak_y = net(v[ei], v[hi_i], B)
                            nxt_lo = None
                            # death depth: continue to find next low later; fill after pass
                            ep_rows.append(
                                dict(
                                    run=r,
                                    day=int(T.day[a + lo_i]),
                                    k=r,
                                    lo_i=lo_i,
                                    hi_i=hi_i,
                                    turn_i=turn_i,
                                    rise=rise,
                                    v_lo=float(v[lo_i]),
                                    v_hi=float(v[hi_i]),
                                    v_turn=float(v[turn_i]),
                                    dur=float(t[hi_i] - t[lo_i]),
                                    prints=hi_i - lo_i,
                                    age_lo=float(age0[lo_i]) if np.isfinite(age0[lo_i]) else np.nan,
                                    buy_sol=float(sol[lo_i : hi_i + 1][side[lo_i : hi_i + 1] == 1].sum()),
                                    left=left,
                                    secs=float(t[hi_i] - t[ei]),
                                    lagc=lagc,
                                    peak_y=peak_y,
                                    door_before=int(cnt),
                                    keep=keep,
                                    kind=kind,
                                    his=is_his,
                                    dump1=is_dump1,
                                    ep_i=cnt,
                                )
                            )
                        if rise >= D_DOOR:
                            cnt += 1
                            bst = max(bst, rise)
                            last_pk = hi
                            last_pt = t[hi_i]
                            last_builds = set(pending_builds)
                            confirmed.append((hi_i, rise, last_builds, hi, t[hi_i], lo_i, turn_i))
                    up = False
                    lo = x
                    lo_i = i
                    pending_builds = set()
            else:
                if x < lo:
                    lo = x
                    lo_i = i
                elif (x / lo) ** 2 - 1.0 >= R_ZZ:
                    up = True
                    hi = x
                    hi_i = i
                    turn_i = i
                    pending_builds = {int(bld[i])}
                    if cnt >= 1 and side[i] == 1:
                        off_pk = (x / last_pk) ** 2 - 1.0 if np.isfinite(last_pk) else np.nan
                        pb = (lo / last_pk) ** 2 - 1.0 if np.isfinite(last_pk) else np.nan
                        since = t[i] - last_pt if np.isfinite(last_pt) else np.nan
                        role = bclass[int(bld[i])]
                        returning = int(bld[i]) in last_builds
                        age = float(age0[i]) if np.isfinite(age0[i]) else -1.0
                        # leftover of THIS swing is filled after the token pass (we don't know hi yet)
                        fire_rows.append(
                            dict(
                                run=r,
                                day=int(T.day[a + i]),
                                k=i,
                                n_ep=cnt,
                                best=bst,
                                v=float(x),
                                sol=float(sol[i]),
                                age=age,
                                since=float(since) if np.isfinite(since) else -1.0,
                                off_pk=float(off_pk) if np.isfinite(off_pk) else np.nan,
                                pb=float(pb) if np.isfinite(pb) else np.nan,
                                role=role,
                                returning=returning,
                                cr_sold=bool(cr_sold[i]),
                                keep=keep,
                                kind=kind,
                                his=is_his,
                                dump1=is_dump1,
                                turn_i=i,
                                lo_i=lo_i,
                            )
                        )

        if up and hi_i > lo_i:
            rise = (hi / lo) ** 2 - 1.0
            if rise >= 0.05:
                for g in nep:
                    if rise >= g:
                        nep[g] += 1
                ei = fill_idx(t, turn_i, 0.115)
                if ei > hi_i:
                    ei = hi_i
                ep_rows.append(
                    dict(
                        run=r,
                        day=int(T.day[a + lo_i]),
                        lo_i=lo_i,
                        hi_i=hi_i,
                        turn_i=turn_i,
                        rise=rise,
                        v_lo=float(v[lo_i]),
                        v_hi=float(v[hi_i]),
                        v_turn=float(v[turn_i]),
                        dur=float(t[hi_i] - t[lo_i]),
                        prints=hi_i - lo_i,
                        age_lo=float(age0[lo_i]) if np.isfinite(age0[lo_i]) else np.nan,
                        buy_sol=float(sol[lo_i : hi_i + 1][side[lo_i : hi_i + 1] == 1].sum()),
                        left=(v[hi_i] / v[ei]) ** 2 - 1.0,
                        secs=float(t[hi_i] - t[ei]),
                        lagc=(v[ei] / v[turn_i]) ** 2 - 1.0,
                        peak_y=net(v[ei], v[hi_i], B),
                        door_before=int(cnt),
                        keep=keep,
                        kind=kind,
                        his=is_his,
                        dump1=is_dump1,
                        ep_i=cnt,
                    )
                )

        tok_rows.append(
            dict(
                run=r,
                prints=n,
                day=int(T.day[a]),
                keep=keep,
                kind=kind,
                his=is_his,
                dump1=is_dump1,
                **{"ep%d" % int(g * 100): c for g, c in nep.items()},
            )
        )
        if r % 20000 == 0:
            print(
                "  run %d/%d  ep %d  fires %d  %ds"
                % (r, len(T.start), len(ep_rows), len(fire_rows), time.time() - t0),
                flush=True,
            )

    E = pd.DataFrame(ep_rows)
    K = pd.DataFrame(tok_rows)
    Fraw = pd.DataFrame(fire_rows)
    print(
        "\n=== FUNNEL: tokens scanned %d / %d  episodes(rise>=5%%) %d on %d tokens  turn-fires %d  %ds"
        % (n_scanned, len(T.mints), len(E), E.run.nunique() if len(E) else 0, len(Fraw), time.time() - t0),
        flush=True,
    )

    # attach this-swing rise onto fires by (run, turn_i)
    if len(E) and len(Fraw):
        emap = E.set_index(["run", "turn_i"])["rise"]
        Fraw["rise"] = [emap.get((int(rr), int(ti)), np.nan) for rr, ti in zip(Fraw.run, Fraw.turn_i)]
    else:
        Fraw["rise"] = np.nan

    print("\n=== 1. how many episodes exist, by size (full tape)")
    out = []
    for g in (0.10, 0.20, 0.50, 1.00, 2.00):
        d = E[E.rise >= g]
        out.append(
            dict(
                size=">= %d%%" % int(g * 100),
                n=len(d),
                per_day=round(len(d) / days, 0),
                tokens=d.run.nunique(),
                dur_p50=round(float(d.dur.median()), 1) if len(d) else None,
                dur_p90=round(float(d.dur.quantile(0.9)), 1) if len(d) else None,
                v_lo_p50=round(float(d.v_lo.median()), 1) if len(d) else None,
            )
        )
    show(out, "episode counts")

    print("\n=== 1b. same, create-keep only (3ix/dead/dump-factory out)")
    out = []
    Ek = E[E.keep]
    for g in (0.10, 0.20, 0.50, 1.00):
        d = Ek[Ek.rise >= g]
        out.append(
            dict(
                size=">= %d%%" % int(g * 100),
                n=len(d),
                per_day=round(len(d) / days, 0),
                tokens=d.run.nunique(),
                dur_p50=round(float(d.dur.median()), 1) if len(d) else None,
            )
        )
    show(out, "keep-only episode counts")

    print("\n=== 1c. 3ix / dump-factory / dead vs keep: do they repeat?")
    KK = K[K.prints >= 20]
    out = []
    for kind, sub in KK.groupby("kind"):
        v = sub["ep50"]
        out.append(
            dict(
                kind=kind,
                tokens=len(sub),
                pct0=round(100 * (v == 0).mean(), 1),
                pct1=round(100 * (v == 1).mean(), 1),
                pct2=round(100 * (v == 2).mean(), 1),
                pct3p=round(100 * (v >= 3).mean(), 1),
                mean=round(float(v.mean()), 2),
                p2_given1=round(100 * (v >= 2).sum() / max((v >= 1).sum(), 1), 1),
            )
        )
    show(out, "repeat by create kind, tokens with >=20 prints")

    print("\n=== 1d. dump1 shape (lookahead, description only) vs keep")
    out = []
    for name, mask in (("dump1 path", KK.dump1), ("not dump1", ~KK.dump1)):
        v = KK.loc[mask, "ep50"]
        out.append(
            dict(
                shape=name,
                tokens=int(mask.sum()),
                pct0=round(100 * (v == 0).mean(), 1),
                pct1=round(100 * (v == 1).mean(), 1),
                pct2p=round(100 * (v >= 2).mean(), 1),
                p2_given1=round(100 * (v >= 2).sum() / max((v >= 1).sum(), 1), 1),
            )
        )
    show(out, "dump1 vs not")

    print("\n=== 2. REACHABILITY. Fire at the zigzag turn (seen +15%% from the low), fill 115 ms later.")
    print("    left = rise still ahead at fill.  peak_y = booked SOL if we sold at the peak (perfect exit).")
    out = []
    for g in (0.20, 0.50, 1.00):
        for door in (0, 1):
            d = E[(E.rise >= g) & (E.door_before >= door) & (E.turn_i > 0)]
            if door == 1:
                d = d[d.door_before >= 1]
            lab = "ep2+" if door else "all"
            if not len(d):
                continue
            out.append(
                dict(
                    size=">=%d%%" % int(g * 100),
                    which=lab,
                    n=len(d),
                    keep=int(d.keep.sum()),
                    left_p25=round(float(d.left.quantile(0.25)), 3),
                    left_p50=round(float(d.left.median()), 3),
                    left_p75=round(float(d.left.quantile(0.75)), 3),
                    secs_p50=round(float(d.secs.median()), 2),
                    lag_p50=round(100 * float(d.lagc.median()), 2),
                    peak_sol=round(float(d.peak_y.sum()), 2),
                    peak_pct=round(100 * float(d.peak_y.mean()) / B, 2),
                    peak_pos="%d/%d"
                    % (
                        int((d.groupby("day").peak_y.sum() > 0).sum()),
                        d.day.nunique(),
                    ),
                )
            )
    show(out, "leftover at the turn, 115 ms")

    # book honest fires
    print("\n=== 3. booking turn-fires at 115 ms (one position at a time)", flush=True)
    booked = []
    # group fires by run to walk tape once more? We stored k as local index.
    # Re-open each run — 127k is ok if we index fires.
    by_run = {}
    for rec in Fraw.itertuples(index=False):
        by_run.setdefault(int(rec.run), []).append(rec)

    n_book = 0
    for r, recs in by_run.items():
        a, b = T.start[r], T.end[r]
        v = T.v[a:b]
        t = T.t[a:b]
        sl = T.slot[a:b]
        recs = sorted(recs, key=lambda x: x.k)
        for xn, spec in EXITS.items():
            last = -1
            for rec in recs:
                k = int(rec.k)
                if k <= last or k >= len(t) - 1:
                    continue
                y, ei, xi, reason = book(t, v, sl, k, spec, lag=0.115, B=B)
                last = xi
                booked.append(
                    dict(
                        event="TURN",
                        exit=xn,
                        day=int(rec.day),
                        run=int(rec.run),
                        y=y,
                        reason=reason,
                        n_ep=int(rec.n_ep),
                        v=float(rec.v),
                        age=float(rec.age),
                        since=float(rec.since),
                        pb=float(rec.pb) if rec.pb == rec.pb else np.nan,
                        role=rec.role,
                        returning=bool(rec.returning),
                        cr_sold=bool(rec.cr_sold),
                        keep=bool(rec.keep),
                        kind=rec.kind,
                        his=bool(rec.his),
                        dump1=bool(rec.dump1),
                        rise=float(rec.rise) if rec.rise == rec.rise else np.nan,
                    )
                )
                n_book += 1
        if n_book and n_book % 200000 < len(EXITS):
            print("  booked %d  %ds" % (n_book, time.time() - t0), flush=True)

    R = pd.DataFrame(booked)
    R.to_parquet(HERE / "cvx_fires.parquet")
    E.to_parquet(HERE / "cvx_episodes.parquet")
    K.to_parquet(HERE / "cvx_tokens.parquet")
    print("booked %d rows  %ds" % (len(R), time.time() - t0), flush=True)

    def pack(d, name):
        c = cell(d, days=days)
        c["name"] = name
        return c

    rows = []
    for xn in EXITS:
        d = R[R.exit == xn]
        rows.append(pack(d, "all / " + xn))
        rows.append(pack(d[d.keep], "keep / " + xn))
        ora = d[d.rise >= 0.50]
        rows.append(pack(ora, "oracle ep>=50% / " + xn))
        rows.append(pack(ora[ora.keep], "oracle+keep / " + xn))
    show(rows, "TURN x exit. oracle = this swing really became >=50% (lookahead ceiling)")

    # pick the unbiased clock and the best honest keep trail for gates
    print("\n=== 4. gates, one at a time, on keep door, clock 30 and trail30 c300")
    parent_exits = ["clock 30", "trail30 c300", "arm10 t20 c300"]
    gate_rows = []
    for xn in parent_exits:
        P = R[(R.exit == xn) & (R.keep)]
        gate_rows.append(pack(P, xn + " | parent keep"))
        for name, m in (
            ("creator in", ~P.cr_sold),
            ("creator sold", P.cr_sold),
            ("v 35-55", (P.v >= 35) & (P.v < 55)),
            ("v 55-80", (P.v >= 55) & (P.v < 80)),
            ("v 80+", P.v >= 80),
            ("age 10-120s", (P.age >= 10) & (P.age < 120)),
            ("age 120-900s", (P.age >= 120) & (P.age < 900)),
            ("age 900s+", P.age >= 900),
            ("since peak 0-15s", (P.since >= 0) & (P.since < 15)),
            ("since peak 15-60s", (P.since >= 15) & (P.since < 60)),
            ("since peak 60s+", P.since >= 60),
            ("pullback 15-40%", (P.pb <= -0.15) & (P.pb > -0.40)),
            ("pullback 40-70%", (P.pb <= -0.40) & (P.pb > -0.70)),
            ("pullback 70%+", P.pb <= -0.70),
            ("returning machine", P.returning),
            ("new machine", ~P.returning),
            ("n_ep=1", P.n_ep == 1),
            ("n_ep>=2", P.n_ep >= 2),
            ("not dump1 path", ~P.dump1),
        ):
            gate_rows.append(pack(P[m], xn + " | " + name))
        for role, g in P.groupby("role"):
            if len(g) >= 30:
                gate_rows.append(pack(g, xn + " | role " + str(role)))
    show(gate_rows, "gates on keep TURN")

    print("\n=== 5. P(this turn becomes >=50%) — the detection rate, decision-time slices")
    H = Fraw[Fraw.keep]
    out = []

    def rate(name, d):
        if not len(d):
            return
        out.append(
            dict(
                slice=name,
                n=len(d),
                p20=round(100 * (d.rise >= 0.20).mean(), 1),
                p50=round(100 * (d.rise >= 0.50).mean(), 1),
                p100=round(100 * (d.rise >= 1.00).mean(), 1),
            )
        )

    rate("keep turns", H)
    rate("creator in", H[~H.cr_sold])
    rate("returning", H[H.returning])
    rate("new machine", H[~H.returning])
    rate("n_ep=1", H[H.n_ep == 1])
    rate("n_ep>=2", H[H.n_ep >= 2])
    rate("pb 15-40%", H[(H.pb <= -0.15) & (H.pb > -0.40)])
    rate("pb 40-70%", H[(H.pb <= -0.40) & (H.pb > -0.70)])
    rate("v 35-55", H[(H.v >= 35) & (H.v < 55)])
    rate("v 55-80", H[(H.v >= 55) & (H.v < 80)])
    for role, g in H.groupby("role"):
        if len(g) >= 50:
            rate("role " + str(role), g)
    show(out, "detection rate")

    print("\nCVX OK %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
