#!/usr/bin/env python3
"""Generate declared synthetic algorithm fixtures, not calibrated operational scenarios.

2026-10-20: transfer the source game's reproducible flat/slope/ridge/chokepoint/
wind-shift laboratory into the TTX using its published scenario schema and World.
Uses only Python's standard library. No hidden refill, added unit or duplicated physics.
"""
import argparse
import copy
import json
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]


def save(path, value):
    path.write_text(json.dumps(value, indent=2) + '\n')


def order(t, actor, kind, **fields):
    return {'at_s': t, 'actor': actor, 'command': {'type': kind, **fields}}


def road(identifier, points, road_class='service'):
    return {'id': identifier, 'name': identifier.replace('_', ' ').title(),
            'points': points, 'class': road_class, 'oneway': False, 'drivable': True}


def scenario(template, case):
    scn = copy.deepcopy(template)
    n, cellsize = 32, 20.0
    scn.update(id='lab_' + case, name='Synthetic laboratory: ' + case,
               description='Declared 640 m algorithm fixture. Not a real territory or validated operational calibration.',
               grid={'rows': n, 'cols': n, 'cellsize': cellsize},
               fuel=[4] * (n * n), dem=[0.0] * (n * n),
               weather={'wind_dir_deg': 0.0, 'wind_speed_kmh': 12.0, 'moisture_pct': 6.0},
               zones={'rows': 2, 'cols': 2, 'size_m': 320.0},
               sectors=[{'id':'lab','name':'Laboratory sector','bounds':[0.0,0.0,640.0,640.0]}], communication_sites=[], fuel_definitions=[], msel=[],
               places=[{'name': 'Staging', 'aliases': [], 'pos': [60.0, 480.0]},
                       {'name': 'Approved sector', 'aliases': [], 'pos': [300.0, 400.0]}])
    # Common vertices every 20 m make local road closures and available access explicit.
    main = [[float(x), 400.0] for x in range(60, 621, 20)]
    scn['roads'] = [road('main_access', main), road('staging_link', [[60.0, 480.0], [60.0, 400.0]])]
    if case != 'single_road':
        scn['roads'] += [road('bypass', [[float(x), 480.0] for x in range(60, 621, 20)]),
                         road('east_link', [[620.0, 480.0], [620.0, 400.0]])]
    if case in ('slope', 'ridge'):
        for row in range(n):
            for col in range(n):
                x, y = (col + .5) * cellsize, (n - row - .5) * cellsize
                scn['dem'][row * n + col] = y * .12 if case == 'slope' else max(0.0, 90.0 - abs(x - 360.0) * .3)
    house = next(b for b in template['buildings'] if b['kind'] == 'house')
    shelter = next(b for b in template['buildings'] if b['kind'] == 'sports_centre')
    scn['buildings'] = []
    for i, pos in enumerate([[300.0, 260.0], [380.0, 260.0], [460.0, 260.0]], 1):
        building = copy.deepcopy(house)
        building.update(id=i, name='Lab dwelling ' + str(i), pos=pos, occupants=4, sector='lab')
        scn['buildings'].append(building)
    building = copy.deepcopy(shelter)
    building.update(id=4, name='Lab assembly facility', pos=[60.0, 500.0], occupants=0, shelter_capacity=300, sector='lab')
    scn['buildings'].append(building)
    scn.pop('render_terrain', None)
    # Exactly the source fixture's resources, with the same explicit two engines
    # added by enable_operational_model. No resource adjustments in hidden code.
    return scn


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path, help='new directory; refuses to overwrite evidence')
    parser.add_argument('--duration-s', type=int, default=2400)
    parser.add_argument('--realizations', type=int, default=2)
    args = parser.parse_args()
    if not 1500 <= args.duration_s <= 21600 or not 1 <= args.realizations <= 32:
        parser.error('duration must be 1500..21600 s and realizations 1..32')
    if args.output.exists():
        parser.error('output already exists; choose a new directory')
    args.output.mkdir(parents=True)
    template = json.loads((REPO / 'data/scenarios/synthetic_valley.json').read_text())
    cases = []
    for name in ['flat', 'slope', 'ridge', 'single_road', 'wind_shift']:
        save(args.output / (name + '.json'), scenario(template, name))
        injects = []
        if name == 'single_road':
            # Announced EXCON hazard at the sole access, shared by every paired run.
            injects = [order(900, 'master', 'Ignite', at={'x': 220, 'y': 400}, radius_m=25)]
        if name == 'wind_shift':
            injects = [order(720, 'master', 'SetWeather', wind_dir_deg=180.0),
                       order(1140, 'master', 'SetWeather', wind_dir_deg=0.0)]
        cases.append({'name': name, 'path': name + '.json', 'injects': injects})
    setup = order(0, 'master', 'SetDeploymentTravel', role='fire_commander', resource='engines',
                  at='Staging', speed_mps=8.0)
    engines = [order(0, 'fire_commander', 'DeployEngine', at='Approved sector'),
               order(0, 'fire_commander', 'DeployEngine', at='Approved sector')]
    suite = {'scenarios': cases, 'seeds': [1, 2, 3], 'duration_s': args.duration_s,
             'sample_s': 30, 'realizations': args.realizations, 'cutoff': 0.5,
             'variants': [{'name': 'fixed_post', 'operational': True, 'parameters': {'/operations/local_autonomy': False}},
                          {'name': 'local_policy', 'operational': True, 'parameters': {'/operations/local_autonomy': True}}],
             'strategies': [{'name': 'paired_engines', 'orders': engines}], 'injects': [order(0, 'master', 'Ignite', at={'x': 360.0, 'y': 340.0}, radius_m=35.0), setup]}
    save(args.output / 'suite.json', suite)
    # Isolate the resume switch under one deliberately conservative safety threshold.
    # The fixed-post model hardcodes 0.35; comparing its threshold to 0.2 would confound
    # recovery with safety policy. Both resume arms therefore use local autonomy.
    resume = copy.deepcopy(suite)
    resume['scenarios'] = [cases[-1]]
    resume['variants'] = [{'name': name, 'operational': True, 'parameters': {
        '/operations/local_autonomy': True, '/tactical_policy/work_danger': 0.2,
        '/tactical_policy/resume_after_withdrawal': enabled}}
        for name, enabled in [('resume_off', False), ('resume_on', True)]]
    save(args.output / 'resume-suite.json', resume)
    save(args.output / 'recipe.json', {'generator': 'scripts/lab/generate_cases.py',
        'template': 'data/scenarios/synthetic_valley.json', 'duration_s': args.duration_s,
        'realizations': args.realizations, 'fuel': 'legacy grassland (4)',
        'grid': '32x32 cells of20 m; all1024 cells burnable before interventions',
        'resources': 'source scenario plus explicit operational-model engines; no extra units',
        'purpose': 'algorithm diagnostics, not calibrated tactics; compare paired seeds and retain adverse results'})
    print(args.output / 'suite.json')
    print(args.output / 'resume-suite.json')


if __name__ == '__main__':
    main()
