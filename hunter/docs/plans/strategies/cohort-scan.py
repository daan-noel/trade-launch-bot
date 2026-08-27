"""Score a launch cohort for a long edge at the bot's real latency.

Answers one question -- is there any observable state on this cohort from which the
forward price move beats the round-trip toll -- and answers it in the order that makes
a negative answer trustworthy: terrain before gates, gates before conjunctions, and an
oracle exit before any exit search.

Pricing follows `curve-honest-pricing.md`: price ~ vsol^2, the exit is the last print at
or before the exit instant (the fill itself when nobody traded), and silence freezes a
price rather than zeroing it.

    python cohort-scan.py --workdir DIR --from 2026-07-28 --to 2026-08-22 [--stage all]

Each stage caches its parquet in `workdir`, so a re-run only redoes what changed.
"""
from __future__ import annotations

import argparse
import os

import duckdb

LAKE = r"C:\Users\User\Documents\Bot\hunter\lake-data"
SIX_IX = (
    '["Compute Budget: SetComputeUnitLimit","Compute Budget: SetComputeUnitPrice",'
    '"Pump.Fun: Create_v2","Associated Token: CreateIdempotent","Pump.Fun: Buy",'
    '"System Program: Transfer"]'
)
LAG_MS = 115          # measured decide-to-fill; see execution-latency.md
HOLD_S = 30
B = 0.10              # ~1% of a mid-cohort pool; see round8 sizing
FEE_BUY, FEE_SELL = 1.0125, 0.9875

NET = ("pow({x}/{e},2)*(1-" + str(B) + "/{x})*" + str(FEE_SELL)
       + "/((1+" + str(B) + "/{e})*" + str(FEE_BUY) + ")-1")

AGE_BAND = """CASE WHEN age<5 THEN 'a 0-5s' WHEN age<15 THEN 'b 5-15s'
     WHEN age<30 THEN 'c 15-30s' WHEN age<60 THEN 'd 30-60s' WHEN age<120 THEN 'e 60-120s'
     WHEN age<300 THEN 'f 2-5m' WHEN age<900 THEN 'g 5-15m' ELSE 'h 15m+' END"""

FEATURES = ["age", "vsol", "life_ntx", "life_gross", "ntx60", "burst_share",
            "buyshare60", "ret60", "dd", "silence_ms", "buy5", "buy60", "sell60"]


def connect(workdir):
    c = duckdb.connect()
    c.execute("SET temp_directory='" + workdir + "/spill'")
    c.execute("SET memory_limit='6GB'")
    c.execute("CREATE OR REPLACE VIEW trades AS SELECT * FROM read_parquet('"
              + LAKE.replace("\\", "/") + "/trades/*/*.parquet', hive_partitioning=1)")
    c.execute("CREATE OR REPLACE VIEW tokens AS SELECT * FROM read_parquet('"
              + LAKE.replace("\\", "/") + "/tokens/tokens.parquet', hive_partitioning=1)")
    return c


def show(c, sql, title):
    r = c.execute(sql)
    cols = [d[0] for d in r.description]
    rows = r.fetchall()
    print("\n== " + title)
    if not rows:
        print("   (empty)")
        return
    w = [max(len(cols[i]), *(len(str(x[i])) for x in rows)) for i in range(len(cols))]
    print("  ".join(cols[i].ljust(w[i]) for i in range(len(cols))))
    for row in rows:
        print("  ".join(str(row[i]).ljust(w[i]) for i in range(len(cols))))


def export_cohort(c, wd, lo, hi, ix_labels):
    out = wd + "/cohort.parquet"
    if os.path.exists(out):
        return out
    c.execute("""
      COPY (WITH k AS (SELECT mint FROM tokens WHERE fp_ix_labels = '""" + ix_labels + """')
            SELECT t.mint, t.block_time, t.slot, t.tx_index, t.is_buy, t.sol_amount,
                   t.vsol, t.venue, t.wallet, t.dt
            FROM trades t JOIN k USING (mint)
            WHERE t.dt BETWEEN '""" + lo + """' AND '""" + hi + """'
            ORDER BY t.mint, t.slot, t.tx_index)
      TO '""" + out + """' (FORMAT PARQUET, COMPRESSION ZSTD)""")
    return out


def build_features(c, wd, cohort, cutoff, sample=5):
    """One row per sampled print: the state a rule could read, plus what happened next.

    Windows are computed over the FULL tape and sampled afterwards -- sampling first
    would silently shorten every trailing window.
    """
    out = wd + "/fw.parquet"
    if os.path.exists(out):
        c.execute("CREATE OR REPLACE VIEW fw AS SELECT * FROM read_parquet('" + out + "')")
        return out
    c.execute("CREATE OR REPLACE TABLE tr AS "
              "SELECT mint, block_time, vsol FROM read_parquet('" + cohort + "')")
    c.execute("""
      CREATE OR REPLACE TABLE feat AS
      WITH t AS (SELECT mint, block_time, is_buy, sol_amount, vsol,
                        min(block_time) OVER (PARTITION BY mint) AS t0
                 FROM read_parquet('""" + cohort + """')),
      w AS (SELECT mint, block_time, vsol, (block_time-t0)/1e6 AS age,
              count(*) OVER life AS life_ntx, sum(sol_amount) OVER life AS life_gross,
              max(vsol) OVER life AS life_maxvsol,
              count(*) OVER w60 AS ntx60, count(*) OVER w3 AS ntx3,
              sum(CASE WHEN is_buy THEN sol_amount ELSE 0 END) OVER w60 AS buy60,
              sum(CASE WHEN NOT is_buy THEN sol_amount ELSE 0 END) OVER w60 AS sell60,
              sum(CASE WHEN is_buy THEN sol_amount ELSE 0 END) OVER w5 AS buy5,
              first_value(vsol) OVER w60 AS vsol60ago,
              lag(block_time) OVER (PARTITION BY mint ORDER BY block_time) AS prev_bt
            FROM t
            WINDOW life AS (PARTITION BY mint ORDER BY block_time
                            RANGE BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW),
                   w60 AS (PARTITION BY mint ORDER BY block_time
                           RANGE BETWEEN 60000000 PRECEDING AND CURRENT ROW),
                   w5  AS (PARTITION BY mint ORDER BY block_time
                           RANGE BETWEEN 5000000 PRECEDING AND CURRENT ROW),
                   w3  AS (PARTITION BY mint ORDER BY block_time
                           RANGE BETWEEN 3000000 PRECEDING AND CURRENT ROW))
      SELECT mint, block_time, vsol, age, life_ntx, life_gross, buy5, ntx60, ntx3,
             100.0*ntx3/nullif(ntx60,0) AS burst_share, buy60, sell60,
             100.0*buy60/nullif(buy60+sell60,0) AS buyshare60,
             pow(vsol/nullif(vsol60ago,0),2)-1 AS ret60,
             1 - pow(vsol/nullif(life_maxvsol,0),2) AS dd,
             (block_time-prev_bt)/1000.0 AS silence_ms
      FROM w WHERE block_time <= """ + cutoff
              + " AND hash(mint||block_time) % " + str(sample) + " = 0")
    c.execute("""
      COPY (WITH e AS (SELECT f.*, a.vsol AS e_vsol FROM feat f ASOF JOIN tr a
                         ON a.mint=f.mint AND a.block_time <= f.block_time + """
              + str(LAG_MS) + """*1000),
            x AS (SELECT e.*, b.vsol AS v30 FROM e ASOF JOIN tr b
                    ON b.mint=e.mint AND b.block_time <= e.block_time + """
              + str(HOLD_S) + """*1000000)
            SELECT *, """ + NET.format(x="v30", e="e_vsol") + """ AS net30,
                   pow(e_vsol/vsol,2)-1 AS gap
            FROM x) TO '""" + out + """' (FORMAT PARQUET, COMPRESSION ZSTD)""")
    c.execute("CREATE OR REPLACE VIEW fw AS SELECT * FROM read_parquet('" + out + "')")
    return out


def stage_toll(c):
    """The harness checks itself: an unmoved price must cost exactly the toll."""
    show(c, """
      SELECT count(*) n, round(100.0*avg(net30),3) observed,
             round(100.0*avg((1-""" + str(B) + """/e_vsol)*""" + str(FEE_SELL)
             + """/((1+""" + str(B) + """/e_vsol)*""" + str(FEE_BUY) + """)-1),3) analytic,
             round(100.0*min(net30),3) mn, round(100.0*max(net30),3) mx
      FROM fw WHERE v30 = e_vsol""",
         "TOLL CHECK -- observed must equal analytic, else the fill or cost model is wrong")


def stage_terrain(c):
    show(c, """
      SELECT """ + AGE_BAND + """ AS age_band, count(*) n,
        round(100.0*avg(gap),2) fill_gap_pct,
        round(100.0*avg(net30),2) mean_net30, round(100.0*median(net30),2) med_net30,
        round(100.0*avg(CASE WHEN net30>0 THEN 1 ELSE 0 END),1) win_pct
      FROM fw GROUP BY 1 ORDER BY 1""",
         "TERRAIN -- unconditional forward move by age; a cohort negative everywhere has "
         "no long edge to condition on")


def stage_deciles(c):
    rows = []
    for f in FEATURES:
        rows += c.execute("""
          WITH q AS (SELECT """ + f + """ AS v, net30, gap,
                            ntile(10) OVER (ORDER BY """ + f + """) AS b
                     FROM fw WHERE """ + f + """ IS NOT NULL AND isfinite(""" + f + """))
          SELECT '""" + f + """', b, count(*), round(min(v),3), round(max(v),3),
                 round(100*avg(net30),2), round(100*avg(gap),2)
          FROM q GROUP BY b ORDER BY b""").fetchall()
    print("\n== DECILES -- one positive cell is a lead; zero positive cells closes the "
          "single-term search")
    hdr = ("feature", "q", "n", "lo", "hi", "mean_net30", "gap_pct")
    w = [max(len(hdr[i]), *(len(str(r[i])) for r in rows)) for i in range(len(hdr))]
    print("  ".join(hdr[i].ljust(w[i]) for i in range(len(hdr))))
    for r in rows:
        flag = " <<<" if r[5] > 0 else ""
        print("  ".join(str(r[i]).ljust(w[i]) for i in range(len(hdr))) + flag)


def stage_oracle(c, cohort):
    """Sell at the best price in the window. Every real exit is a subset of this."""
    c.execute("""
      CREATE OR REPLACE TABLE trf AS
      SELECT mint, block_time, vsol,
             max(vsol) OVER f30 AS fmax30, max(vsol) OVER f300 AS fmax300
      FROM read_parquet('""" + cohort + """')
      WINDOW f30  AS (PARTITION BY mint ORDER BY block_time
                      RANGE BETWEEN CURRENT ROW AND 30000000 FOLLOWING),
             f300 AS (PARTITION BY mint ORDER BY block_time
                      RANGE BETWEEN CURRENT ROW AND 300000000 FOLLOWING)""")
    show(c, """
      WITH e AS (SELECT f.age, f.e_vsol, f.net30, a.fmax30, a.fmax300
                 FROM fw f ASOF JOIN trf a
                   ON a.mint=f.mint AND a.block_time <= f.block_time + """
             + str(LAG_MS) + """*1000)
      SELECT """ + AGE_BAND + """ AS age_band, count(*) n,
        round(100.0*avg(net30),2) clock30,
        round(100.0*avg(""" + NET.format(x="fmax30", e="e_vsol") + """),2) oracle30,
        round(100.0*avg(""" + NET.format(x="fmax300", e="e_vsol") + """),2) oracle300
      FROM e GROUP BY 1 ORDER BY 1""",
         "ORACLE -- an upper bound on every exit rule; non-positive here closes the "
         "cohort, positive here means the exit search is the one still worth running")


def stage_takeprofit(c, cohort):
    """A take-profit sends a market order like any other, so it pays the same lag."""
    net = NET.format(x="x_vsol", e="e_vsol")
    c.execute("CREATE OR REPLACE TABLE tr AS "
              "SELECT mint, block_time, vsol FROM read_parquet('" + cohort + "')")
    c.execute("""
      CREATE OR REPLACE TABLE ent AS
      WITH s AS (SELECT mint, block_time, age, e_vsol FROM fw
                 WHERE hash(mint||block_time||'s') % 5 = 0)
      SELECT s.mint, s.age, s.e_vsol, a.block_time AS e_bt
      FROM s ASOF JOIN tr a ON a.mint=s.mint AND a.block_time <= s.block_time + """
              + str(LAG_MS) + "*1000")
    show(c, """
      WITH t AS (SELECT e.mint, e.age, e.e_vsol, e.e_bt, v.x, min(a.block_time) AS trig_bt
                 FROM ent e CROSS JOIN (VALUES (0.05),(0.10),(0.20),(0.50)) AS v(x)
                 LEFT JOIN tr a ON a.mint=e.mint AND a.block_time > e.e_bt
                   AND a.block_time <= e.e_bt + """ + str(HOLD_S) + """*1000000
                   AND a.vsol >= e.e_vsol * sqrt(1+v.x)
                 GROUP BY 1,2,3,4,5),
      f AS (SELECT t.*, coalesce(trig_bt, e_bt + """ + str(HOLD_S) + """*1000000) AS x_fire,
                   trig_bt IS NOT NULL AS hit FROM t),
      k AS (SELECT f.*, b.vsol AS x_vsol FROM f ASOF JOIN tr b ON b.mint=f.mint
              AND b.block_time <= f.x_fire
                  + CASE WHEN f.hit THEN """ + str(LAG_MS) + """*1000 ELSE 0 END)
      SELECT x AS tp, count(*) n,
        round(100.0*avg(CASE WHEN hit THEN 1 ELSE 0 END),1) hit_pct,
        round(100.0*avg(""" + net + """) FILTER (hit),2) on_hits,
        round(100.0*avg(""" + net + """) FILTER (NOT hit),2) on_misses,
        round(100.0*avg(""" + net + """),2) overall
      FROM k GROUP BY 1 ORDER BY 1""",
         "TAKE-PROFIT -- what a reactive exit actually harvests; read `on_misses`, which "
         "is what the positions that fail to run cost you")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--workdir", required=True)
    ap.add_argument("--from", dest="lo", required=True)
    ap.add_argument("--to", dest="hi", required=True)
    ap.add_argument("--ix-labels", default=SIX_IX)
    ap.add_argument("--stage", default="all",
                    choices=["all", "toll", "terrain", "deciles", "oracle", "tp"])
    a = ap.parse_args()
    wd = a.workdir.replace("\\", "/")
    os.makedirs(wd + "/spill", exist_ok=True)

    c = connect(wd)
    cohort = export_cohort(c, wd, a.lo, a.hi, a.ix_labels)
    # Entries stop a day short of the tape so every one of them has a full forward
    # window; an entry on the last exported day reads as silent for want of tape.
    cutoff = "epoch_ms(DATE '" + a.hi + "')*1000"
    build_features(c, wd, cohort, cutoff)

    run = a.stage
    if run in ("all", "toll"):
        stage_toll(c)
    if run in ("all", "terrain"):
        stage_terrain(c)
    if run in ("all", "deciles"):
        stage_deciles(c)
    if run in ("all", "oracle"):
        stage_oracle(c, cohort)
    if run in ("all", "tp"):
        stage_takeprofit(c, cohort)


if __name__ == "__main__":
    main()
