# Simulation model (v0.1)

Deliberately simple around a real fire core: no individual people, no suppression teams. Fire lines and drops take effect directly.

## Fire (`crates/fire`)

- **Core**: PROPAGATOR's Rust core, vendored in `crates/propagator-core`, std-only, running in-process. It uses the legacy 7-class fuel table, 20 m cells, **one stochastic realization** (players fight one concrete fire) and a fixed seed per run. Spotting is off.
- **Weather**: wind direction (meteorological: where it blows *from*), wind speed and fuel moisture, applied from the next core event.
- **Burn-out**: a cell burns for 20 min after ignition, then it is burnt out. This ageing layer is ours; the kernel never un-lights a cell.
- **All levers go through the core's boundary conditions**, never by editing its state:

| Lever | Implementation |
|---|---|
| Ignition | `ignitions`; at least 40 m radius, because single cells fizzle about 20 % of the time. Cells show as burning immediately |
| Fire line | `vegetation_changes` → non-vegetated along a band (default 40 m wide). Acts only on unburnt fuel |
| Water drop | `additional_moisture`: 1.2 L/m² × 30 points per L/m² = +36 points (extinction is 30), decaying 1 %/min |
| Extinguish (Master) | Burning cells in the area are shown as burnt, and the area's fuel is cleared so nothing there burns again |

- Two traps inherited from `propagator_abm` are handled: `vegetation_changes` is NaN-sparse (every non-NaN cell *sets* the fuel), and cells the core lights after being cleared (from ignition events scheduled earlier) are ignored.
- **Snapshots**: the core checkpoint plus our layers. A restore reseeds deterministically.

## Buildings (`crates/sim`)

- Buildings sit on non-burnable cells, so they are threatened by **exposure**, not burnt by the fire model.
- Every 60 s sub-step, each building takes the strongest weight among burning cells within 70 m: 1.0 up to 30 m, 0.5 up to 50 m, 0.2 up to 70 m. Accumulated exposure of ≥ 300 s marks it **damaged**; ≥ 900 s marks it **destroyed**. Exposure is integrated over simulated time, so the result does not depend on the step size.
- **Threatened**: fire burning within 300 m.
- **Occupants** are counts. When a building is damaged with people inside, they are counted as **people at risk**; this indicator replaces casualties.

## Evacuation

- An order sets `done_at = now + 20 min` (10 min if the zone was **warned**). Schools and hospitals take 10 more minutes and need **buses**: 50 seats each, validated against the ordering role's pool.
- When it completes, people are counted as evacuated. If the destination is an open shelter, they are also counted as **sheltered** there.

## Roads, resources, reinforcements

- A road is **cut by fire** when any cell along it burns (reported once). `CloseRoad` is a status for traffic management and uses a police patrol. There is no traffic model yet.
- Resource pools belong to roles, with `total` and `used`; `used` resets each turn.
- `RequestReinforcement` schedules an arrival (`delay_min` from the offer). On arrival the offer's role gets `total += qty`.

## Calibration (synthetic scenario)

Tuned with `crates/fire/tests/synthetic_spread.rs`: 45 km/h NE wind, 4 % moisture, a 120 m ignition on the south-west flank of Pine Hill about 600 m from Pineta.

| Check | Result |
|---|---|
| Free-burning fire reaches Pineta's edge | about 60–75 min (mean distance to the school after 90 min < 320 m over 3 seeds) |
| A 60 m line across the head, cut at 20 min | keeps the fire ≥ 100 m further from the school, summed over 3 seeds |
| Unchecked fire for 2.5 h | damages Pineta houses and produces people at risk |

This gives one turn of 30 min to act before contact.

## Known simplifications

- Single realization: outcomes vary by seed (`--seed`).
- No spotting, no smoke, no crews on the ground, no traffic or congestion, no infrastructure cascade model. The Master applies cascades with `SetBuildingStatus` and injects.
- Time jumps run the model forward. A 7-day jump means running 7 days of fire, which is fast (seconds), but the fire does not stop on its own.
