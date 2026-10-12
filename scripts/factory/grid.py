"""The fixed geometry of the territory and the raster helpers everything shares.

Frame (same as `crates/scenario`): x east, y north, metres, SW corner origin;
raster row 0 is the NORTH edge. The fire grid is 20 m (fixed by the model).
"""

from __future__ import annotations

import numpy as np

WORLD_M = 8000.0
N = 400
CELL_M = WORLD_M / N
# The designed landscape (and every ignition) sits in a central core; the ring
# around it is buffer, so fires run hours before they meet the window edge.
CORE_M = 4000.0
CORE_OFF_M = (WORLD_M - CORE_M) / 2


def xy() -> tuple[np.ndarray, np.ndarray]:
    """Cell-centre coordinates, shape (N, N), row 0 north."""
    c = (np.arange(N) + 0.5) * CELL_M
    X, Y = np.meshgrid(c, WORLD_M - c)
    return X, Y


def to_cell(x: float, y: float) -> tuple[int, int]:
    """World metres -> (row, col), as `World::cell_of` does."""
    col = int(np.clip(x // CELL_M, 0, N - 1))
    row = int(np.clip((WORLD_M - y) // CELL_M, 0, N - 1))
    return row, col


def to_xy(row: int, col: int) -> tuple[float, float]:
    return (col + 0.5) * CELL_M, WORLD_M - (row + 0.5) * CELL_M


def smooth(a: np.ndarray, sigma_cells: float) -> np.ndarray:
    """Gaussian blur through the FFT, reflect-padded so the edges do not wrap."""
    if sigma_cells <= 0:
        return a.copy()
    p = int(3 * sigma_cells) + 1
    b = np.pad(a, p, mode="reflect")
    ky = np.fft.fftfreq(b.shape[0])[:, None]
    kx = np.fft.rfftfreq(b.shape[1])[None, :]
    g = np.exp(-2 * (np.pi * sigma_cells) ** 2 * (kx**2 + ky**2))
    out = np.fft.irfft2(np.fft.rfft2(b) * g, s=b.shape)
    return out[p:-p, p:-p]


def fractal(rng: np.random.Generator, beta: float, min_scale_cells: float = 0.0) -> np.ndarray:
    """Power-law noise (spectrum ~ k^-beta), zero mean, unit std. Generated on a
    doubled canvas and cropped, so it is not periodic across the window."""
    m = 2 * N
    ky = np.fft.fftfreq(m)[:, None]
    kx = np.fft.rfftfreq(m)[None, :]
    k = np.sqrt(kx**2 + ky**2)
    k[0, 0] = 1.0
    amp = k ** (-beta / 2.0)  # power ~ k^-beta
    amp[0, 0] = 0.0
    if min_scale_cells > 0:
        amp *= np.exp(-(k * min_scale_cells) ** 2)
    spec = np.fft.rfft2(rng.normal(size=(m, m))) * amp
    f = np.fft.irfft2(spec, s=(m, m))[:N, :N]
    return (f - f.mean()) / f.std()


def polyline_distance(X: np.ndarray, Y: np.ndarray, line: list[tuple[float, float]]):
    """Distance to a polyline, a signed side (+1 left of travel, -1 right) and
    the along-line coordinate of the nearest point (metres from the start)."""
    best = np.full(X.shape, np.inf)
    side = np.ones(X.shape)
    along = np.zeros(X.shape)
    run = 0.0
    for (ax, ay), (bx, by) in zip(line[:-1], line[1:]):
        dx, dy = bx - ax, by - ay
        seg = float(np.hypot(dx, dy))
        t = np.clip(((X - ax) * dx + (Y - ay) * dy) / (seg * seg), 0.0, 1.0)
        px, py = ax + t * dx, ay + t * dy
        d = np.hypot(X - px, Y - py)
        closer = d < best
        best = np.where(closer, d, best)
        cross = dx * (Y - ay) - dy * (X - ax)
        side = np.where(closer, np.where(cross >= 0, 1.0, -1.0), side)
        along = np.where(closer, run + t * seg, along)
        run += seg
    return best, side, along


def smoothstep(e0: float, e1: float, x: np.ndarray) -> np.ndarray:
    t = np.clip((x - e0) / (e1 - e0), 0.0, 1.0)
    return t * t * (3 - 2 * t)


def slope_aspect(dem: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Slope in degrees and `southness` in [-1, 1] (+1 = the slope faces south)."""
    dz_drow, dz_dcol = np.gradient(dem, CELL_M)
    gx, gy = dz_dcol, -dz_drow  # d/dx east, d/dy north (rows run south)
    g = np.hypot(gx, gy)
    slope = np.degrees(np.arctan(g))
    south = np.where(g > 1e-6, gy / np.maximum(g, 1e-6), 0.0)
    return slope, south


def hillshade(dem: np.ndarray, az_deg: float = 315.0, alt_deg: float = 45.0) -> np.ndarray:
    dz_drow, dz_dcol = np.gradient(dem, CELL_M)
    gx, gy = dz_dcol, -dz_drow
    slope = np.arctan(np.hypot(gx, gy))
    aspect = np.arctan2(-gx, -gy)  # bearing the slope faces, clockwise from north
    az, alt = np.radians(az_deg), np.radians(alt_deg)
    hs = np.sin(alt) * np.cos(slope) + np.cos(alt) * np.sin(slope) * np.cos(az - aspect)
    return np.clip(hs, 0, 1)


_D8 = [(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)]


def flow_accumulation(dem: np.ndarray) -> np.ndarray:
    """D8 upslope area, in cells. Steepest-descent receivers, accumulated from
    the highest cell down; pits simply keep what reaches them."""
    p = np.pad(dem, 1, mode="edge")
    best = np.zeros(dem.shape)
    recv = np.full(dem.shape, -1, dtype=np.int64)
    idx = np.arange(N * N).reshape(N, N)
    pidx = np.pad(idx, 1, mode="edge")
    for dr, dc in _D8:
        nb = p[1 + dr:1 + dr + N, 1 + dc:1 + dc + N]
        drop = (dem - nb) / np.hypot(dr, dc)
        better = drop > best
        best = np.where(better, drop, best)
        recv = np.where(better, pidx[1 + dr:1 + dr + N, 1 + dc:1 + dc + N], recv)
    acc = np.ones(N * N)
    r = recv.ravel()
    for i in np.argsort(dem.ravel())[::-1]:
        if r[i] >= 0 and r[i] != i:
            acc[r[i]] += acc[i]
    return acc.reshape(N, N)


def erode(dem: np.ndarray, iterations: int = 8, k: float = 1.0, cap_m: float = 6.0,
          diffusion: float = 0.6, exponent: float = 0.4) -> np.ndarray:
    """Stream-power incision (E ~ A^0.4 * S) with a little diffusion each pass:
    enough to cut a dendritic net of gullies into analytic slopes."""
    d = dem.copy()
    for _ in range(iterations):
        acc = np.maximum(smooth(flow_accumulation(d), 0.7), 1.0)
        s, _ = slope_aspect(d)
        e = np.minimum(k * acc**exponent * np.tan(np.radians(s)), cap_m)
        d = smooth(d - smooth(e, diffusion), 0.5)
    return d


def mode_filter(cls: np.ndarray, n_classes: int, passes: int = 2) -> np.ndarray:
    """3x3 majority filter for a categorical raster: removes speckle so the
    vegetation comes in patches rather than a dithered texture."""
    out = cls.copy()
    for _ in range(passes):
        votes = np.zeros((n_classes,) + cls.shape)
        p = np.pad(out, 1, mode="edge")
        for dr in (-1, 0, 1):
            for dc in (-1, 0, 1):
                win = p[1 + dr:1 + dr + N, 1 + dc:1 + dc + N]
                for k in range(n_classes):
                    votes[k] += win == k
        out = votes.argmax(axis=0)
    return out
