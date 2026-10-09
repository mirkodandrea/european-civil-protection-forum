# HTTP / WebSocket API

Everything the UIs do goes through this API, so the whole exercise can be driven by scripts (see `scripts/demo_session.sh`, and `scripts/record/session.js`, which drives the UIs themselves in a browser). JSON in, JSON out. Errors are `400 {"error": "..."}`. Requests require a per-run access key (`Authorization: Bearer KEY`). Missing/invalid keys return 401; keys for a different role or a Master-only endpoint return 403. `/api/health` is the public readiness endpoint.

## Reads

| Method | Path | Returns |
|---|---|---|
| GET | `/api/static` | Master scenario: grid, zones, base64 fuel raster, buildings, roads, places, roles, resource types, reinforcement offers, `msel`; player keys receive only base geography and their own role |
| GET | `/api/state` | `turn`, `phase` (`briefing` / `orders` / `resolving` / `debrief`), `turn_minutes`, `speed`, `resolving` (`{turn, from_s, to_s, minutes}` while a turn is being simulated), `generation`, `last_turn`, `players`, `sent_injects`, `llm`, and `world` (stats, weather, `ensemble` `{realizations, cutoff}`, base64 fire raster, building states, roads, fire lines, drops, warned zones, shelters, resource pools, arrivals, burning zones) |
| GET | `/api/master` | All proposals, all messages, last 300 log entries, full digest |
| GET | `/api/player/{role}` | The player, role, turn, phase, clock, inbox, own resources, own proposals, reported `map`, linked `knowledge_events` |
| GET | `/api/catalogue` | Command kinds, the Master-only list, catalogue documentation, roles |
| GET | `/api/debrief` | Stats, proposals, messages, full log |
| GET | `/ws` | WebSocket (key in `?token=KEY`); pushes `{"generation": n}` whenever anything changes |

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
# Set TTX_MASTER_KEY from the startup link. This wrapper adds the key to every call.
curl() { command curl -H "Authorization: Bearer $TTX_MASTER_KEY" "$@"; }

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

## Role access and map information

At startup the server prints a fresh Master link containing a random access key.
The Master console exposes individual player links (`GET /api/master/access`).
Distribute each role's link to its participant. The all-roles console is a
privileged Director/demo interface. Keys change on restart. Browser pages remove
the key from the address bar after storing it in session storage; scripts use the
Bearer header. Player keys cannot read `/api/state`, Master records, the command
catalogue, debriefs, other inboxes, or mutate Master state. `/api/access` identifies
the current key. WebSocket query parameters such as `role` do not change identity.

Base geography (fuel, grid, named places, building footprints and roads) is
pre-exercise information shared with roles. Player static responses exclude
scenario description/initial conditions, MSEL, population counts, reinforcement
offers and other roles. Player maps contain only explicitly delivered reports;
there is no live raster, weather, casualty total, authoritative building state or
other role's resource ledger. AI reply interpretation uses this same knowledge.
Automatic turn notifications do not disclose the global situation.

`POST /api/master/inject` accepts optional `observation`:
`{"locations":[[2500,2500]],"cells":[{"row":3,"col":4,"state":1}]}`.
Only actual message recipients acquire it. No information is inferred from prose.
Locations are metres; cells use north-first raster row/column and the state codes
above. Out-of-map locations/cells and invalid codes fail before sending anything.
Each delivery/acquisition event records role, message, turn, simulation time and
original observation. Latest reports overwrite the same reported cell; other
cells/locations remain as previously reported. Unmarked areas are unknown, not
confirmed safe. Simulation changes and snapshot rollback do not silently update
or erase a player's acquired information; EXCON sends correction reports.

`POST /api/master/knowledge` records acquired information:
`{"role":"mayor","source":"Field team report","observation":{...}}`.
Alternatively, `{"role":"mayor","source":"Reviewed aerial survey","current_perimeter":true}`
explicitly discloses a full current raster snapshot. This Master-only choice is
also available beside the custom inject form. Subsequent simulation changes stay
undisclosed until another report. Debriefs retain the linked acquisition evidence.

## Read-only evaluators

Three independent evaluator keys are generated per run. The Master obtains their
links from `/api/master/access`. `GET /api/access` identifies the evaluator. Each
key permits read-only Master information (`/api/static`, `/api/state`,
`/api/master`, `/api/catalogue`, `/api/debrief`, `/api/evaluator`, and `/ws`). Every
POST is denied, including player replies/languages, reference changes and Master
controls. Evaluators cannot retrieve participant access keys. The page has no
exercise action controls and supports multiple concurrent evaluators.

`GET /api/evaluator` returns context, role cards, objective-to-MSEL delivery links,
reference plans/procedures, communication chains, map-acquisition evidence,
resource accounting, and a paged evidence timeline. Filters: `role`, `inject`
(unique delivery ID), `turn`, `phase` (`briefing`, `orders`, `resolving`, `debrief`),
`status`, and timeline event `kind`. Chains are selected by declaration turn/phase
and retain their complete subsequent decision/execution history. Timeline filters
apply to each event's own turn/phase. `offset` defaults to 0 and `limit` to 200
(maximum 1000); `timeline_total` counts all matching records, ordered newest first.
The full debrief export retains all evidence, including rolled-back executions.

Response metrics are computed from the delivered parent message, declaration's
first history entry, and Master decision: `reply_delay_wall_ms`,
`reply_delay_sim_s`, `master_queue_wall_ms`, and `master_queue_sim_s`. Missing events
produce null metrics; read/open times and translation-send latency are not inferred.
Wall durations are elapsed milliseconds; simulated durations follow the recorded
simulation clock and can reflect rollback. Role cards count actual recorded
approvals/edits/rejections/executions. Training objectives come from scenario MSEL
entries; actual delivery IDs connect them to communication chains.

The Master can publish a reference library through `POST /api/master/references`
with `{"references":[{"basis":{"kind":"plan","reference":"Plan title/section"},
"attachments":[{"title":"Plan","url":"https://example.test/plan.pdf"}]}]}`.
Plan/procedure references and links use decision-evidence validation and are
retained with change history in debriefs. The dashboard also exposes the original
basis and attachments on every player decision. Evaluator language translation,
notes and advanced AI assessment are separate enhancements; this dashboard is
strictly read-only and shows English working text plus delivered original wording.

## Within-turn clock, training phases and scheduled injects

The decision-turn lifecycle (`briefing`, `orders`, `resolving`, `debrief`) is
separate from the training/incident `exercise_phase`: `early_warning`,
`early_action`, `escalation`, `full_response`. A training phase may span turns;
one open turn can contain several training phases and many injects. Phase changes
require EXCON's reason and retain the physical world, proposals, messages,
resource commitments/deployments and accounting history. `phase_history` records
the entry time, wall time, turn and reason. Logs include both phase dimensions.
Evaluator filtering additionally accepts `exercise_phase`.

`POST /api/master/clock {"running":true,"ratio":60,"reason":"..."}` runs
physical simulation during an open turn: 60 simulated seconds per wall second.
Ratio is 0.01–86400; pause explicitly with `running:false`. This is distinct from
`turn_minutes` (resolution duration) and `speed` (resolution playback, where 0
means maximum speed). Defaults: paused, ratio 60. `/api/state` and player views
expose `clock_control`; state includes current `wall_ms` and authoritative
simulation time. Large ratios can exceed hardware throughput: `lag_s` reports
catch-up lag and the displayed simulation clock remains authoritative. No
unapproved orders execute as time passes. Physics and timed arrivals continue.
Ratio/pause changes reanchor pacing at the actual current simulation time.

`POST /api/master/phase {"phase":"early_action","reason":"Protect vulnerable groups"}`
changes training phase without advancing a turn or resetting resources. Starting
turn resolution pauses the within-turn clock; rollback and exercise end also
pause it. Resuming is an explicit EXCON action. Direct/approved `AdvanceTime`
jumps reanchor pacing so elapsed wall time is not applied twice. Rollback retains
phase/schedule evidence; delivered schedules are never silently sent again.

`POST /api/master/schedules {"msel_id":"I-02","at_clock":"13:30"}` schedules an
MSEL inject at a Day 1 clock time (`HH:MM` or `HH:MM:SS`). Alternatively use `at_s`
(seconds since scenario start) for later days; choose exactly one. Scheduling
accepts current/future times within seven simulated days. Schedules at 13:00,
13:30 and 14:00 can all occur in the same open turn and training phase. The clock
steps stop at scheduled due instants and claims each delivery once. Translation
runs independently: `due_s`, `triggered_s`/`triggered_wall_ms`, and
`delivered_s`/`delivered_wall_ms` distinguish planned, dispatch and actual delivery
and expose any latency. Message timestamps always use the actual world clock at
delivery. Explicit jumps can make a scheduled inject overdue; the actual late
trigger/delivery is recorded rather than backdating it. Declarations and actions
continue while translations run.

Cancel a pending/dispatching schedule through
`POST /api/master/schedules/{id}/cancel {"reason":"..."}`. Delivery rechecks
cancellation/end before sending, and terminal schedules cannot duplicate. All
schedules and delivery message IDs are retained in the debrief.

`POST /api/master/proposals/{id}/execute` explicitly executes already-approved
orders within the open turn without resolving it. Unapproved/previously executed
proposals fail. Commands are revalidated by the world and actual outcomes are
recorded. Approved orders otherwise execute at turn resolution. The Master UI
provides separate approval, execution, clock, phase and scheduling controls.

## Total and per-training-phase resource limits

`POST /api/master/resource-limits` publishes a reviewed complete configuration:
`{"limits":{"total":{"ground_crews":2},"phases":{"early_warning":{"ground_crews":1},"early_action":{"ground_crews":2}}},"reason":"Progressive tactical capacity"}`.
Keys must be scenario resource IDs. Missing caps mean physical inventory is the
limit; zero explicitly blocks deployment. Caps apply across roles to concurrent
active deployment plus approved-but-unexecuted commitments. Effective capacity is
the minimum of actual exercise-wide inventory, the total cap and the current
training-phase cap. They do not create inventory or reset on a phase transition.
Returns free deployable capacity; physical effects and usage evidence remain.

Approval and actual execution both check total/phase caps and the simulator's
role-specific inventory. Requests alone do not consume capacity. Reductions and
phase transitions that would undercut active/approved units fail without cancelling
an effective action. Inventory adjustments also respect approved reservations.
A snapshot rollback that would restore deployment above current policy fails until
EXCON reviews the limits. Policy changes remain independent of physical rollback
and have before/after, rationale, approver, proposal ID and phase/time evidence.
Master physical interventions remain explicit EXCON simulation interventions;
they do not pretend to allocate operational teams. Resource-using role actions
are the allocations subject to inventory and deployment caps.

Accounting additionally separates `inventory_available`, `effective_capacity`,
`total_limit`, `phase_limit` and actual deployable `available` from requested,
committed, deployed and returned quantities. Current policy appears in Master,
evaluator and debrief views; players see their own deployable accounting.

The Master AI may return a `resource_limits` recommendation (complete configuration).
It stays in `resource_limit_draft` and has no operational effect while pending or
rejected. The Master can edit it using proposal decide body `resource_limits` and
then explicitly approve it. Only Master-origin proposals can recommend policy.
Actual application and the approved configuration are recorded separately from
the original draft; malformed recommendations can always be rejected. Limits-only
proposals require the same approval as physical commands or communications.
