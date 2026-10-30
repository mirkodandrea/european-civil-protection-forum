"""Roccavento: the propagator_abm island (`rocca_ventosa`) as a TTX territory.

The published Factory package (terrain, EU12 fuel, buildings, roads and
households) goes through the Rust importer unchanged; this recipe then adds
what the TTX needs: a sea bed below 0 m, the six borghi (districts) as sectors
and places, the hand-picked gathering areas of the source game, the AIB posts
and island roles. Geography and population are the source's synthetic data
(EUPL-1.2, see provenance.json); roles, quantities and capacities are authored
training assumptions.
"""
from collections import defaultdict
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

import numpy as np

from . import translations

ID = 'roccavento_island'
SOURCE = Path.home() / 'dev/experiments/propagator_abm/data/scenarios/rocca_ventosa'
ROOT = Path(__file__).resolve().parents[2]
SEA_BED_M = -3.0
VISUAL_POSTING_M = 10.0
# Gathering areas of the source game (`game.json` evacuation.refuges), by borgo.
# Le Ghiande has none on purpose. The source names the area, not a facility, so
# the open ones become TTX gathering areas (shelters EXCON can open).
GATHERING = [
    ('Pianoro di nord-ovest', [3265, 6249], 900, ['Castelvento', 'Fornaci'],
     ['pianoro', 'area di raccolta nord-ovest']),
    ('Porto di Roccavento', [7880, 3413], 300, ['San Rocco'],
     ['porto', 'molo']),
    ('Incrocio con la Costiera', [1778, 3582], 300, ['Pian dei Grilli'],
     ['incrocio della Costiera']),
]
# Le Terrazze gathers at the existing waiting area of Castelvento.
WAITING_REFUGE = {'Le Terrazze': 'Area di attesa di Castelvento'}


def write(path, value, compact=False):
    text = json.dumps(value, ensure_ascii=False, indent=None if compact else 2,
                      separators=(',', ':') if compact else None) + '\n'
    path.write_text(text)
    return hashlib.sha256(text.encode()).hexdigest()


def slug(name):
    return ''.join(c if c.isalnum() else '_' for c in name.lower()).strip('_')


def sea_mask(z, level=0.01):
    """Cells at the source's flat 0 m sea floor connected to the map edge."""
    low = z <= level
    reach = np.zeros_like(low)
    reach[0, :], reach[-1, :], reach[:, 0], reach[:, -1] = low[0], low[-1], low[:, 0], low[:, -1]
    while True:
        grown = reach.copy()
        grown[1:] |= reach[:-1]
        grown[:-1] |= reach[1:]
        grown[:, 1:] |= reach[:, :-1]
        grown[:, :-1] |= reach[:, 1:]
        grown &= low
        if (grown == reach).all():
            return reach
        reach = grown


def imported(source):
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / 'imported.json'
        subprocess.run(['cargo', 'run', '--release', '--offline', '-q', '-p', 'scenario',
                        '--bin', 'import_factory', '--', source,
                        ROOT / 'scripts/factory/fuels_eu12.json', out], cwd=ROOT, check=True)
        return json.loads(out.read_text())


def generate(source):
    s = imported(source)
    meta = json.loads((source / 'scenario.json').read_text())
    game = json.loads((source / 'game.json').read_text())
    island = json.loads((source / 'island.json').read_text())
    osm = json.loads((source / 'osm.json').read_text())
    population = json.loads((source / 'population.json').read_text())
    rows, cols, cell = s['grid']['rows'], s['grid']['cols'], s['grid']['cellsize']
    world = cols * cell

    # Sea bed: the source sea is flat at exactly 0 m, which the TTX draws as
    # land. Lowering it changes no burnable cell (sea fuel is 0) and lets the
    # renderer's sea sheet show. Inland 0 m ground, if any, stays land.
    z = np.array(s['dem'], dtype=float).reshape(rows, cols)
    fuel = np.array(s['fuel']).reshape(rows, cols)
    sea = sea_mask(z)
    if (fuel[sea] != 0).any():
        raise ValueError('Sea cells with burnable fuel')
    z[sea] = SEA_BED_M
    s['dem'] = np.round(z, 3).ravel().tolist()
    rt = s['render_terrain']
    fine = np.array(rt['elev'], dtype=float).reshape(rt['rows'], rt['cols'])
    step = int(round(VISUAL_POSTING_M / rt['posting_m']))
    fine = fine[::step, ::step]
    fine[sea_mask(fine)] = SEA_BED_M
    s['render_terrain'] = dict(rows=fine.shape[0], cols=fine.shape[1], posting_m=VISUAL_POSTING_M,
                               world_size_m=rt['world_size_m'], elev=np.round(fine, 2).ravel().tolist())

    # Borghi: the localities the source gives each household, in its order.
    homes = defaultdict(list)
    for h in population['households']:
        homes[h['locality']].append(h['pos'])
    borghi = [n for n in meta['localities'] if homes.get(n)]
    centre = {n: np.mean(homes[n], axis=0) for n in borghi}
    buildings = s['buildings']
    if len(buildings) != len(osm['buildings']):
        raise ValueError('Importer changed the building list')
    member = {}
    for b, src in zip(buildings, osm['buildings']):
        name = src.get('locality') or min(
            borghi, key=lambda n: np.linalg.norm(centre[n] - b['pos']))
        member[b['id']] = name
        b['sector'] = slug(name)
    # Unique facility names: the source repeats «Chiesa», «Area di attesa»…
    seen = defaultdict(int)
    for b in buildings:
        seen[b.get('name')] += 1
    facilities = []
    for b, src in zip(buildings, osm['buildings']):
        if src['kind'] in ('house', 'villa', 'terrace', 'shop', 'apartments', 'farm'):
            # «Casa N» would label every home on the maps; homes stay `#id`.
            b.pop('name', None)
            continue
        name = b.get('name') or src['kind']
        if src['kind'] == 'fire_station':
            name = f'Presidio AIB di {member[b["id"]]}'
        elif seen[b.get('name')] > 1 or name in ('Piazza', 'Municipio', 'Scuola', 'Cimitero', 'Distributore'):
            name = f'{name} di {member[b["id"]]}'
        b['name'] = name
        facilities.append(b)
    for b in facilities:
        if b.get('visual_kind') == 'assembly':
            # Waiting areas inside the village: near people, not necessarily safe.
            b['shelter_capacity'] = 150
    next_id = max(b['id'] for b in buildings) + 1
    gathering = []
    for name, pos, capacity, serves, _ in GATHERING:
        b = dict(id=next_id, name=name, kind='sports_centre', visual_kind='assembly', pos=pos,
                 size=[40.0, 30.0], occupants=0, shelter_capacity=capacity,
                 sector=slug(min(borghi, key=lambda n: np.linalg.norm(centre[n] - pos))),
                 rotation_deg=0.0)
        next_id += 1
        gathering.append(b)
        # A gathering area is cleared ground, like every building footprint.
        x0, x1 = pos[0] - 22, pos[0] + 22
        y0, y1 = pos[1] - 17, pos[1] + 17
        for r in range(int((world - y1) // cell), int((world - y0) // cell) + 1):
            for c in range(int(x0 // cell), int(x1 // cell) + 1):
                if 0 <= r < rows and 0 <= c < cols:
                    fuel[r, c] = 0
    buildings.extend(gathering)
    s['fuel'] = fuel.ravel().tolist()

    by_name = {b['name']: b for b in buildings if b.get('name')}
    for target in WAITING_REFUGE.values():
        by_name[target]['shelter_capacity'] = 500

    # Sectors per borgo: bounds of its buildings with a margin. They may
    # overlap; impact counts use each building's own sector.
    s['sectors'] = []
    added = {b['id'] for b in gathering}
    for n in borghi:
        pts = np.array([b['pos'] for b in buildings if b['sector'] == slug(n) and b['id'] not in added])
        lo, hi = pts.min(axis=0) - 150, pts.max(axis=0) + 150
        s['sectors'].append(dict(id=slug(n), name=n, bounds=np.round(
            [max(lo[0], 0), max(lo[1], 0), min(hi[0], world), min(hi[1], world)], 1).tolist()))

    refuge_of = {n: g[0] for g in GATHERING for n in g[3]} | WAITING_REFUGE
    places = []
    for n in borghi:
        ids = [b['id'] for b in buildings if b['sector'] == slug(n) and b['id'] not in added]
        radius = float(np.max(np.linalg.norm(np.array(homes[n]) - centre[n], axis=1)))
        places.append(dict(name=n, aliases=[f'borgo di {n}', f'abitato di {n}'],
                           pos=np.round(centre[n], 1).tolist(), radius_m=round(radius + 40),
                           buildings=sorted(ids)))
    s['districts'] = [dict(id=slug(n), name=n, buildings=p['buildings'], **(
        {'gathering': refuge_of[n]} if n in refuge_of else {})) for n, p in zip(borghi, places)]
    evacuation = game['evacuation']
    s['district_evacuation'] = dict(per_min=evacuation['per_min'], prep_min=int(evacuation['prep_min']))
    places += [dict(name=b['name'], aliases=[], pos=b['pos']) for b in facilities]
    places += [dict(name=g[0], aliases=g[4], pos=g[1]) for g in GATHERING]
    stations = game['stations']
    pian_post = 'Presidio AIB di Pian dei Grilli'
    places.append(dict(name=pian_post, aliases=['caserma di Pian dei Grilli'], pos=stations[1]['pos']))
    landmarks = island['landmarks']
    passo = next(r for r in s['roads'] if r['name'] == 'Strada del Passo')

    def elevation(p):
        c, r = int(np.clip(p[0] // cell, 0, cols - 1)), int(np.clip((world - p[1]) // cell, 0, rows - 1))
        return z[r, c]
    valico = max(passo['points'], key=elevation)
    places += [
        dict(name='Rocca', aliases=['la rocca', 'cima', 'castello'], pos=landmarks['rocca']['pos']),
        dict(name='Torre costiera', aliases=['torre'], pos=landmarks['torre']['pos']),
        dict(name='Valico', aliases=['sella', 'passo', 'valico della Strada del Passo'],
             pos=np.round(valico, 1).tolist()),
    ]
    s['places'] = places

    s.update(
        id=ID, name='Isola di Roccavento',
        description=('Isola interamente sintetica di 8 × 8 km (da propagator_abm): sei borghi, '
                     'un crinale con un valico, la rocca sulla cima, porto a est, scogliera sul resto '
                     'della costa. Nessuna geografia reale.'),
        zones=dict(cols=8, rows=8, size_m=world / 8),
        timeline=[], msel=[], start_clock_s=13 * 3600, turn_minutes=30,
        weather=dict(wind_dir_deg=119, wind_speed_kmh=36, moisture_pct=5),
        staging_base='Presidio AIB di Castelvento', operational_grants=[],
        operations={k: k != 'spotting' for k in [
            'road_routing', 'progressive_suppression', 'traffic', 'intensity_exposure', 'spotting',
            'segment_fire_closures', 'terrain_travel', 'emergency_congestion', 'local_autonomy']})
    s['resource_types'] = [dict(id=rid, name=name, description=desc, speed_mps=speed) for rid, name, desc, speed in [
        ('engines', 'Autobotti AIB', 'Le squadre dei presidi dell’isola: missioni stradali con acqua limitata e rifornimento alla base.', 10),
        ('ground_crews', 'Squadre a terra', 'Linee tagliafuoco con viaggio e lavoro progressivo.', 1.4),
        ('air_tankers', 'Mezzi aerei', 'Lanci puntuali; arrivano dal continente su richiesta.', 55),
        ('buses', 'Autobus', '50 posti; necessari per scuola e persone senza auto.', 10),
        ('police_patrols', 'Pattuglie', 'Chiusure al traffico civile. Non riparano le strade.', 12),
        ('drones', 'Droni di monitoraggio', 'Mantengono aggiornata la mappa di al massimo 4 zone ogni 5 minuti mentre sono in posizione (40 minuti), poi rientrano e si ricaricano per 15.', 20)]]
    for r in s['resource_types']:
        if r['id'] == 'drones':
            r['drone'] = dict(max_zones=4, endurance_s=2400, turnaround_s=900, update_interval_s=300)
    crews = defaultdict(int)
    for t in game['roster']:
        crews[t['station']] += 1
    s['roles'] = [dict(id=rid, name=name, level=level, language='it', description=desc,
                       allowed_actions=actions, resources=res) for rid, name, level, desc, actions, res in [
        ('comune', 'Comune di Roccavento', 'local',
         'Sindaco e polizia locale: avvisi, evacuazione dei borghi e aree di raccolta. Sei borghi, '
         'circa 1.500 residenti; l’isola non ha uscite via terra.',
         ['IssueWarning', 'EvacuateDistrict', 'EvacuateZone', 'EvacuateBuilding', 'OpenShelter',
          'CloseRoad', 'ReopenRoad'],
         {'buses': 2, 'police_patrols': 2}),
        ('dos', 'Direzione delle operazioni di spegnimento', 'local',
         f'Squadre dei presidi AIB di Castelvento ({crews[0]}) e Pian dei Grilli ({crews[1]}) e un drone. '
         'I rinforzi arrivano dal continente via mare o via aria.',
         ['CutFireLine', 'WaterDrop', 'DeployEngine', 'MonitorArea', 'Reconnoitre'],
         {'engines': crews[0], 'ground_crews': crews[1], 'drones': 1}),
        ('sala_operativa', 'Sala operativa provinciale', 'regional',
         'Quadro di situazione, richiesta di rinforzi dal continente, comunicazione pubblica.',
         ['RequestReinforcement', 'IssueWarning', 'OpenShelter'], {})]]
    s['excon_roles'] = ['Centrale 112', 'Servizio meteo', 'Capitaneria di porto',
                        'Osservatori sul territorio', 'Radio dell’isola']
    s['reinforcements'] = [dict(resource=r, max_qty=q, delay_min=t, source=src, assigned_to=role) for r, q, t, src, role in [
        ('air_tankers', 1, 45, 'Flotta aerea dal continente', 'dos'),
        ('ground_crews', 2, 90, 'Colonna AIB dal continente (traghetto)', 'dos'),
        ('engines', 1, 90, 'Colonna AIB dal continente (traghetto)', 'dos'),
        ('buses', 2, 60, 'Noleggio autobus dal continente (traghetto)', 'comune')]]
    return s, dict(
        recipe='roccavento-island-v1', source=str(source), source_id=meta['id'],
        source_sha256={f.name: hashlib.sha256(f.read_bytes()).hexdigest()
                       for f in sorted(source.iterdir()) if f.is_file()},
        physical_cell_m=cell, visual_posting_m=VISUAL_POSTING_M, world_size_m=[world, world],
        sea_cells=int(sea.sum()), sea_bed_m=SEA_BED_M, buildings=len(buildings), roads=len(s['roads']),
        occupants=sum(b['occupants'] for b in buildings),
        borghi={n: dict(buildings=sum(b['sector'] == slug(n) for b in buildings),
                        people=sum(b['occupants'] for b in buildings if b['sector'] == slug(n)),
                        gathering=refuge_of.get(n)) for n in borghi},
        provenance='propagator_abm Scenario Factory package (EUPL-1.2), imported unchanged; '
                   'sea bed, borghi, gathering areas, roles and resources authored here.')


def landscape(args):
    s, manifest = generate(args.source)
    translations.fill(s, args.output, llm=getattr(args, 'translate', True))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    manifest['scenario_sha256'] = write(args.output, s, compact=True)
    pack = args.output.parent / 'roccavento_valico.pack.json'
    if pack.exists():
        manifest['pack_sha256'] = hashlib.sha256(pack.read_bytes()).hexdigest()
    write(args.output.with_suffix('.manifest.json'), manifest)
    print(json.dumps({k: v for k, v in manifest.items() if k != 'source_sha256'}, indent=2, ensure_ascii=False))
