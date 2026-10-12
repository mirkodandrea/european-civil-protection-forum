# Simulation model (v0.1)

This page describes the calibrated legacy model. The opt-in
[operational model](operational-model.md) adds road routing, progressive
suppression, finite water loads, traffic, intensity-based exposure and outages.

Deliberately simple around a real fire core: no individual people, no suppression teams. Fire lines and drops take effect directly.

## Fire (`crates/fire`)

- **Core**: PROPAGATOR's Rust core, vendored in `crates/propagator-core`, std-only, running in-process. It uses the legacy 7-class fuel table and 20 m cells, runs a **Monte Carlo ensemble** (default 32 realizations, in parallel) with a fixed seed per run, and has spotting off.
- **From an ensemble to one fire**: a cell counts as reached once a fraction `cutoff` of the realizations have reached it (default 0.5, the median fire; 0.25 gives a pessimistic, planning fire). Its arrival time is the matching **quantile of the realizations' arrival times** (the `ceil(cutoff·R)`-th earliest). Arrival times never change once set, so the fire only grows, and burning/burnt, building exposure and the live view all follow from one consistent arrival-time field. The fraction of runs that reached each cell is kept as `probability`: cells with 0 < p < cutoff form the **uncertainty halo**, drawn faint orange in the 3D view and on the web maps (raster code 5). This summary is recomputed every 10 simulated seconds (`fire::REFRESH_S`). Arrival times are exact; only the moment a cell appears can lag by up to 10 s.
- **Why an ensemble**: one realization is a single random draw. Its outcome varies a lot with the seed (the calibration tests used to average several seeds), and a lucky or unlucky draw would decide the exercise. The median of 32 runs is stable and defensible, and the halo shows the Fire Commander where the fire *might* go.
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
- Resource pools belong to roles, with `total` and `used` (actual deployed units). Deployment usage persists across turns and phases. Each successful resource-consuming command creates an allocation with a unique ID. EXCON confirms partial or full returns with `ReturnResource`; returning units does not undo fire lines, drops or evacuation effects. Requested declarations and approved commitments remain separate from actual deployment. Availability changes cannot reduce total stock below deployed quantities.
- `RequestReinforcement` schedules an arrival (`delay_min` from the offer). On arrival the offer's role gets `total += qty`.

## Calibration (synthetic scenario)

Tuned with `crates/fire/tests/synthetic_spread.rs`: 45 km/h NE wind, 4 % moisture, a 120 m ignition on the south-west flank of Pine Hill about 600 m from Pineta.

| Check | Result |
|---|---|
| Free-burning fire reaches Pineta's edge | about 60–75 min (mean distance to the school after 90 min < 320 m over 3 seeds) |
| A 60 m line across the head (between the fire and Pineta), cut at 20 min | fewer than 1/5 as many cells burn on the Pineta side of the line after 90 min, summed over 3 seeds |
| Cutoff 0.25 vs 0.5 | the 0.25 fire is larger; every cell of the 0.5 fire has probability ≥ 0.5 |
| Unchecked fire for 2.5 h | damages Pineta houses and produces people at risk |

This gives one turn of 30 min to act before contact.

## Turn resolution

A turn advances in **1-second steps** (`exercise::STEP_S`, `sim::SUBSTEP_S`). Each step runs the fire core for one second, then updates building exposure, evacuations, roads and arrivals. The core is event-driven (cells ignite at their own computed times), so the fire does not depend on the step size: one simulated hour gives the same area at 60 s, 10 s and 1 s steps. The results are identical to advancing the whole turn at once (`stepped_and_one_shot_resolution_agree`).

| Cost on the synthetic scenario | 1 realization | 32 realizations |
|---|---|---|
| One simulated hour at 1 s steps | ~0.16 s | ~0.8 s |
| A 30-minute turn at maximum speed (server) | — | ~0.45 s |

The Director's **speed** (simulated seconds per real second; 0 = maximum) only sets how the turn is spread over wall-clock time.

## Known simplifications

- The thresholded ensemble is not one physically possible fire (it can have small gaps), and at a 0.5 cutoff it underplays rare extreme runs. That's fine for a table-top exercise; use `--cutoff 0.25` for a more pessimistic fire.
- No spotting, no smoke, no crews on the ground, no traffic or congestion, no infrastructure cascade model. The Master applies cascades with `SetBuildingStatus` and injects.
- Time jumps run the model forward. A 7-day jump means running 7 days of fire, which is fast (seconds), but the fire does not stop on its own.
