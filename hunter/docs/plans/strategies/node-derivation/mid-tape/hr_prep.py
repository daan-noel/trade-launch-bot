"""Hit-rate method test, step 0: a compact curve tape for 09-01 .. 09-06 from the lake.

One row per curve leg in the canonical order (mint, slot, tx_index, leg_index). Carries only what
the method test reads: coin, slot, time, side, SOL, vsol after and before the leg, whether the
wallet is 8dtx2t, whether this is the wallet's first print on the coin, and the token amount
(8dtx2t's positions). DuckDB does the sort with a memory cap; the host has little free RAM.

  python hr_prep.py      -> data/hr_tape.parquet
"""
import time

import duckdb

import _paths  # noqa: F401
from _paths import DATA, LAKE

DAYS = ["2026-09-0%d" % d for d in range(1, 7)]
ACTOR = "8dtx2tr4TuJsYpri2suggFu1pg3DVjFLBBVmhtDy1MEF"
DAY0_US = 1788220800 * 1_000_000   # 2026-09-01 00:00 UTC

t0 = time.time()
DATA.mkdir(exist_ok=True)
con = duckdb.connect()
con.execute("SET memory_limit='1500MB'; SET threads=4; SET preserve_insertion_order=true")
con.execute("SET temp_directory='%s'" % (DATA / "hr_tmp").as_posix())
files = ", ".join("'%s'" % (LAKE / "trades" / f"dt={d}" / "data.parquet").as_posix() for d in DAYS)
con.execute(f"""
COPY (
  WITH b AS (
    SELECT mint, slot, tx_index, leg_index, block_time, is_buy, sol_amount, token_amount, vsol,
           wallet
    FROM read_parquet([{files}])
    WHERE venue = 'curve' AND vsol IS NOT NULL AND vsol > 0
  )
  SELECT dense_rank() OVER (ORDER BY mint)::INTEGER AS coin,
         slot, tx_index, leg_index,
         ((block_time - {DAY0_US}) / 1e6)::DOUBLE AS t,
         is_buy::TINYINT AS buy,
         sol_amount::FLOAT AS sol,
         vsol::FLOAT AS v,
         (wallet = '{ACTOR}')::TINYINT AS actor,
         (row_number() OVER (PARTITION BY mint, wallet ORDER BY slot, tx_index, leg_index) = 1)::TINYINT
             AS first_here,
         CASE WHEN wallet = '{ACTOR}' THEN token_amount ELSE 0 END AS tok,
         hash(wallet) AS wh
  FROM b
  ORDER BY mint, slot, tx_index, leg_index
) TO '{(DATA / "hr_tape.parquet").as_posix()}' (FORMAT parquet, COMPRESSION zstd)
""")
n = con.execute(f"SELECT count(*), sum(actor) FROM '{(DATA / 'hr_tape.parquet').as_posix()}'").fetchone()
print("rows %s, 8dtx2t legs %s, %ds" % (f"{n[0]:,}", f"{n[1]:,}", time.time() - t0))
