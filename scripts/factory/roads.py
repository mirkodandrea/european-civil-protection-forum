"""DEM road-planning and welded intersections from propagator_abm.
Source: tools/factory/strade.py, EUPL-1.2; see provenance.json.
Pure helpers only. The default TTX network is authored in network.py.
"""
import heapq
import numpy as np

W = 4000.0
STEP = 40.0
MAX_GRADE = 0.18
MOVES_8 = [(a,b) for a in (-1,0,1) for b in (-1,0,1) if a or b]
MOVES_16 = MOVES_8 + [(a,b) for a in (-2,-1,1,2) for b in (-2,-1,1,2) if abs(a) != abs(b)]

def route(dem, a, b, via, max_grade=MAX_GRADE, moves=MOVES_8):
    n = dem.shape[0]
    cell = W / n
    g = int(W / STEP)

    def elev(i, j):  # i = row from the north, j = column
        r = min(n - 1, int(i * STEP / cell))
        c = min(n - 1, int(j * STEP / cell))
        return dem[r, c]

    def node(p):
        return (g - 1 - min(g - 1, int(p[1] / STEP)), min(g - 1, int(p[0] / STEP)))

    def leg(s, t):
        dist = {s: 0.0}
        prev = {}
        pq = [(0.0, s)]
        while pq:
            d, u = heapq.heappop(pq)
            if u == t:
                break
            if d > dist[u]:
                continue
            for di, dj in moves:
                v = (u[0] + di, u[1] + dj)
                if not (0 <= v[0] < g and 0 <= v[1] < g):
                    continue
                ev = elev(*v)
                if ev < 4.0:
                    continue
                ln = STEP * (1.414 if abs(di) == abs(dj) else 1.0 if not (di and dj) else 5 ** 0.5)
                grade = abs(ev - elev(*u)) / ln
                if grade > max_grade:
                    continue
                nd = d + ln * (1 + (grade / 0.06) ** 2)
                if nd < dist.get(v, 1e18):
                    dist[v] = nd
                    prev[v] = u
                    heapq.heappush(pq, (nd, v))
        if t not in prev and s != t:
            raise RuntimeError(f"no road between {s} and {t} under the {max_grade} grade ceiling")
        out = [t]
        while out[-1] != s:
            out.append(prev[out[-1]])
        return out[::-1]

    stops = [node(a)] + [node(v) for v in via] + [node(b)]
    cells = []
    for s, t in zip(stops, stops[1:]):
        cells += leg(s, t)[(1 if cells else 0):]
    return [((j + 0.5) * STEP, (g - 1 - i + 0.5) * STEP) for i, j in cells]


def smooth(pts, passes=4):
    p = np.array(pts, dtype=float)
    for _ in range(passes):
        q = p.copy()
        q[1:-1] = 0.25 * p[:-2] + 0.5 * p[1:-1] + 0.25 * p[2:]
        p = q
    return p


def resample(p, every=25.0):
    seg = np.hypot(*np.diff(p, axis=0).T)
    s = np.concatenate([[0], np.cumsum(seg)])
    t = np.arange(0, s[-1], every)
    t = np.append(t, s[-1])
    return np.stack([np.interp(t, s, p[:, 0]), np.interp(t, s, p[:, 1])], axis=1)


def crossing(a, b, c, d):
    """Where segment ab crosses segment cd, as (point, t on ab, u on cd), or None.
    A crossing at the very ends of ab is not one (the way is welded there);
    one at an end of cd is, so a junction already in the old way is reused."""
    den = (b[0] - a[0]) * (d[1] - c[1]) - (b[1] - a[1]) * (d[0] - c[0])
    if abs(den) < 1e-9:
        return None
    t = ((c[0] - a[0]) * (d[1] - c[1]) - (c[1] - a[1]) * (d[0] - c[0])) / den
    u = ((c[0] - a[0]) * (b[1] - a[1]) - (c[1] - a[1]) * (b[0] - a[0])) / den
    if not (1e-6 < t < 1 - 1e-6 and -1e-3 <= u <= 1 + 1e-3):
        return None
    p = [round(a[0] + t * (b[0] - a[0]), 2), round(a[1] + t * (b[1] - a[1]), 2)]
    # Within half a metre of an old vertex: that vertex, so a second run adds nothing.
    for end, ue in ((c, 0.0), (d, 1.0)):
        if np.hypot(p[0] - end[0], p[1] - end[1]) < 0.5:
            return list(end), t, ue
    if not 0 < u < 1:
        return None
    return p, t, u


def stop_at_first_meeting(pts, end, roads):
    """Cut the way where it first crosses the road it ends on: the routed line
    can cross it and come back, and that last stretch only doubles the road."""
    target = [r for r in roads if r["drivable"] and any(v[0] == end[0] and v[1] == end[1] for v in r["line"])]
    for i in range(len(pts) - 1):
        for r in target:
            ol = r["line"]
            for k in range(len(ol) - 1):
                hit = crossing(pts[i], pts[i + 1], ol[k], ol[k + 1])
                if hit:
                    if 0 < hit[2] < 1:
                        ol.insert(k + 1, list(hit[0]))
                    return np.vstack([pts[: i + 1], [hit[0]]])
    return pts


def weld_crossings(way, others):
    """Put a shared vertex wherever `way` crosses a drivable way of `others`:
    `abm::network` joins ways only where vertices coincide, so a crossing
    without one is a bridge, and an engine cannot turn there."""
    welded = 0
    changed = True
    while changed:
        changed = False
        line = way["line"]
        for i in range(len(line) - 1):
            for o in others:
                if not o["drivable"] or o is way:
                    continue
                ol = o["line"]
                for k in range(len(ol) - 1):
                    hit = crossing(line[i], line[i + 1], ol[k], ol[k + 1])
                    if not hit:
                        continue
                    p, _, u = hit
                    line.insert(i + 1, p)
                    if 0 < u < 1:
                        ol.insert(k + 1, list(p))
                    print(f"  incrocio con {o.get('name')} a ({p[0]:.0f}, {p[1]:.0f})")
                    welded += 1
                    changed = True
                    break
                if changed:
                    break
            if changed:
                break
    return welded

