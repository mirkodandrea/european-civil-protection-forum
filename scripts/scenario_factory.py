#!/usr/bin/env python3
"""TTX Scenario Factory: nature -> Rust fire sweep -> inspection atlas.

Terrain/vegetation kernels come from propagator_abm. Fire physics always runs
in the Rust core. Import a populated Factory package with import_factory.
Requires numpy for generation; atlas and sweep orchestration use stdlib only.
"""
import argparse
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def rust(binary, *args):
    subprocess.run(['cargo', 'run', '--release', '--offline', '-q', '-p',
                    'scenario' if binary == 'import_factory' else 'sim',
                    '--bin', binary, '--', *map(str, args)], cwd=ROOT, check=True)


def nature(args):
    import numpy as np
    from factory import terrain, vegetation, grid
    candidate = terrain.by_id(args.candidate)
    if args.seed is not None:
        from dataclasses import replace
        candidate = replace(candidate, seed=args.seed)
    dem = terrain.generate(candidate)
    fuel, veg_params = vegetation.generate(dem, candidate.seed)
    package = args.output / 'package'
    package.mkdir(parents=True, exist_ok=True)
    dem.astype('<f8').tofile(package / 'dem.f64')
    fuel.astype('<i4').tofile(package / 'fuel.i32')
    write(package / 'scenario.json', {'id': candidate.id, 'name': candidate.name})
    write(package / 'osm.json', {'fire_grid': {'rows': grid.N, 'cols': grid.N,
        'cellsize': grid.CELL_M}, 'buildings': [], 'roads': [], 'water': []})
    write(package / 'population.json', {'households': []})
    write(args.output / 'params.json', {'candidate': vars(candidate), 'vegetation': veg_params,
        'source': 'propagator_abm Scenario Factory', 'cell_m': grid.CELL_M})
    rust('import_factory', package, ROOT / 'scripts/factory/fuels_eu12.json', args.output / 'scenario.json')
    # Nature candidates have no towns yet. Keep fire selection independent of outcomes.
    scn = read(args.output / 'scenario.json')
    good = np.argwhere((fuel >= 1) & (fuel <= 12))
    margin = int(800 / grid.CELL_M)
    good = good[(good[:, 0] > margin) & (good[:, 0] < grid.N-margin) &
                (good[:, 1] > margin) & (good[:, 1] < grid.N-margin)]
    good = np.array([p for p in good if ((fuel[p[0]-3:p[0]+4,p[1]-3:p[1]+4]>=1) &
                    (fuel[p[0]-3:p[0]+4,p[1]-3:p[1]+4]<=12)).mean() > .8])
    if not len(good):
        raise ValueError('No viable ignition patch in the core')
    picks = [good[np.argmin(((good-grid.N/2)**2).sum(axis=1))]]
    d = ((good-picks[0])**2).sum(axis=1)
    for _ in range(2):
        picks.append(good[np.argmax(d)])
        d = np.minimum(d, ((good-picks[-1])**2).sum(axis=1))
    ignitions = [list(grid.to_xy(int(r),int(c))) for r,c in picks]
    scn['initial_ignitions'] = [{'at': ignitions[0], 'radius_m': 60}]
    write(args.output / 'scenario.json', scn)
    write(args.output / 'ignitions.json', ignitions)
    print(f'Generated {candidate.name}: {args.output}')


def sweep(args):
    scn = read(args.scenario)
    starts = read(args.ignitions) if args.ignitions else [i['at'] for i in scn['initial_ignitions']]
    if not starts:
        raise ValueError('Author initial_ignitions or pass --ignitions')
    jobs = [{'name': f'I{i+1}_w{wind}_s{seed}', 'seed': seed, 'at': at,
        'radius_m': 60, 'wind_from_deg': wind, 'wind_kmh': args.wind_kmh,
        'moisture_pct': args.moisture, 'minutes': args.minutes,
        'realizations': args.realizations, 'cutoff': args.cutoff}
        for i,at in enumerate(starts) for wind in args.winds for seed in args.seeds]
    args.output.mkdir(parents=True, exist_ok=True)
    write(args.output / 'jobs.json', jobs)
    rust('scenario_sweep', args.scenario, args.output / 'jobs.json', args.output)
    atlas(args.output)


def atlas(folder):
    results = read(folder / 'results.json')
    g = results['grid']; rows, cols = g['rows'], g['cols']
    lines = ['# Fire atlas', '', '| Run | Burned ha | Established ≥5 ha | Arrival map |',
             '|---|---:|---|---|']
    # SVG scales the original raster without requiring plotting dependencies.
    for index, run in enumerate(results['results']):
        arrivals = read(folder / run['arrival'])
        minutes = run['job']['minutes']
        rects = []
        for flat, t in enumerate(arrivals):
            if t < 0:
                continue
            fraction = min(1, t / (minutes * 60))
            color = f'rgb({int(255*(1-fraction))},{int(60+120*fraction)},40)'
            rects.append(f'<rect x="{flat%cols}" y="{flat//cols}" width="1" height="1" fill="{color}"/>')
        filename = f'arrival-{index:04}.svg'
        (folder / filename).write_text(f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {cols} {rows}">' +
            f'<rect width="{cols}" height="{rows}" fill="#ece8dc"/>' + ''.join(rects) + '</svg>')
        lines.append(f"| {run['job']['name']} | {run['burned_ha']:.1f} | {run['established']} | [map]({filename}) |")
    (folder / 'atlas.md').write_text('\n'.join(lines)+'\n')
    print(f'Atlas: {folder / "atlas.md"}')


def read(path):
    return json.loads(Path(path).read_text())


def write(path, value):
    Path(path).write_text(json.dumps(value, indent=2, default=lambda o: o.item())+'\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    p = sub.add_parser('nature'); p.add_argument('--candidate', choices=['t1','t2','t3','t4'], default='t4')
    p.add_argument('--seed', type=int); p.add_argument('--output', type=Path, default=ROOT/'runs/factory/nature'); p.set_defaults(run=nature)
    p = sub.add_parser('sweep'); p.add_argument('scenario', type=Path); p.add_argument('--ignitions', type=Path)
    p.add_argument('--output', type=Path, default=ROOT/'runs/factory/sweep')
    p.add_argument('--seeds', nargs='+', type=int, default=[1,2,3]); p.add_argument('--winds', nargs='+', type=int, default=list(range(0,360,45)))
    p.add_argument('--wind-kmh', type=float, default=40); p.add_argument('--moisture', type=float, default=3)
    p.add_argument('--minutes', type=int, default=180); p.add_argument('--realizations', type=int, default=32)
    p.add_argument('--cutoff', type=float, default=.5); p.set_defaults(run=sweep)
    p = sub.add_parser('atlas'); p.add_argument('folder', type=Path); p.set_defaults(run=lambda a: atlas(a.folder))
    args = parser.parse_args(); args.run(args)


if __name__ == '__main__':
    main()
