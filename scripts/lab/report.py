#!/usr/bin/env python3
"""Summarise paired runs without hiding seed-level evidence or rejected orders."""
import csv
import json
import statistics
import sys
from collections import defaultdict
from pathlib import Path


def main():
    root = Path(sys.argv[1])
    manifest = json.loads((root / 'manifest.json').read_text())
    groups = defaultdict(list)
    for run in manifest['results']:
        groups[(run['scenario'], run['variant'], run['strategy'])].append(run)
    rows = []
    for (scenario, variant, strategy), runs in groups.items():
        avoided = [r['paired_reference']['burned_ha_avoided'] for r in runs]
        rows.append({
            'scenario': scenario, 'variant': variant, 'strategy': strategy,
            'seeds': ' '.join(str(r['seed']) for r in runs),
            'runs': len(runs), 'mean_ha_avoided': statistics.mean(avoided),
            'min_ha_avoided': min(avoided), 'max_ha_avoided': max(avoided),
            'sample_sd_ha_avoided': statistics.stdev(avoided) if len(avoided) > 1 else '',
            'mean_evacuated': statistics.mean(r['metrics']['stats']['evacuated_people'] for r in runs),
            'rejected_orders': sum(r['metrics']['rejected_orders'] for r in runs),
        })
    if not rows:
        raise SystemExit('No runs in manifest')
    with (root / 'summary.csv').open('w', newline='') as stream:
        writer = csv.DictWriter(stream, fieldnames=rows[0].keys(), lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)
    report = ['# Operational laboratory', '',
              'Paired model counterfactuals. Physical quantities are not participant scores.', '',
              '| Scenario / variant | Strategy | Seeds | Mean ha avoided (min–max) | Mean evacuated | Rejected |',
              '|---|---|---|---:|---:|---:|']
    for row in rows:
        report.append(f"| {row['scenario']} / {row['variant']} | {row['strategy']} | {row['seeds']} | "
                      f"{row['mean_ha_avoided']:.2f} ({row['min_ha_avoided']:.2f}–{row['max_ha_avoided']:.2f}) | "
                      f"{row['mean_evacuated']:.1f} | {row['rejected_orders']} |")
    report.extend(['', 'The seed-level metrics.csv and run traces remain the primary evidence.',
                   'Ranges and sample standard deviations describe this sample; no significance claim is implied.',
                   'Check rejected orders and pending evacuations before comparing strategies.', ''])
    (root / 'report.md').write_text('\n'.join(report))
    print(root / 'report.md')


if __name__ == '__main__':
    main()
