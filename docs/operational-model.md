# Operational simulation model

The ABM-derived model is opt-in, so existing exercises keep their calibrated
fire lines, drops, exposure and evacuation timing. Enable the operational model:

```sh
cargo run --release -p app -- --operational-model
# Or save the setting in a scenario:
cargo run -p scenario --bin gen-scenario -- --operational-model --out runs/operational_valley.json
cargo run --release -p app -- --scenario runs/operational_valley.json
```

All changes remain behind Master-approved commands and the existing role/resource
checks. The Master can still draw direct fire lines and drops. `DeployEngine`
requires a role allocation; it is not an unconstrained Master effect. Missing
engine capability is added to the synthetic Fire Commander when enabling the
model on an older Valverde package.

## What changes

| Feature | Operational behavior |
|---|---|
| Ground travel | Routes follow a welded metric graph, road classes, one-way rules and drivable/foot-only access. Shared vertices connect; geometric crossings alone do not. Maximum speed remains configurable with `SetDeploymentTravel`. |
| Road restrictions | Police closures bind civilian traffic. Emergency vehicles can pass closures, but not roads made unsafe by fire. With segment closures enabled, fire cuts affect only unsafe graph links. Ground tasks check the untravelled route every simulated second and search again only when a link ahead becomes blocked; cuts behind the unit do not force a detour. Unreachable deployments fail rather than travelling straight across the map. |
| Crew work | Line length is earned at 120 m/h per active crew for a 6 m width, scaled inversely by requested width. Default operational width is 6 m. Fuel removal advances in fire-cell-sized increments; the last fragment is applied on completion. Partial confirmed returns reduce production; full confirmed returns cancel subsequent work. |
| Road engines | `{"type":"DeployEngine","at":"Pineta Primary School"}` stations an engine on a drivable road within 60 m hose reach of the requested target. Its 2,500 L tank pumps at 400 L/min onto exposed, unburnt fuel below 30 added moisture points. It returns to the staging base for a 150 s refill and resumes its post when reachable. With local autonomy, the requested target also defines the approved sector centre (default radius 300 m); the unit can regroup and move to useful unreserved fuel inside it. Only delivered water is charged, capped at the authored moisture target. Staging bases are training refill locations. |
| Aircraft | One approved `WaterDrop` is one 6,137 L drop over the existing 60 m-radius footprint. Dispatch includes 25 minutes of response time. The aircraft returns to staging and replenishes for 90 s; the task completes then. Another drop requires another approved order and available inventory; it does not repeat autonomously. |
| Safety | Ground work stops at danger 0.35. Units withdraw toward staging; if the return route is unavailable, the deployment reports failure at its last physical position. With local autonomy and resumption enabled, engines and incomplete fire-line crews suspend at staging and may resume the same approved mission when safe. Crew position follows earned progress on the original approved line; partial confirmed returns reduce later production. |
| Evacuation | Existing warning-dependent preparation times remain. Occupants then leave in aggregate packets (4 seats for cars, 50 for transport-dependent facilities). Counts become evacuated and sheltered on arrival, rather than when the preparation timer expires. |
| Traffic | Directed links have FIFO order, length-based storage (7.5 m per vehicle), road-class discharge capacity and free-flow travel time. A link cannot discharge into full downstream storage. Closed roads cause waiting or rerouting; unsafe routes expose occupants instead of silently counting them safe. |
| Destinations | Explicit shelters reserve capacity across active and approved evacuations. A closed shelter makes evacuees wait until it opens. Without an explicit destination, vehicles choose reachable open shelters or safe road exits at the map boundary. No reachable destination means people remain in the building. |
| Structure exposure | Radiant reach follows ensemble-mean intensity; ember exposure follows intensity and wind. The load integrates over simulated time using the existing damage thresholds. The ember component contributes one third of full radiant exposure. These are training assumptions, separate from the pedestrian danger field. |
| Communications | Authored sites, or deterministic sites derived for household coverage, fail at danger 0.35. Buildings outside all initial coverage retain their existing warning behavior. If a zone contains households that lose all site coverage, its warning is delivered by a fallback after 20 minutes. No outage means no added delay. |

Every subsystem uses the one-second `World::advance` clock. Routes, work progress,
consumables, queues, reservations, site failures and delayed warnings are included
in world snapshots. The legacy path remains covered by the existing tests.

The Master state API now contains `operations`, `traffic`, `communication_sites`
and `pending_warnings`. Deployments carry routes, work progress, water consumption
and replenishment timestamps. The Master console shows operational counters;
the big-screen view draws routes, moving evacuation packets and urban landmarks.
Role APIs continue to expose only their authorized state.

## Fire and fuels

The Monte Carlo ensemble and its cutoff remain authoritative. Exposure sources
come from cells in the thresholded fire and use the ensemble-mean intensity of
reached realizations. A cell has spatial extent: danger accounts for its half
diagonal rather than treating its center as a dimensionless flame.

Scenario packages can include `fuel_definitions`, preserving EU12 ids and their
spread/spotting parameters. Fuel breaks use a non-burnable id from that table;
id 3 is not assumed non-burnable in EU12. Rendering interprets the table as well.
`operations.spotting` defaults to false and is preserved by `--operational-model`.
Set it explicitly to true in the scenario to activate spotting. Recalibrate the
exercise when changing fuels or spotting; operational timings differ from the
legacy scenario's calibration.

## Scenario Factory

The terrain/vegetation generators and urban mesh bake are adapted from
`propagator_abm`, revision `be08c4bbbf2ffcbb25b7959902b396d8d93ed8c5`, under EUPL-1.2.
Routing, threat, suppression and traffic are adapted to the TTX state model;
the kiosk game loop, scoring and autonomous coordinator are not imported.

```sh
# numpy is required for terrain generation.
python3 scripts/scenario_factory.py nature --candidate t4 --output runs/factory/t4
python3 scripts/scenario_factory.py sweep runs/factory/t4/scenario.json \
  --ignitions runs/factory/t4/ignitions.json --output runs/factory/t4/sweep

# Import an existing populated ABM/Factory package, retaining EU12 fuel ids:
cargo run --release -p scenario --bin import_factory -- \
  /path/to/package /path/to/fuels_eu12.json runs/imported_scenario.json
```

`nature` produces a reproducible 8 km landscape with a 4 km central core and
three geometrically selected ignition patches. `sweep` varies ignition, wind
and seed while Rust runs the actual fire core. Its output contains the complete
job settings, arrival-time rasters, area histories and an SVG/Markdown atlas.
`atlas` can rebuild the visual report from saved results without running physics.

The importer reads `scenario.json`, `osm.json`, `population.json`, `dem.f64`,
`fuel.i32`, and the supplied fuel table. Household sizes are aggregated into
building occupant counts; missing building references fail import. Roads retain
classes, one-way flags and driving restrictions. TTX roles/resources are supplied
as editable templates. Original game-specific MSEL, AI behavior and scoring are
not transplanted; imported scenarios have no initial fire until one is authored.
Render terrain remains the fire DEM; the ABM package's separate fine terrain
and detailed individual-person behavior are outside this integration.

## Verification

```sh
cargo test --release --workspace --offline
cargo test --release -p sim --test operations --offline
```

Operational tests cover route geometry, driving restrictions, civilian closures,
progressive work, partial returns, finite loads, water-flow integration, queues,
storage, shelter reservations, unreachable destinations, count conservation,
warning failure, safety withdrawal, determinism and rollback. Factory smoke
checks exercise terrain generation, package import, a Rust ensemble sweep and
SVG atlas generation.

The original port was verified with 167 workspace tests. The 2026-10-20 update passes 199 tests (including one documentation test); see [implemented changes and verification](operational-upgrade.md). Native startup and the Master state API were exercised with the operational model; a 60 s advance completed successfully, and the 3D screenshot was inspected. A Factory landscape, a two-realization fire sweep and its SVG atlas were generated. The existing Rocca Ventosa package imported as 479 buildings, 70 roads and 1,600 aggregate occupants.


## Browser playtest (2026-10-11)

A single tester exercised Director, Mayor and Fire Commander through the browser
in manual mode (no LLM calls), at 1440×1000 and 390×844. This checks functional
usability; it does not establish usability for first-time human participants.

- Sent an inject, selected Italian/English, submitted decisions with evidence,
  edited actions, approved and separately executed them.
- School evacuation correctly refused two buses for 120 people and accepted
  three after EXCON adjusted inventory. The Northern Road then became cut by
  fire: buses and an engine failed to reach the school. Nobody was counted as
  evacuated. Ground crews using another route progressed from 34/120 m at
  30 minutes to 94/120 m at 60 minutes.
- Rolled back from 60 to 30 simulated minutes and confirmed the restored clock.
  EXCON confirmed return of the failed bus allocation before reusing it.
- Evacuated Valverde Hospital over the southern network: two packets of 50 and
  40 people arrived at the sports centre; 90 evacuated, 90 sheltered, zero in
  transit. Browsers produced no uncaught JavaScript errors in this flow.
- Corrected mobile grid overflow, brought the live Director map before setup
  panels, put proposal tabs first on phones, added workflow/empty-inbox guidance,
  and made approval errors visible at the viewport. Proposal cards and player
  orders now show current task outcomes alongside dispatch records. The command
  catalogue now states the operational fire-line width default of 6 m.

Remaining usability constraints: manual interpretation requires JSON and an
experienced Director; choosing Italian does not localize all interface labels,
and translation falls back to English without an LLM. That playtest used whole-road fire closures. The updated operational model supports segment-local closures; the historical closure behavior remains when the new option is disabled. In the
synthetic initial scenario this can isolate the school very early; validate
scenario accessibility and timing before using it for a facilitated exercise.
The playtest did not validate live AI interpretation or translation quality.

Local screenshots and flow logs are in `runs/playtest/` (ignored Git output).
