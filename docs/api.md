# HTTP / WebSocket API

Everything the UIs do goes through this API, so the whole exercise can be driven by scripts (see `scripts/demo_session.sh`, and `scripts/record/session.js`, which drives the UIs themselves in a browser). JSON in, JSON out. Errors are `400 {"error": "..."}`. No authentication: run it on a dedicated LAN.

## Reads

| Method | Path | Returns |
|---|---|---|
| GET | `/api/static` | Scenario: grid, zones, base64 fuel raster, buildings, roads, places, roles, resource types, reinforcement offers, `msel` |
| GET | `/api/state` | `turn`, `phase` (`briefing` / `orders` / `resolving` / `debrief`), `turn_minutes`, `speed`, `resolving` (`{turn, from_s, to_s, minutes}` while a turn is being simulated), `generation`, `last_turn`, `players`, `sent_injects`, `llm`, and `world` (stats, weather, `ensemble` `{realizations, cutoff}`, base64 fire raster, building states, roads, fire lines, drops, warned zones, shelters, resource pools, arrivals, burning zones) |
| GET | `/api/master` | All proposals, all messages, last 300 log entries, full digest |
| GET | `/api/player/{role}` | The player, role, turn, phase, clock, inbox, own resources, own proposals |
| GET | `/api/catalogue` | Command kinds, the Master-only list, catalogue documentation, roles |
| GET | `/api/debrief` | Stats, proposals, messages, full log |
| GET | `/ws` | WebSocket; pushes `{"generation": n}` whenever anything changes |

Fire raster: one byte per cell, row 0 at the north edge, codes `0` unburnt, `1` burning, `2` burnt, `3` fire line, `4` wetted, `5` possible (reached by some Monte Carlo runs, below the cutoff).

## Players

| Method | Path | Body |
|---|---|---|
| POST | `/api/player/{role}/reply` | `{"text": "...", "reply_to": message_id}` → `{"proposal": id}`; parsed asynchronously by the LLM |
| POST | `/api/player/{role}/language` | `{"language": "it"}` |

## Director

| Method | Path | Body / effect |
|---|---|---|
| POST | `/api/master/start` | Opens turn 1 |
| POST | `/api/master/inject` | `{"title", "sender"?, "recipients": ["mayor"] or ["all"], "text"}`; translated per recipient |
| POST | `/api/master/msel/{id}/send` | Sends an MSEL inject |
| POST | `/api/master/prompt` | `{"text": "...", "reply_to": message_id}` → proposal with commands and/or inject drafts (AI) |
| POST | `/api/master/proposals/{id}/decide` | `{"approve": bool, "commands"?: [...], "note"?: "..."}`; `commands` replaces the proposal's actions (edit); feasibility is re-checked |
| POST | `/api/master/command` | One command or a list, applied immediately as the Master → per-command results |
| POST | `/api/master/resolve` | `{"minutes"?: 30}` → applies approved actions, then simulates the turn live in 1-second steps (phase `resolving`, observable through `/api/state` and `/ws`). The response comes when the turn ends: turn report (applied actions, events) |
| POST | `/api/master/rollback` | `{"turn": n}` |
| POST | `/api/master/end` | Debrief phase |
| POST | `/api/master/settings` | `{"turn_minutes": 30, "speed": 120}`: default Δt, and simulated seconds per real second during resolution (0 = maximum speed) |

## Examples

```bash
B=http://localhost:8080/api; H='content-type: application/json'

curl -XPOST $B/master/start
curl -XPOST $B/master/msel/I-01/send
curl -XPOST $B/player/mayor/reply -H "$H" -d '{"text":"Evacuate zone F4 to the sports centre","reply_to":3}'
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

## Inject threads

Every transmitted inject has a unique numeric `inject_id` shared across its
recipient messages. Re-sending the same MSEL item creates a new delivery thread;
`inject_sent` log entries retain the original `msel_id` and recipient message IDs.
Messages retain sender, all inject recipients, simulated `t_s`/`clock`, wall-clock
`wall_ms`, and turn. Proposals, replies, clarifications and feedback carry the
same `inject_id`; `reply_to` references the selected received message.

A reply must reference an inject or clarification delivered to that player.
Missing references return 422; unknown references or another role's messages
return 400 without creating a declaration. Obtain the actual message ID from
`GET /api/player/{role}` before composing; the example ID above is illustrative.
Master and debrief records retain the full thread across turns and rollbacks.
