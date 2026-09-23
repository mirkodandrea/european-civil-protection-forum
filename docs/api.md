# HTTP / WebSocket API

Everything the UIs do goes through this API, so the whole exercise can be driven by scripts (see `scripts/demo_session.sh`). JSON in, JSON out. Errors are `400 {"error": "..."}`. No authentication: run it on a dedicated LAN.

## Reads

| Method | Path | Returns |
|---|---|---|
| GET | `/api/static` | Scenario: grid, zones, base64 fuel raster, buildings, roads, places, roles, resource types, reinforcement offers, `msel` |
| GET | `/api/state` | `turn`, `phase`, `turn_minutes`, `generation`, `last_turn`, `players`, `sent_injects`, `llm`, and `world` (stats, weather, base64 fire raster, building states, roads, fire lines, drops, warned zones, shelters, resource pools, arrivals, burning zones) |
| GET | `/api/master` | All proposals, all messages, last 300 log entries, full digest |
| GET | `/api/player/{role}` | The player, role, turn, phase, clock, inbox, own resources, own proposals |
| GET | `/api/catalogue` | Command kinds, the Master-only list, catalogue documentation, roles |
| GET | `/api/debrief` | Stats, proposals, messages, full log |
| GET | `/ws` | WebSocket; pushes `{"generation": n}` whenever anything changes |

Fire raster: one byte per cell, row 0 at the north edge, codes `0` unburnt, `1` burning, `2` burnt, `3` fire line, `4` wetted.

## Players

| Method | Path | Body |
|---|---|---|
| POST | `/api/player/{role}/reply` | `{"text": "..."}` → `{"proposal": id}`; parsed asynchronously by the LLM |
| POST | `/api/player/{role}/language` | `{"language": "it"}` |

## Director

| Method | Path | Body / effect |
|---|---|---|
| POST | `/api/master/start` | Opens turn 1 |
| POST | `/api/master/inject` | `{"title", "sender"?, "recipients": ["mayor"] or ["all"], "text"}`; translated per recipient |
| POST | `/api/master/msel/{id}/send` | Sends an MSEL inject |
| POST | `/api/master/prompt` | `{"text": "..."}` → proposal with commands and/or inject drafts (AI) |
| POST | `/api/master/proposals/{id}/decide` | `{"approve": bool, "commands"?: [...], "note"?: "..."}`; `commands` replaces the proposal's actions (edit); feasibility is re-checked |
| POST | `/api/master/command` | One command or a list, applied immediately as the Master → per-command results |
| POST | `/api/master/resolve` | `{"minutes"?: 30}` → turn report (applied actions, events) |
| POST | `/api/master/rollback` | `{"turn": n}` |
| POST | `/api/master/end` | Debrief phase |
| POST | `/api/master/settings` | `{"turn_minutes": 30}` |

## Examples

```bash
B=http://localhost:8080/api; H='content-type: application/json'

curl -XPOST $B/master/start
curl -XPOST $B/master/msel/I-01/send
curl -XPOST $B/player/mayor/reply -H "$H" -d '{"text":"Evacuate zone F4 to the sports centre"}'
curl $B/master | jq '.proposals[-1] | {status, descriptions, verdicts}'
curl -XPOST $B/master/proposals/7/decide -H "$H" -d '{"approve":true,"note":"ok"}'

# Commands by code (no LLM): a list is applied in order
curl -XPOST $B/master/command -H "$H" -d '[
  {"type":"Extinguish","area":"G3"},
  {"type":"CutFireLine","along":"F4:N"},
  {"type":"EvacuateZone","zone":"F4","destination":"Valverde Sports Centre"},
  {"type":"SetWeather","wind_dir_deg":0,"wind_speed_kmh":60}
]'

curl -XPOST $B/master/resolve -H "$H" -d '{"minutes":30}'
```
