"""Vegetation: a few eu_fuel12 classes the model already supports, laid out as
continuous patches driven by elevation, aspect, slope and topographic position.

eu_fuel12 ids (data/fuels_eu12.json): 0 non-vegetated, 1-3 grass, 4-6
broadleaves, 7-9 shrub/maquis, 10-12 conifers; low/medium/high susceptibility.
"""

from __future__ import annotations

import numpy as np

from .grid import CELL_M, CORE_M, CORE_OFF_M, flow_accumulation, fractal, mode_filter, slope_aspect, smooth

# Landscape classes, before the susceptibility split.
OPEN, MACCHIA, BROADLEAF, PINE, SPARSE = range(5)
CLASS_NAMES = {OPEN: "prato/coltivo", MACCHIA: "macchia", BROADLEAF: "bosco di latifoglie",
               PINE: "pineta", SPARSE: "vegetazione rada"}

# Display names for the fuel ids the factory writes.
FUEL_NAMES = {0: "non vegetato (alveo, roccia)", 1: "erba rada", 2: "prato/coltivo", 3: "prato secco",
              4: "latifoglie umide", 5: "latifoglie", 7: "macchia rada", 8: "macchia", 9: "macchia densa",
              11: "pineta", 12: "pineta densa"}

# Thresholds, saved with each candidate.
VEG = {"channel_cells": 350, "river_cells": 2500, "open_max_slope_deg": 10.0, "rock_slope_deg": 34.0}


def generate(dem: np.ndarray, seed: int, veg: dict | None = None) -> tuple[np.ndarray, dict]:
    v = {**VEG, **(veg or {})}
    rng = np.random.default_rng(seed + 1000)
    slope, south = slope_aspect(dem)
    # Relative height against the CORE's range, so the buffer's hills do not
    # shift the vegetation belts inside the designed landscape.
    a, b = int(CORE_OFF_M / CELL_M), int((CORE_OFF_M + CORE_M) / CELL_M)
    core = dem[a:b, a:b]
    rel = np.clip((dem - core.min()) / np.ptp(core), 0.0, 1.2)
    tpi = dem - smooth(dem, 12.0)  # >0 ridges and spurs, <0 hollows (240 m scale)
    acc = flow_accumulation(dem)
    patch = smooth(fractal(rng, 3.5), 2.5)  # patch-scale noise, 300-600 m blobs
    patch2 = smooth(fractal(rng, 3.5), 2.5)
    grain = smooth(fractal(rng, 3.0), 1.0)
    sun = south * np.clip(slope / 12.0, 0.0, 1.0)  # aspect only matters on a slope
    flat = np.exp(-((slope / v["open_max_slope_deg"]) ** 2))

    score = np.zeros((5,) + dem.shape)
    # Fields and pasture: valley floors and gentle shoulders, some high pasture.
    score[OPEN] = 3.2 * flat * smooth((rel < 0.4).astype(float), 2.0) + 1.4 * flat * (rel > 0.75) + 0.45 * patch - 0.3
    # Macchia: sunny, dry, low-to-mid slopes; the Mediterranean default.
    score[MACCHIA] = 0.9 + 0.9 * sun - 0.9 * np.clip(rel - 0.7, 0, 1) + 0.45 * patch2
    # Broadleaf (oak, chestnut): shaded slopes and hollows, mid elevation.
    score[BROADLEAF] = 0.7 - 1.0 * sun + 1.2 * np.clip(-tpi / 25.0, 0, 1) * np.clip((slope - 5.0) / 10.0, 0, 1) - 0.5 * patch2 \
        - 0.6 * np.clip(rel - 0.85, 0, 1) * 5
    # Pine: upper slopes and spurs, on either aspect.
    score[PINE] = 0.4 + 1.3 * np.exp(-((rel - 0.62) / 0.2) ** 2) + 0.8 * np.clip(tpi / 25.0, 0, 1) \
        - 0.5 * patch
    # Sparse: steep rocky ground and exposed crests.
    score[SPARSE] = 3.0 * np.clip((slope - v["rock_slope_deg"]) / 6.0, 0, 1) \
        + 1.0 * np.clip(tpi / 30.0, 0, 1) * (rel > 0.8) - 0.3
    cls = mode_filter(score.argmax(axis=0), 5, passes=3)

    # Susceptibility within each class, from a finer grain.
    fuel = np.zeros(dem.shape, dtype=np.int32)
    fuel[cls == OPEN] = np.where((sun > 0.3) & (rel > 0.3), 3, 2)[cls == OPEN]
    fuel[cls == MACCHIA] = np.where((sun > 0.45) & (grain > 0.0), 9, np.where(grain < -0.9, 7, 8))[cls == MACCHIA]
    fuel[cls == BROADLEAF] = np.where(tpi < -12, 4, 5)[cls == BROADLEAF]
    fuel[cls == PINE] = np.where(grain > 0.6, 12, 11)[cls == PINE]
    fuel[cls == SPARSE] = np.where(slope > v["rock_slope_deg"] + 12, 0, np.where(grain > 0, 7, 1))[cls == SPARSE]

    # Drainage: riparian strips along the channels, a bare gravel bed on the river.
    channel = acc >= v["channel_cells"]
    fuel[channel & (slope < 25)] = 4
    fuel[acc >= v["river_cells"]] = 0

    stats = {"fuel_share": {int(k): round(float((fuel == k).mean()), 4) for k in np.unique(fuel)},
             "class_share": {CLASS_NAMES[k]: round(float((cls == k).mean()), 4) for k in range(5)},
             "veg": v}
    return fuel, stats
