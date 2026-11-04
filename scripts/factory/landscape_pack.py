"""The visual-only `landscape` block of a territory imported from propagator_abm.

Rendering detail the TTX model never reads: the source's 5 m heights and land
cover, its island description (port, sea stacks, landmarks) and the traced
building footprints. The renderer (`crates/viz/src/land`) draws the desktop
game's landscape from it; without it the renderer falls back to the 10 m
`render_terrain` and the building rectangles.

Rasters travel packed: `u8` classes zlib-compressed, heights as centimetre
deltas along each row (`i16`), zlib-compressed; both base64.

    python3 scripts/factory/landscape_pack.py [SOURCE] [TERRITORY_JSON]

patches only the `landscape` key of an existing territory in place.
"""
import base64
import hashlib
import json
import sys
import zlib
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
SOURCE = Path.home() / 'dev/experiments/propagator_abm/data/scenarios/rocca_ventosa'


def b64(data: bytes) -> str:
    return base64.b64encode(zlib.compress(data, 9)).decode('ascii')


def packed_heights(elev: np.ndarray) -> str:
    cm = np.round(elev * 100.0).astype(np.int64)
    delta = np.diff(cm, axis=1, prepend=0)
    if delta.min() < -32768 or delta.max() > 32767:
        raise ValueError('Height step too large for i16 deltas')
    return b64(delta.astype('<i2').tobytes())


def unpack_heights(text: str, rows: int, cols: int) -> np.ndarray:
    delta = np.frombuffer(zlib.decompress(base64.b64decode(text)), dtype='<i2').reshape(rows, cols)
    return np.cumsum(delta.astype(np.int64), axis=1) / 100.0


def pack(source: Path, buildings: list) -> dict:
    """`buildings`: the territory's buildings; the first N are the source's, in order."""
    meta = json.loads((source / 'render_terrain.json').read_text())
    rows, cols = meta['rows'], meta['cols']
    elev = np.fromfile(source / 'render_terrain.f32', dtype='<f4').reshape(rows, cols).astype(np.float64)
    out = dict(terrain=dict(rows=rows, cols=cols, cell_m=meta['posting_m'],
                            world_size_m=meta['world_size_m'], encoding='i16-cm-row-delta-zlib',
                            data=packed_heights(elev)))
    check = unpack_heights(out['terrain']['data'], rows, cols)
    if np.abs(check - elev).max() > 0.006:
        raise ValueError('Packed heights do not round-trip')
    cover_meta = source / 'cover.json'
    if cover_meta.exists():
        cm = json.loads(cover_meta.read_text())
        classes = (source / 'cover.u8').read_bytes()
        if len(classes) != cm['rows'] * cm['cols']:
            raise ValueError('cover.u8 size')
        out['cover'] = dict(rows=cm['rows'], cols=cm['cols'], cell_m=cm['cell_m'],
                            world_size_m=cm['world_size_m'], encoding='u8-zlib', data=b64(classes))
    island = source / 'island.json'
    if island.exists():
        out['island'] = json.loads(island.read_text())
    osm = json.loads((source / 'osm.json').read_text())
    population = json.loads((source / 'population.json').read_text())
    levels = {d['osm_id']: d['levels'] for d in population.get('dwellings', [])}
    footprints = []
    for b, src in zip(buildings, osm['buildings']):
        footprints.append(dict(id=b['id'], source_id=src['id'], kind=src['kind'],
                               ring=[[round(x, 2), round(y, 2)] for x, y in src['ring']],
                               levels=levels.get(src['id'], 0)))
    out['buildings'] = footprints
    out['provenance'] = ('propagator_abm Scenario Factory package «%s» (EUPL-1.2): render_terrain, '
                         'cover, island.json, osm footprints and dwelling storeys, unchanged.' % source.name)
    return out


def main():
    source = Path(sys.argv[1]) if len(sys.argv) > 1 else SOURCE
    target = Path(sys.argv[2]) if len(sys.argv) > 2 else ROOT / 'data/scenarios/roccavento_island.json'
    s = json.loads(target.read_text())
    s['landscape'] = pack(source, s['buildings'])
    text = json.dumps(s, ensure_ascii=False, separators=(',', ':')) + '\n'
    target.write_text(text)
    manifest = target.with_suffix('.manifest.json')
    if manifest.exists():
        m = json.loads(manifest.read_text())
        m['scenario_sha256'] = hashlib.sha256(text.encode()).hexdigest()
        m['landscape'] = 'visual-only 5 m terrain and cover, island, footprints (scripts/factory/landscape_pack.py)'
        manifest.write_text(json.dumps(m, ensure_ascii=False, indent=2) + '\n')
    print(f'{target}: landscape {len(text) / 1e6:.1f} MB total')


if __name__ == '__main__':
    main()
