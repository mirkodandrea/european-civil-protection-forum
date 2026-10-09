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
| POST | `/api/player/{role}/reply` | `{"text": "...", "reply_to": message_id, "evidence": {"basis":{"kind":"plan", "reference":"..."}, "attachments":[]}}` → `{"proposal": id}`; parsed asynchronously by the LLM |
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
| POST | `/api/master/resolve` | `{"minutes"?: 30}` → applies approved actions, then simulates the turn live in 1-second steps (phase `resolving`, observable through `/api/state` and `/ws`). The response comes when the turn ends: turn report (applied actions, events) |
| POST | `/api/master/rollback` | `{"turn": n}` |
| POST | `/api/master/end` | Debrief phase |
| POST | `/api/master/settings` | `{"turn_minutes": 30, "speed": 120}`: default Δt, and simulated seconds per real second during resolution (0 = maximum speed) |

## Examples

```bash
B=http://localhost:8080/api; H='content-type: application/json'

curl -XPOST $B/player/mayor/language -H "$H" -d '{"language":"it"}'
curl -XPOST $B/master/start
curl -XPOST $B/master/msel/I-01/send
INJECT_MESSAGE=$(curl -s $B/player/mayor | jq '[.inbox[] | select(.kind == "inject")] | last | .id')
BODY=$(jq -n --argjson reply_to "$INJECT_MESSAGE" '{text:"Issue warning to F4", reply_to:$reply_to, evidence:{basis:{kind:"procedure",reference:"Municipal warning procedure, section 3"}}}')
PROPOSAL=$(curl -s -XPOST $B/player/mayor/reply -H "$H" -d "$BODY" | jq '.proposal')
curl $B/master | jq '.proposals[-1] | {status, descriptions, verdicts, evidence}'
# Master supplies the effective command explicitly; a pending AI parse cannot overwrite this decision.
curl -XPOST $B/master/proposals/$PROPOSAL/decide -H "$H" -d '{"approve":true,"commands":[{"type":"IssueWarning","zone":"F4"}],"note":"Follow warning procedure"}'

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

## Declaration, approval and execution evidence

Replies may set `declaration_kind` to `request` or `proposed_decision` (default).
Declarations are recorded immediately before AI processing. `parsing` means
awaiting interpretation, `pending` means awaiting exercise control, and
`approved` means queued for resolution; none of these statuses imply execution.
The Master can reject during parsing or supply edited commands manually. Late
AI results cannot overwrite decisions or revive rolled-back proposals.

Approval requires an open turn and feasible commands. Invalid AI actions require
an explicit edit. A proposal's `decision` retains the Master identity, approval,
original/effective commands, edit flag, note, turn and timestamps. `history` is
append-only. `executions` retain each attempted command, actual success/failure,
reason and timestamps. Failed commands produce `failed`, rather than claiming
execution. Repeated decision/resolution requests cannot execute a proposal twice.
Rollback marks proposals undone while retaining their evidence. Direct Master
commands carry `direct_excon_intervention` origin; approved player actions and
Master suggestions have distinct origins in execution records.

## Review operational AI communications

AI role messages and clarification questions are `communication_drafts` on the
proposal. They are not delivered during parsing. Master decision requests may
replace `communication_drafts` (each has `to`, `from`, `kind`, `title`, `text_en`)
and Master proposals' `inject_drafts` before approving. Only `role_message` and
`clarification` communication kinds are accepted; recipients and sender origin
are validated. Empty arrays remove drafts.

Approved action-dependent communications wait for successful execution.
Approved communication-only proposals send immediately. Delivery is claimed once,
rechecks effective approval after translation, and records proposal/message IDs.
Rejected, failed or rolled-back suggestions cannot produce operational messages.
Automatic decision feedback and turn summaries translate recorded facts, preserving
original English evidence, rather than generating new operational content.

## Persistent resource accounting

`/api/state` exposes `resource_accounting` by role/resource. Player responses expose
their own accounting and allocations. `requested` is demand in pending declarations;
`committed` is approved demand queued for execution; `deployed` is actual usage;
`available` subtracts both commitments and deployments; `returned` is cumulative
confirmed returns. `external_requested` records executed requests still awaiting
reinforcement arrival. World/debrief data retain allocations, resource events and
actual physical state.

Deployment does not reset when opening a turn or changing phase. Each execution
records its allocation IDs. EXCON returns units with
`{"type":"ReturnResource","allocation":id,"qty":count}`; excess/duplicate returns
are rejected and physical effects remain in place. Inventory cannot be reduced
below active deployment. Snapshot rollback restores the actual ledger but retains
execution/rollback evidence in the exercise log; allocation IDs are never reused.

## Decision evidence and language onboarding

Each role must confirm a supported language through `/api/player/{role}/language`
before declaring a decision. Player data expose `language_confirmed`. New incoming
messages use that language; earlier messages keep the language in which they were
delivered. Players write replies in their language, with original wording retained
alongside the English interpretation. Without translation, incoming messages use
English. The player page explains this before language confirmation.

Reply `evidence` is required. Its `basis` has `kind` (`procedure`, `plan`, or
`operational_judgment`) and a nonempty `reference`: cite the procedure/plan or explain
the operational rationale. Optional `attachments` contain `title` and an HTTP(S)
`url`, linking documents, maps or ordinances. Invalid bases and unsafe URL schemes
are rejected before recording a declaration. Evidence stays attached to proposals,
approval/execution history and debrief records; the Master interface displays the
basis and supporting links. Maximums: 2000 characters of basis, 20 links, 200
characters per title and 4000 characters per URL.
