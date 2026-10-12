# Command catalogue

Every change to the world is a `sim::Command` (`crates/sim/src/command.rs`). The LLM produces the same JSON shapes, and so do the Master map tools and scripts (`POST /api/master/command`). The live documentation is at `GET /api/catalogue`.

## Locations

A `LOC` is any of:

| Form | Example | Resolves to |
|---|---|---|
| Zone code | `"F4"` (any case, `"f 4"`) | Zone centre |
| Named place, facility, sector or road (aliases in several languages) | `"Pine Hill"`, `"Monte Pino"`, `"Pineta Primary School"` | Its position |
| Point in metres (x east, y north) | `{"x": 2800, "y": 2400}` | Itself |

Unknown names are **rejected, never guessed**.

**Zone grid**: 8 × 8 squares of 500 m. Columns `A`–`H` run west → east and rows `1`–`8` north → south, so `A1` is the north-west corner. **Zone edges** are written `F4:N`, `F4:S`, `F4:E`, `F4:W`.

## Player actions

Each role's `allowed_actions` in the scenario decides which of these it may use. Resources remain allocated across turns until EXCON explicitly confirms their return.

| Command | JSON | Cost / rule |
|---|---|---|
| CutFireLine | `{"type":"CutFireLine","from":LOC,"to":LOC}` or `{"type":"CutFireLine","along":"F4:N"}`, optional `"width_m"` (10–200, default 40) | 1 `ground_crews` per 600 m |
| WaterDrop | `{"type":"WaterDrop","at":LOC}` | 1 `air_tankers`; 60 m radius, 1.2 L/m² |
| EvacuateZone | `{"type":"EvacuateZone","zone":"F4","destination":"Valverde Sports Centre"}` | Schools and hospitals need `buses` (50 seats each); must contain occupied buildings |
| EvacuateBuilding | `{"type":"EvacuateBuilding","building":"Pineta Primary School"}` (or `"#12"`) | As above |
| IssueWarning | `{"type":"IssueWarning","zone":"F4"}` | Later evacuations there take 10 min instead of 20 |
| CloseRoad / ReopenRoad | `{"type":"CloseRoad","road":"north_road"}` | Closing costs 1 `police_patrols` |
| OpenShelter | `{"type":"OpenShelter","building":"Valverde Sports Centre"}` | Building must have shelter capacity and be intact |
| RequestReinforcement | `{"type":"RequestReinforcement","resource":"air_tankers","qty":2}` | Limited by the scenario's offer; arrives after its delay, assigned to the offer's role |

## Master only

| Command | JSON |
|---|---|
| Ignite | `{"type":"Ignite","at":LOC,"radius_m":80}` |
| Extinguish | `{"type":"Extinguish","area":"G3"}` (whole zone), `{"type":"Extinguish","area":"Pine Hill"}` (250 m), `{"type":"Extinguish","area":{"at":LOC,"radius_m":150}}` |
| SetWeather | `{"type":"SetWeather","wind_dir_deg":45,"wind_speed_kmh":55,"moisture_pct":4}` (any subset; the direction is where the wind blows **from**) |
| SetBuildingStatus | `{"type":"SetBuildingStatus","building":"Pineta Fuel Station","damage":"destroyed","evacuated":true}`; `building` may be a zone code |
| AdjustResource | `{"type":"AdjustResource","role":"mayor","resource":"buses","delta":1}` |
| ReturnResource | `{"type":"ReturnResource","allocation":1,"qty":1}` (EXCON confirms a partial/full return from a recorded allocation) |
| AdvanceTime | `{"type":"AdvanceTime","minutes":60}` (time jump outside the turn loop) |

The Master may also issue any player action; the Master is not bound by resources or role authority.

## Validation

`World::validate_batch(prior, cmds, actor)` returns one `Verdict` per command:

- `{"verdict":"ok","notes":[...]}`, for example `"500 m line, 1 crew(s)"` or `"3 bus(es) for 120 people"`;
- `{"verdict":"rejected","reason":"..."}`, for example `"moving 120 people without cars needs 3 buses, only 2 currently available"`.

The check covers role authority, resources (after earlier commands in the batch and already-approved proposals), place resolution, capacities and value ranges. Verdicts are computed when a proposal is parsed, again when the Master decides, and again at resolution.

## Operational engines

`DeployEngine` takes `at` in the usual location syntax and consumes one `engines` unit from the approved role. It requires the operational model and a reachable drivable-road post within 60 m hose reach. The engine pumps from a finite 2,500 L tank, refills at staging, and withdraws when unsafe. It does not bypass Master approval. See [operational assumptions](operational-model.md).
