"""Borghi (districts) for territories that have no source data for them.

Roccavento takes its borghi from the source game. The other territories derive
them: by sector (Valverde) or by the nearest locality (Selvariva). Only houses
belong to a borgo; a house belongs to one borgo only.
"""
import hashlib
import json
import math
from pathlib import Path

MIN_HOUSES = 5


def slug(name):
    return '_'.join(''.join(c if c.isalnum() else ' ' for c in name.lower()).split())


def _district(name, houses, gathering):
    d = dict(id=slug(name), name=name, buildings=sorted(b['id'] for b in houses))
    if gathering:
        d['gathering'] = gathering
    return d


def by_sector(s, gathering=None):
    """One borgo per sector that holds at least MIN_HOUSES houses."""
    out = []
    for sec in s['sectors']:
        houses = [b for b in s['buildings'] if b['kind'] == 'house' and b['sector'] == sec['id']]
        if len(houses) >= MIN_HOUSES:
            out.append(_district(sec['name'], houses, gathering))
    return out


def by_anchors(s, anchors):
    """Every house goes to its nearest locality. `anchors`: name -> (x, y, gathering or None)."""
    mine = {n: [] for n in anchors}
    for b in s['buildings']:
        if b['kind'] == 'house':
            mine[min(anchors, key=lambda n: math.dist(anchors[n][:2], b['pos']))].append(b)
    return [_district(n, h, anchors[n][2]) for n, h in mine.items() if len(h) >= MIN_HOUSES]


def valverde(s):
    s['districts'] = by_sector(s, gathering='Valverde Sports Centre')
    s['district_evacuation'] = dict(per_min=6.0, prep_min=5)


def selvariva(s):
    s['districts'] = by_anchors(s, {
        'Selvariva': (3450, 2300, 'Centro di accoglienza di Pianalto'),
        'Roverechiare': (2370, 3500, None),
        'I Prati': (3550, 1820, 'Polo di accoglienza dei Prati'),
        'Montebruno': (2200, 4100, None),
        'Pianalto': (4900, 2450, None),
    })
    s['district_evacuation'] = dict(per_min=6.0, prep_min=5)


RECIPES = {'valverde_factory': valverde, 'synthetic_valley': valverde, 'selvariva_basin': selvariva}


def patch(folder):
    """Adds the borghi to the shipped territories in place, keeping their manifests honest."""
    for name, recipe in RECIPES.items():
        path = Path(folder) / f'{name}.json'
        s = json.loads(path.read_text())
        recipe(s)
        text = json.dumps(s, ensure_ascii=False, separators=(',', ':')) + '\n'
        path.write_text(text)
        manifest = path.with_suffix('.manifest.json')
        if manifest.exists():
            m = json.loads(manifest.read_text())
            m['scenario_sha256'] = hashlib.sha256(text.encode()).hexdigest()
            manifest.write_text(json.dumps(m, ensure_ascii=False, indent=2) + '\n')
        print(name, [(d['name'], len(d['buildings'])) for d in s['districts']])
