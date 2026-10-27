"""Footprint geometry from propagator_abm tools/factory/borghi.py (EUPL-1.2).
TTX uses actual footprint sizes, without the source game's 1.9x scaling.
See provenance.json. No files are modified by these helpers.
"""
import math
import numpy as np

def rect_of(ring):
    r = np.array(ring[:4], float)
    e1, e2 = r[1] - r[0], r[2] - r[1]
    return r.mean(0), float(np.hypot(*e1)), float(np.hypot(*e2)), math.atan2(e1[1], e1[0])


def corners(c, w, h, th):
    u = np.array([math.cos(th), math.sin(th)])
    v = np.array([-u[1], u[0]])
    return np.array([c - u * w / 2 - v * h / 2, c + u * w / 2 - v * h / 2,
                     c + u * w / 2 + v * h / 2, c - u * w / 2 + v * h / 2])


def sdf_box(p, c, w, h, th):
    """Signed distance from points p (n,2) to a rotated box; negative inside."""
    u = np.array([math.cos(th), math.sin(th)])
    v = np.array([-u[1], u[0]])
    d = p - c
    q = np.stack([np.abs(d @ u) - w / 2, np.abs(d @ v) - h / 2], 1)
    return np.hypot(*np.maximum(q, 0).T) + np.minimum(np.maximum(q[:, 0], q[:, 1]), 0)


def mtv(a, b):
    """Minimum translation to push convex quad a out of quad b (None if apart)."""
    best = None
    for P in (a, b):
        for i in range(4):
            e = P[(i + 1) % 4] - P[i]
            n = np.array([-e[1], e[0]])
            n /= np.hypot(*n)
            pa, pb = a @ n, b @ n
            o = min(pa.max() - pb.min(), pb.max() - pa.min())
            if o <= 0:
                return None
            if best is None or o < best[0]:
                s = 1.0 if pa.mean() >= pb.mean() else -1.0
                best = (o, n * s)
    return best[1] * best[0]

