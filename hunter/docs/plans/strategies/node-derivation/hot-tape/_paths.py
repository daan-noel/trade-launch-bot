"""Bootstrap for the hot-tape scripts: the toolkit and the shared study kernel on sys.path.

Every script here imports this first. data_file(name) resolves a data file: node-derivation/data/ first,
the shared study-kernel/ second (the study tape and its sidecars live there); a new output goes
to data/.
"""
from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

import toolkit  # noqa: E402,F401  - puts study-kernel on sys.path
from toolkit.paths import DATA, HUNTER, LAKE, LOCAL, SHARED  # noqa: E402,F401
from toolkit.paths import data as data_file  # noqa: E402,F401
