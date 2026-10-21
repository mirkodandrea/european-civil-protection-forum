# The demo: what happens in the video

The narrated demo ([`media/ttx_demo.mp4`](media/ttx_demo.mp4), 4 min 18 s) plays a complete two-turn wildfire exercise. This page describes the incident, the roles, a timeline of the video, what the players wrote and what the AI made of it, and the outcome. The timeline and outcome tables are generated from the recording by `scripts/record/readme_demo.py`.

A complete two-turn exercise, played for real: every reply was typed into the UI; every translation, interpretation, feedback and situation update came from `deepseek/deepseek-v4.1-flash` (recorded once, replayed from [`scripts/record/llm_cassette.jsonl`](../scripts/record/llm_cassette.jsonl) so re-recordings are identical); every consequence was computed by the simulator (32-run Monte Carlo fire, turns simulated live at 120×). Nothing is mocked or pre-scripted except the players' words.

## What you see

| Left: the phone (480 × 1040) | Right: the big screen (Bevy 3D) |
|---|---|
| `play.html`: the EXCON and the three players as tabs in one window. Each player's tab opens on the situation map, with the inbox and the reply box below. A yellow dot marks a tab with new messages or open proposals | The simulator with the **cinematic camera**: it cuts to the place each inject, proposal and order refers to (a slow push-in), follows the fire while a turn is simulated, and holds a wide establishing orbit in between. Low late-afternoon sun with long shadows, fog, bloom, fire light on the terrain, and a smoke plume carried downwind (the wind is faster higher up, so the column bends over). Top left: turn, clock, weather, the fire model, key numbers, a progress bar while a turn is simulated. Top right: the exercise feed. During each resolution the turn is **simulated live**: flame tongues sized by fireline intensity and by the share of runs that burn each cell, leaning with the wind; a faint glow ahead where only some runs reach; smoke drifting downwind; the ground cooling from glowing to charcoal. Under it: the subtitles |

## The situation

The synthetic **Valverde valley** (4 × 4 km, 8 × 8 grid of 500 m zones, see [docs/scenario.md](scenario.md)). **Day 1, 13:00**: a wildfire of about 5 ha burns on the south-west flank of Pine Hill (zone G3), pushed by a 45 km/h north-east wind at 4 % fuel moisture towards the village of **Pineta** (F4). Pineta's primary school holds **120 pupils and staff**. Left alone, the fire reaches Pineta's edge in about an hour.

| Role | Played in | Resources per turn |
|---|---|---|
| EXCON | English | Omnipotent: injects, AI prompts, commands by code, approvals, turn speed |
| Mayor of Valverde | Italian | 2 buses, 2 police patrols |
| Fire and Rescue Commander | English | 3 ground crews (600 m of fire line each), 1 air tanker |
| Regional Civil Protection | Armenian | None: coordinates and requests reinforcements |

## Timeline

Generated from the recording by `scripts/record/readme_demo.py`: each row is a narration line, at the moment it starts in the video.

<!-- demo:timeline -->
| Video | Who | What happens (narration) | What it shows |
|---|---|---|---|
| 0:02 | — | Valverde valley, one in the afternoon. A wildfire has broken out on Pine Hill, and a strong north-east wind is pushing it towards the village of Pineta. | The situation: one device plays every role |
| 0:12 | Regional | Four people run the response: the exercise director, the mayor of Valverde, the fire commander, and a regional civil protection officer who works in Armenian. | Each player works in their own language |
| 0:23 | EXCON | The exercise begins. The fire already covers five hectares and is spreading south-west through dry pine forest. | Turn-based control; turn speed set to 120× |
| 0:32 | EXCON | The first reports come in: emergency calls about the fire, a warning that the primary school is in session, and a fire-behaviour estimate for the commander. | Injects from the exercise script, translated for each player |
| 0:42 | Mayor | In Pineta, 120 pupils and staff are still inside the primary school, and the school has no transport of its own. | Role-specific information |
| 0:51 | Mayor | The mayor acts at once: warn the village, evacuate the school to the sports centre in Valverde, and close the northern road. | Free-text orders, no command language |
| 1:00 | Fire | The fire commander sends ground crews to cut fire breaks north and east of the village, and an air tanker to wet the forest above it. | Orders by map zone and place name |
| 1:10 | EXCON | Before anything happens on the ground, every order is checked against the people, vehicles and authority actually available. | The AI interprets; the rules decide what is possible |
| 1:18 | EXCON | A problem emerges: evacuating the school needs 3 buses, and the town has only 2. | A real constraint the AI cannot wave away |
| 1:25 | EXCON | EXCON steps in: the regional transport company can release one more bus. | The EXCON steers the scenario in plain language |
| 1:31 | EXCON | The extra bus is confirmed, and the mayor is told it is on its way. | Nothing changes until the EXCON approves |
| 1:37 | EXCON | The fire commander's plan goes ahead: two fire breaks and a water drop. | Approval, with a note to the player |
| 1:42 | EXCON | With three buses available, the school evacuation can start, together with a warning to the whole village. | Feasibility re-checked at the moment of decision |
| 1:49 | Mayor | The mayor receives the confirmation, and the buses head for the school. | Feedback in the player's language |
| 1:56 | EXCON | The next thirty minutes unfold. The fire runs downhill towards Pineta while the crews race to finish the breaks. | The turn is simulated live |
| 2:04 | — | The tallest flames mark where the fire burns hottest and where most simulated runs agree. The faint glow ahead shows where it could spread next. | Flames from fire intensity and the Monte Carlo probability |
| 2:16 | — | The fire breaks hold at the edge of the village, and the children reach the sports centre safely. The northern road is lost to the flames. | Consequences of the decisions |
| 2:25 | Fire | Each responder receives a report on what has changed on the ground. | Situation updates for every role |
| 2:32 | EXCON | Half past one. The wind strengthens to 55 kilometres per hour, and a second fire starts in the farmland south-east of Pineta. | Escalation by the EXCON |
| 2:42 | Regional | Local resources are stretched. Regional civil protection asks for international help: two water-bombing aircraft through the EU Civil Protection Mechanism, and two more ground crews. | International assistance (UCPM) |
| 2:55 | Fire | Under pressure, the fire commander sends an unclear order… | — |
| 3:00 | Fire | …and is asked exactly where the crews should go, instead of anyone guessing. | Ambiguity is clarified, never guessed |
| 3:05 | Fire | The answer comes back: the eastern edge of the village, and the slope above it. | The follow-up is read in context |
| 3:11 | EXCON | The request for aircraft and the new fire breaks are approved; the unclear order is sent back. | Every action passes through the EXCON |
| 3:26 | EXCON | The next hour. Reinforcements arrive and the breaks keep the fire out of Pineta, but the second fire races through the farmland, where nobody was warned. | The escalation, simulated live |
| 3:59 | — | By half past two, Pineta is safe and every child is accounted for, but farmhouses have burned and the aircraft are still on their way. Every decision and its consequences are recorded for the debrief. | Outcome and debrief |
<!-- /demo:timeline -->

## What the players wrote and what the AI made of it

<details>
<summary><b>Mayor (Italian)</b>: warn F4, evacuate the school, open a shelter, close the road</summary>

> *Allertate la zona F4 ed evacuate la scuola primaria di Pineta verso il centro sportivo di Valverde, da aprire come area di accoglienza. Chiudete la Northern Road al traffico civile. Avvisate i vigili del fuoco.*

- **AI translation**: "Warn zone F4 and evacuate the Pineta Primary School towards the Valverde Sports Centre, to be opened as a reception area. Close the Northern Road to civilian traffic. Notify the fire brigade."
- **Commands**: `IssueWarning F4` ✓ · `EvacuateBuilding Pineta Primary School → Valverde Sports Centre` ✗ *needs 3 buses, only 2 available* · `OpenShelter Valverde Sports Centre` ✓ · `CloseRoad north_road` ✓
- **"Notify the fire brigade"** became a message to the Fire Commander (delivered at once), not a command.
- **Feedback after approval** (Italian): *"Sindaco, l'avviso alla zona F4 è approvato e sarà eseguito alla fine del turno. L'evacuazione della Scuola Primaria Pineta verso il Centro Sportivo Valverde (3 bus per 120 persone) è approvata…"*
</details>

<details>
<summary><b>Fire Commander (English)</b>: fire lines by zone code, a drop</summary>

> *Two crews cut a fire line along the northern edge of zone F4; the third crew from Pineta east edge to Forest edge east. Air tanker: one drop on Pineta north edge. Tell regional civil protection we may need reinforcements.*

- **Commands**: `CutFireLine along F4:N` ✓ *500 m, 1 crew* · `CutFireLine Pineta east edge → Forest edge east` ✓ *447 m, 1 crew* · `WaterDrop Pineta north edge` ✓
- **"Tell regional civil protection…"** became a message to Regional, which received it in Armenian.
</details>

<details>
<summary><b>EXCON (AI prompt)</b>: one extra bus</summary>

> *The regional transport company gives the mayor one extra bus right now. Send the mayor a short inject from the Regional transport company confirming it.*

- **Proposal**: `AdjustResource mayor.buses +1` plus the inject draft *"Extra bus available immediately"*. After approval the bus was added and the inject was translated and sent to the Mayor in Italian.
</details>

<details>
<summary><b>Regional Civil Protection (Armenian)</b>: international assistance</summary>

> *Խնդրում ենք միջազգային օգնությամբ (UCPM) 2 օդային տանկեր և տարածաշրջանային հրշեջ ծառայությունից 2 վերգետնյա թիմ հրշեջ հրամանատարի համար։ Տեղեկացրեք քաղաքապետին, որ Valverde Sports Centre-ը բաց է։*

- **AI translation**: "Please request, with international assistance (UCPM), 2 air tankers and from the regional fire service 2 ground crews for the fire commander. Inform the mayor that the Valverde Sports Centre is open."
- **Commands**: `RequestReinforcement air_tankers ×2` ✓ *arrive in 120 min from International assistance (UCPM / rescEU)* · `RequestReinforcement ground_crews ×2` ✓ *arrive in 60 min from the Regional fire service*
- **Message**: the Mayor was told, in Italian, that the shelter is open.
</details>

<details>
<summary><b>Fire Commander</b>: a vague order, then a precise one</summary>

> *Send the remaining crews over there to hold it.*

- **No command.** AI summary: *"wants remaining ground crews sent to hold a line, but neither the number of crews nor the location is specified."*
- **Clarification sent to the player**: *"Which crews do you mean (how many), and where exactly should they go — hold the F4:N fire line, or another line?"* The question refers back to the fire line ordered earlier: the AI keeps each player's whole conversation.
- The EXCON rejected it with the note *"Please specify the zone edge."*

> *One crew along the east edge of zone F4, the other along the north edge of G4. Air tanker drop on G4.* (read as the answer to the question above)

- **Commands**: `CutFireLine along F4:E` ✓ · `CutFireLine along G4:N` ✓ · `WaterDrop G4` ✓
</details>

## Outcome

<!-- demo:outcome -->
| | After turn 1 (13:30) | After turn 2 (14:30) |
|---|---|---|
| Burned area | 7.8 ha | 39.1 ha |
| Fire lines / water drops | 0.95 km / 1 | 1.95 km / 2 |
| People evacuated / sheltered | 120 / 120 | 120 / 120 |
| Buildings threatened | 4 | 51 |
| Buildings damaged / destroyed | 0 / 0 | 2 / 7 |
| People at risk | 0 | 30 |
| Roads cut by fire | Northern Road | Northern Road, SP1 Main Road |
| Reinforcements arrived | — | 2 ground_crews from Regional fire service arrived for fire_commander |
<!-- /demo:outcome -->

**Debrief reading.** The fire lines on the F4 north and east edges and the G4 north edge held the head of the fire above Pineta. Not a single Pineta building was damaged, and the school was empty 20 minutes after the order. All 9 damaged or destroyed buildings were **farmhouses in G5, G6, F5 and F6**, around the second fire the EXCON started in turn 2. Nobody warned or evacuated those zones, so their 30 occupants count as *people at risk*. That is exactly the kind of gap a debrief should surface: the log shows when the ignition happened, what each role knew, and that no warning followed. The UCPM air tankers, requested at 13:30 with a 120-minute delay, were still en route when the turn ended. The full event log of this run is in [`docs/media/ttx_demo_log.jsonl`](media/ttx_demo_log.jsonl).

## What the demo demonstrates

| Principle (use case §43) | Where in the video |
|---|---|
| AI reduces EXCON's information-management workload | Translations, proposals, feedback and SITREPs are all generated; the EXCON only decides |
| AI supports participants without replacing them | Players write freely in their own language; the AI only interprets, and asks when unsure (3:36) |
| Operational models remain authoritative for physics | The fire comes from a 32-run PROPAGATOR ensemble; lines and drops act on its fuel and moisture; flame length is Byram's from the model's intensity |
| Operational rules remain authoritative for feasibility | The bus shortage is caught (1:19) and the AI cannot override it |
| The EXCON remains in control | Every player action is approved or rejected; the AI prompt becomes a proposal too (1:33) |
| Evaluation becomes easier to reconstruct | The event log (`/api/debrief`) holds every inject, reply, interpretation, decision and consequence |
| The architecture is reusable | Roles, resources, languages and injects all come from the scenario file |

## Run the demo yourself

| Mode | Command | Notes |
|---|---|---|
| **Play it** (any device) | `cargo run --release -p app`, then open `http://<host>:8080/play.html` | Follow the timeline above; the MSEL has the same injects |
| **Play it from a phone** | Add `devtunnel host -p 8080` | See [Playing from a phone](../README.md#playing-from-a-phone) |
| **Scripted, API only** | `./scripts/demo_session.sh` against a running `ttx` | The same session through HTTP calls, plus simulator screenshots in `docs/screenshots/` |
| **Scripted and recorded** | `./scripts/record/record.sh` | Re-records the narrated `docs/media/ttx_demo.mp4` (plus subtitles, captions and the run's event log) in about the video's own length plus ~40 s (LLM answers and narration replayed from caches), then `python3 scripts/record/readme_demo.py` refreshes the timeline and outcome tables above. Details in [scripts/record/README.md](../scripts/record/README.md) |

The simulator is deterministic for a given seed, but **the LLM is not**: wording, summaries and even the split into commands differ from run to run. The recording therefore replays the model's answers from a cassette (use `TTX_REC_FRESH=1` to ask the model again), and the scripted session waits on game state (for example "no proposal still parsing") instead of fixed delays. Timestamped captions for the published video are in [`docs/media/ttx_demo_captions.json`](media/ttx_demo_captions.json).
