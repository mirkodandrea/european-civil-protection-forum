"""Orography: a few hand-shaped archetypes, detailed with noise.

Each archetype is the shape a reviewer can name (an asymmetric valley, a valley
with a tributary, a basin around a hill, a ridge with a saddle); the seed only
adds detail. No houses or roads exist at this stage (02-SCENARIO-FACTORY §A).
"""

from __future__ import annotations

from dataclasses import dataclass, field

import numpy as np

from .grid import CORE_M, CORE_OFF_M, WORLD_M, erode, fractal, polyline_distance, smooth, smoothstep, xy


@dataclass
class Candidate:
    id: str
    name: str
    archetype: str
    seed: int
    notes: str = ""
    detail: dict = field(default_factory=dict)  # overrides of DETAIL for this candidate
    params: dict = field(default_factory=dict)


CANDIDATES = [
    Candidate("t1", "Valle asimmetrica", "valle", 11,
              "valle O-E; versante a solatio lungo e dolce, a bacio corto e ripido; un vallone laterale"),
    Candidate("t2", "Valle con affluente", "affluente", 22,
              "valle SO-NE con un affluente da NO; piana alla confluenza"),
    Candidate("t3", "Conca con colle", "conca", 33,
              "conca chiusa da crinali, sbocco a E, colle isolato con sommita piana"),
    Candidate("t4", "Crinale e sella", "crinale", 44,
              "crinale NO-SE con una sella; versanti di lunghezza diversa",
              # chosen at checkpoint 1 (2026-10-08): lighter erosion, no comb-like rills
              detail={"erosion_iterations": 12, "erosion_k": 0.4, "erosion_diffusion": 2.0}),
]


def by_id(cid: str) -> Candidate:
    for c in CANDIDATES:
        if c.id == cid:
            return c
    raise SystemExit(f"candidato sconosciuto: {cid} (disponibili: {', '.join(c.id for c in CANDIDATES)})")


def _valley(X, Y, axis, floor0, grad_per_km, w0, left, right, beyond_drop=60.0):
    """A valley along `axis`: flat floor of half-width `w0`, then each side
    rises by H over L metres (left = left of travel) and eases off past the
    crest. Returns the elevation and the distance to the thalweg."""
    d, side, along = polyline_distance(X, Y, axis)
    dd = np.maximum(d - w0, 0.0)
    H = np.where(side > 0, left[0], right[0])
    L = np.where(side > 0, left[1], right[1])
    rise = H * smoothstep(0.0, 1.0, dd / L) - beyond_drop * smoothstep(1.0, 1.6, dd / L)
    return floor0 + grad_per_km * along / 1000.0 + rise, d


WORLD_C = WORLD_M / 2


def _ext(line, m=6000.0):
    """Carry a main axis on past both ends, so it runs through the buffer ring
    instead of stopping at the core's edge."""
    (ax, ay), (bx, by) = line[0], line[1]
    (cx, cy), (dx, dy) = line[-2], line[-1]
    u = np.hypot(bx - ax, by - ay)
    v = np.hypot(dx - cx, dy - cy)
    return [(ax - (bx - ax) / u * m, ay - (by - ay) / u * m), *line,
            (dx + (dx - cx) / v * m, dy + (dy - cy) / v * m)]


def _carve(dem, X, Y, line, frac, width, floor):
    """A side valley: cut a share of the height above the main floor, so its bed
    descends toward the confluence like a real tributary."""
    d, _, _ = polyline_distance(X, Y, line)
    w = np.exp(-(d / width) ** 2)
    return dem - frac * np.maximum(dem - floor, 0.0) * w


def _flatten(dem, X, Y, cx, cy, r):
    """A shoulder or terrace: pull a soft disc toward its mean height."""
    m = np.exp(-((np.hypot(X - cx, Y - cy) / r) ** 4))
    target = float((dem * m).sum() / m.sum())
    return dem * (1 - 0.85 * m) + target * 0.85 * m


# Detail knobs, shared by every candidate and saved with it.
DETAIL = {"warp_m": 260.0, "broad_m": 22.0, "broad_relief_m": 40.0, "fine_m": 6.0,
          "erosion_iterations": 20, "erosion_k": 0.5, "erosion_cap_m": 6.0,
          "erosion_diffusion": 1.2, "erosion_exponent": 0.5}


def generate(c: Candidate, detail: dict | None = None) -> np.ndarray:
    k = {**DETAIL, **c.detail, **(detail or {})}
    rng = np.random.default_rng(c.seed)
    X0, Y0 = xy()
    # Archetypes are drawn in core coordinates (0..CORE_M); domain warp bends
    # every analytic line so no ridge or valley is ruler-straight.
    X = X0 - CORE_OFF_M + k["warp_m"] * smooth(fractal(rng, 4.0), 3.0)
    Y = Y0 - CORE_OFF_M + k["warp_m"] * smooth(fractal(rng, 4.0), 3.0)
    if c.archetype == "valle":
        dem, _ = _valley(X, Y, _ext([(-300, 1450), (900, 1620), (2000, 1450), (3100, 1680), (4300, 1560)]),
                         floor0=190, grad_per_km=-14, w0=50, left=(500, 1650), right=(330, 800))
        dem = _carve(dem, X, Y, [(1650, 4200), (1850, 3000), (2050, 1600)], 0.38, 190, 170)
        dem = _carve(dem, X, Y, [(3700, 0), (3300, 900), (3200, 1500)], 0.30, 150, 170)
        dem = _flatten(dem, X, Y, 900, 2330, 260)
    elif c.archetype == "affluente":
        dem, _ = _valley(X, Y, _ext([(-200, 250), (1300, 1450), (2500, 2300), (4300, 3650)]),
                         floor0=130, grad_per_km=16, w0=70, left=(430, 1500), right=(390, 1250))
        dem = _carve(dem, X, Y, [(150, 4200), (800, 3000), (1450, 1600)], 0.55, 240, 140)
        dem = _flatten(dem, X, Y, 1400, 1550, 300)
        dem = _carve(dem, X, Y, [(4200, 600), (3300, 1300), (2700, 2100)], 0.25, 150, 160)
    elif c.archetype == "conca":
        r = np.hypot((X - 2000) / 1.15, (Y - 2050) / 0.95)
        dem = 160 + 470 * smoothstep(650, 1950, r)
        dem = _carve(dem, X, Y, _ext([(2100, 1950), (3100, 1750), (4300, 1900)])[1:], 0.85, 260, 150)
        hill = 210 * np.exp(-((np.hypot(X - 1650, Y - 2250) / 420) ** 2))
        dem = dem + np.minimum(hill, 165)
        dem = _carve(dem, X, Y, [(600, 4200), (1100, 3200), (1500, 2700)], 0.30, 160, 170)
    elif c.archetype == "crinale":
        line = _ext([(-200, 3700), (1300, 2550), (2600, 1600), (4300, 450)])
        d, side, along = polyline_distance(X, Y, line)
        total = sum(np.hypot(b[0] - a[0], b[1] - a[1]) for a, b in zip(line[:-1], line[1:]))
        H = 470 - 130 * np.exp(-((along - total * 0.5) / 450) ** 2)  # the saddle, mid-core
        L = np.where(side > 0, 1300, 1900)  # left of travel = SW: shorter, steeper
        dem = 120 + H * (1 - smoothstep(0.0, 1.0, d / L))
        dem = dem + 120 * np.exp(-(polyline_distance(X, Y, [(2500, 1700), (3400, 2700)])[0] / 230) ** 2) \
            * smoothstep(0, 1, 1 - np.hypot(X - 2500, Y - 1700) / 1500)
        # SW side valley: stops short of the crest, so it does not leave a cliff
        dem = _carve(dem, X, Y, [(300, 100), (800, 1000), (1250, 1550)], 0.25, 260, 130)
    else:
        raise ValueError(c.archetype)

    # Buffer ring: regional hills, fading in outside the core, so the edges are
    # landscape rather than an endless plateau.
    out = np.maximum(np.abs(X0 - WORLD_C) , np.abs(Y0 - WORLD_C)) - CORE_M / 2
    ring = smoothstep(0.0, 1500.0, out)
    dem = dem + ring * 110 * (1.0 + np.clip(smooth(fractal(rng, 4.2), 4.0), -1.0, 2.0))

    # Detail: broad undulation plus fine texture, stronger on the slopes than
    # on the floors, then a light blur so 20 m cells stay credible.
    relief = (dem - dem.min()) / (np.ptp(dem) + 1e-9)
    broad = smooth(fractal(rng, 3.4), 2.0)
    fine = fractal(rng, 2.6, min_scale_cells=1.0)
    dem = dem + (k["broad_m"] + k["broad_relief_m"] * relief) * broad + k["fine_m"] * (0.5 + relief) * fine
    dem = erode(dem, iterations=k["erosion_iterations"], k=k["erosion_k"], cap_m=k["erosion_cap_m"],
                diffusion=k["erosion_diffusion"], exponent=k["erosion_exponent"])
    dem = smooth(dem, 0.8)
    # Nothing below 40 m: the extended valley floors keep falling past the core.
    dem = np.maximum(dem, 40.0 + 20.0 * smoothstep(0.0, 60.0, dem - 40.0))
    c.params = {"archetype": c.archetype, "seed": c.seed, "detail": k}
    return dem
