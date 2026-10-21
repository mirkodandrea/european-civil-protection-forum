#!/usr/bin/env python3
"""Source-style snapshot state lists, adapted to real TTX fixture approvals.

Every state starts a fresh native process. Angles here are degrees; TTX_CAM
remains radians. Unknown keys and invalid values fail before running the app.
"""
import argparse
import html
import hashlib
import json
import math
import os
from pathlib import Path
import re
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
KEYS = {'name', 't', 'fixture', 'at', 'dist', 'yaw', 'pitch', 'ui', 'marks', 'quality', 'bench', 'settle', 'fire'}


def parse(spec):
    states = []
    names = set()
    for raw in spec.split(';'):
        if not raw.strip():
            continue
        state = {'name': f'snap{len(states)}', 'at': 'home', 'ui': '1', 'marks': '1'}
        for pair in raw.split(','):
            if '=' not in pair:
                raise ValueError(f'Expected key=value: {pair!r}')
            key, value = (x.strip() for x in pair.split('=', 1))
            if key not in KEYS:
                raise ValueError(f'Unknown snapshot key: {key}')
            state[key] = value
        if not re.fullmatch(r'[A-Za-z0-9_-]{1,80}', state['name']) or state['name'] in names:
            raise ValueError('Names must be unique simple file names (letters, numbers, _ or -)')
        names.add(state['name'])
        for key in ['t', 'dist', 'yaw', 'pitch', 'bench', 'settle']:
            if key in state:
                value = float(state[key])
                if not math.isfinite(value):
                    raise ValueError(f'{key} must be finite')
                if key == 't' and (value < 0 or not value.is_integer() or value > 10080):
                    raise ValueError('t must be whole simulated minutes, 0..10080')
                if key in ['bench', 'settle'] and not 0 <= value <= 300:
                    raise ValueError(f'{key} must be 0..300 seconds')
                if key == 'dist' and value < 20:
                    raise ValueError('dist must be at least 20 m')
                if key == 'pitch' and not 3 <= abs(value) <= 89:
                    raise ValueError('pitch must be 3..89 degrees (either sign accepted)')
        if state.get('quality', 'projector') not in ['projector', 'low', 'high']:
            raise ValueError('quality must be projector, low or high')
        if any(state[k] not in ['0', '1'] for k in ['ui', 'marks']):
            raise ValueError('ui and marks must be 0 or 1')
        if state.get('fire', 'continuous') not in ['continuous', 'cells']:
            raise ValueError('fire must be continuous or cells')
        at = state['at']
        if at not in ['home', 'fire'] and not at.startswith(('place:', 'task:')):
            xy = at.split(':')
            if len(xy) != 2 or not all(math.isfinite(float(v)) for v in xy):
                raise ValueError('at must be home, fire, place:NAME, task:ID or X:Y')
        if at.startswith('task:') and not at[5:].isdigit():
            raise ValueError('task needs an authoritative deployment ID')
        states.append(state)
    if not states:
        raise ValueError('Empty snapshot specification')
    return states


def report(out, states, compare):
    cards = []
    for st in states:
        name = html.escape(st['name'])
        before = ''
        if compare and (compare / f'{st["name"]}.png').is_file():
            # Generated relative paths also work when the output folders move together.
            path = html.escape(os.path.relpath(compare / f'{st["name"]}.png', out), quote=True)
            before = f'<figure><figcaption>Before</figcaption><img src="{path}"></figure>'
        cards.append(f'<article><h2>{name}</h2><pre>{html.escape(json.dumps(st, ensure_ascii=False))}</pre>'
                     f'<div>{before}<figure><figcaption>Current</figcaption><img src="{name}.png"></figure></div></article>')
    (out / 'report.html').write_text('<!doctype html><meta charset="utf-8"><title>TTX visual review</title>'
        '<style>body{font:15px system-ui;background:#172027;color:#edf2f4;margin:24px}article{margin:24px 0}'
        'article>div{display:flex;gap:12px}figure{margin:0;flex:1;min-width:0}img{width:100%}'
        'pre{white-space:pre-wrap;overflow-wrap:anywhere}</style><h1>TTX visual review</h1>'
        '<p>Each state was prepared through the exercise authority. Inspect every image; successful capture does not establish model validity.</p>'
        + ''.join(cards), encoding='utf-8')


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--out', type=Path, required=True)
    ap.add_argument('--spec', default=os.environ.get('TTX_SNAP_SPEC', 'name=snap0'))
    ap.add_argument('--scenario', type=Path, default=ROOT / 'data/scenarios/synthetic_valley.json')
    ap.add_argument('--seed', type=int, default=1)
    ap.add_argument('--realizations', type=int, default=4)
    ap.add_argument('--size', default='1600x1000')
    ap.add_argument('--scale', type=float, default=1)
    ap.add_argument('--bench', type=float, default=0)
    ap.add_argument('--compare', type=Path)
    ap.add_argument('--legacy-model', action='store_true')
    ap.add_argument('--no-build', action='store_true')
    ap.add_argument('--timeout', type=float, default=180)
    args = ap.parse_args()
    try:
        states = parse(args.spec)
        if not re.fullmatch(r'\d+x\d+', args.size) or not all(160 <= int(n) <= 8192 for n in args.size.split('x')):
            raise ValueError('size must be WIDTHxHEIGHT, each 160..8192')
        if not math.isfinite(args.scale) or not 0 < args.scale <= 4:
            raise ValueError('scale must be finite and in (0,4]')
        if not math.isfinite(args.bench) or not 0 <= args.bench <= 300:
            raise ValueError('bench must be 0..300 seconds')
        if args.realizations < 1 or args.timeout <= 0:
            raise ValueError('realizations and timeout must be positive')
    except ValueError as exc:
        ap.error(str(exc))
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    # Refuse stale output so a failed capture can never look like a success.
    if any((out / f'{s["name"]}.png').exists() for s in states):
        ap.error('Output already contains a requested PNG; choose a fresh directory')
    if not args.no_build:
        subprocess.run(['cargo', 'build', '--offline', '--release', '-p', 'app'], cwd=ROOT, check=True)
    manifest = {'source_style': 'propagator_abm kiosk/snap.rs',
                'scenario_sha256': hashlib.sha256(args.scenario.resolve().read_bytes()).hexdigest(), 'scenario': str(args.scenario.resolve()),
                'seed': args.seed, 'realizations': args.realizations, 'size': args.size, 'scale': args.scale,
                'operational_model': not args.legacy_model,
                'fire_render': os.environ.get('TTX_FIRE_RENDER', 'continuous'), 'states': [], 'argv': sys.argv[1:]}
    (out / 'manifest.json').write_text(json.dumps(manifest, indent=2))
    for state in states:
        name = state['name']
        fixture = json.loads((ROOT / state['fixture']).read_text()) if 'fixture' in state else {}
        if 't' in state:
            if int(state['t']) == 0:
                fixture.pop('minutes', None)
            else:
                fixture['minutes'] = int(state['t'])
        fixture_path = out / f'{name}.fixture.json'
        fixture_path.write_text(json.dumps(fixture, indent=2))
        (out / f'{name}.state.json').write_text(json.dumps(state, indent=2))
        env = os.environ.copy()
        for key in list(env):
            if key.startswith(('TTX_SNAPSHOT', 'TTX_CAM', 'TTX_START_HOUR')):
                env.pop(key)
        env.update(TTX_SNAPSHOT=str(out / f'{name}.png'), TTX_SNAPSHOT_SPEC=str(fixture_path),
                   TTX_SNAPSHOT_AT=state['at'], TTX_SNAPSHOT_UI=state['ui'], TTX_SNAPSHOT_MARKS=state['marks'],
                   TTX_QUALITY=state.get('quality', 'projector'), TTX_FIRE_RENDER=state.get('fire', os.environ.get('TTX_FIRE_RENDER', 'continuous')), TTX_SIZE=args.size, TTX_SCALE=str(args.scale),
                   TTX_SNAPSHOT_BENCH=state.get('bench', str(args.bench)), TTX_SNAPSHOT_SETTLE=state.get('settle', '3'))
        for key, variable in [('dist', 'DISTANCE'), ('yaw', 'YAW'), ('pitch', 'PITCH')]:
            if key in state:
                env['TTX_SNAPSHOT_' + variable] = state[key]
        command = [str(ROOT / 'target/release/ttx'), '--scenario', str(args.scenario.resolve()),
                   '--seed', str(args.seed), '--realizations', str(args.realizations)]
        if not args.legacy_model:
            command.append('--operational-model')
        started = time.monotonic()
        with (out / f'{name}.log').open('w') as log:
            result = subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=args.timeout)
        text = (out / f'{name}.log').read_text()
        if result.returncode or re.search(r'ERROR|panicked|failed to process shader|Validation Error|snapshot .*failed', text) or not (out / f'{name}.png').is_file():
            raise RuntimeError(f'{name}: snapshot failed; inspect {out / (name + ".log")}')
        manifest['states'].append({'state': state, 'fixture': fixture, 'fire_render': env['TTX_FIRE_RENDER'], 'wall_s': time.monotonic() - started})
        (out / 'manifest.json').write_text(json.dumps(manifest, indent=2))
        print(f'Snapshot OK: {out / (name + ".png")}', flush=True)
    report(out, states, args.compare.resolve() if args.compare else None)
    print(f'Review: {out / "report.html"}')


if __name__ == '__main__':
    main()
