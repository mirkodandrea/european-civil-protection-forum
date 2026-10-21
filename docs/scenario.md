# Scenario: Valverde valley (synthetic)

The application now starts with the [Factory landscape](landscape-generation.md),
`data/scenarios/valverde_factory.json`. The following describes the historical
calibrated template, retained for tests and recorded demos.

Generated deterministically by `scenario::synthetic::generate(seed)` and written to `data/scenarios/synthetic_valley.json`:

```bash
cargo run -p scenario --bin gen-scenario -- [--seed 7] [--out data/scenarios/synthetic_valley.json]
```

## Map (4 × 4 km, 20 m cells, zone grid 8 × 8 of 500 m)

```
      A     B     C     D     E     F     G     H
 1  ─────────── North Forest / ridge (conifers, shrubs) ───────────
 2                         North Ridge         ·Ridge Track·
 3                                   Pine Hill ▲  🔥 ignition (G3)
 4                              Pineta ● school ● (F4)
 5                      (fuel station ● E4, west of Pineta)
 6   ····· Valverde town ● hospital · town hall · sports centre (shelter) ·····  SP1 Main Road
 7          farmland (grass, agro-forestry), farmhouses
 8   ~~~~~~~~~~~~~~~~~~~~~~~~~~~~ River Lenta ~~~~~~~~~~~~~~~~~~~~~~~~~
```

- **Terrain**: a valley floor rising to a ridge in the north, with Pine Hill (+140 m) north-east of Pineta.
- **Fuel**: conifers, shrubs and broadleaves in the forest band; a mixed band on the forest edge; grass and agro-forestry on the plain. Non-burnable: the river, Valverde, Pineta and every building cell.
- **Buildings (266)**: 104 in Valverde, 45 in Pineta, the rest farmhouses on the plain, plus named facilities:

| Facility | Zone | Occupants | Notes |
|---|---|---|---|
| Pineta Primary School | F4 | 120 | Needs buses |
| Pineta Fuel Station | E4 | 4 | Cascade candidate |
| Valverde Town Hall | D6 | 40 | |
| Valverde Hospital | C6 | 90 | Needs buses |
| Valverde Sports Centre | D6 | 0 | Shelter, capacity 400 |
| Valverde Timber Mill | E5 | 25 | |

- **Roads**: `sp1` (SP1 Main Road, E–W), `north_road` (Valverde → Pineta → forest), `ridge_track`, `south_road`.
- **Opening situation**: Day 1 13:00, a fire of about 5 ha on the south-west flank of Pine Hill, NE wind 45 km/h, fuel moisture 4 %.

## Roles

| Role | Language | Actions | Resources per turn |
|---|---|---|---|
| `mayor`: Mayor of Valverde (local) | it | EvacuateZone, EvacuateBuilding, IssueWarning, OpenShelter, CloseRoad, ReopenRoad | 2 buses, 2 police patrols |
| `fire_commander`: Fire and Rescue Commander (local) | en | CutFireLine, WaterDrop | 3 ground crews, 1 air tanker |
| `regional_cp`: Regional Civil Protection | en | RequestReinforcement, OpenShelter, IssueWarning, EvacuateZone, EvacuateBuilding | none (coordinates, requests) |

Players can switch language in their page (it, en, fr, de, es, hy, ka, uk, ro, az, el, pt).

## Reinforcement offers

| Resource | Max | Delay | Source | Goes to |
|---|---|---|---|---|
| ground_crews | 3 | 60 min | Regional fire service | fire_commander |
| air_tankers | 2 | 120 min | International assistance (UCPM / rescEU) | fire_commander |
| buses | 4 | 30 min | Regional transport company | mayor |
| police_patrols | 2 | 30 min | Regional police | mayor |

## MSEL

| Id | Turn | Title | To | Objective |
|---|---|---|---|---|
| I-01 | 1 | Fire reported on Pine Hill | all | Initial situational awareness |
| I-02 | 1 | School in session | mayor | Early action for vulnerable groups |
| I-03 | 1 | Fire behaviour estimate | fire_commander | Tactical use of lines and aircraft |
| I-04 | 2 | Wind strengthening | all | Anticipating escalation (pair with `SetWeather`) |
| I-05 | 2 | Northern Road congestion | mayor, fire_commander | Competing use of the route |
| I-06 | 3 | Fuel station threatened | fire_commander, mayor | Critical infrastructure, cascades |
| I-07 | 3 | ERCC monitoring | regional_cp | UCPM interface |
| I-08 | 4 | Local capacity exhausted | regional_cp, fire_commander | Escalation decision |

## Making another scenario

A scenario is one JSON file matching `scenario::Scenario`: grid, fuel and DEM rasters (row 0 north), start clock, weather, ignitions, buildings, roads, sectors, zone grid, places (with aliases), resource types, roles, reinforcement offers and MSEL. `Scenario::check()` validates references. Real places can be baked with the `propagator_abm` pipeline (EU fuel map, DEM, OSM), mapping its fuel classes to the legacy table or porting its eu12 fuel table into `fire`.
