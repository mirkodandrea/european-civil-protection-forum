# 07 — Turn authoring: the Master prepares the game in advance

## Goal
The Master builds the whole game ahead of time — turns, injects, time settings — in an editor, optionally with AI help, and runs it during the session, still able to improvise.

## Today
The scenario JSON holds the geography and a fixed **MSEL** of 8 injects. The Master picks injects by hand during play. A game is not a separate object, turns have no plan (length, speed, objectives), and creating anything means editing JSON or Python.

## Scope
- **Game editor** (this spec's core): turn plans and injects.
- **Scenario parameters** (smaller): ignition point and time, weather, roles and resources, languages, via forms over the generator's parameters. Baking a *real* map stays with the `propagator_abm` pipeline and is out of scope.

## Model
A **game** is a file next to the scenario: `data/games/<id>.json` (YAML accepted), with a schema version.

```
Game { id, title, scenario, languages, objectives: [Objective{id, text}],
       defaults{turn_minutes, speed}, turns: [TurnPlan] }
TurnPlan { id, title, objective_ids, minutes?, speed?, jump?,
           at_start: [Command],            // Master-side world changes (weather, ignition)
           injects: [InjectPlan],
           response_deadline_s?, master_notes }
InjectPlan { id, sender, to: [role]|"all", title, text (source language),
             translations?: {lang: text},  // optional pre-translation, checked by the Master
             release: auto|manual|offset_s, forwardable, objective_ids,
             expected_response, tags }
```

`expected_response` and `objective_ids` mirror the Armenian MSEL fields and feed the evaluator dashboard (08) and the AI assessment (09). The current 8-inject MSEL is migrated to a game file.

## Requirements
1. **Runtime.** Opening a turn applies `at_start` commands, queues the injects, and sends those with `release: auto` (translated per recipient as now). `manual` injects wait in a "planned" list for one click. `offset_s` releases at a simulated offset during the turn. The Master can skip, edit, reorder or add injects at any time. Improvised injects are logged as `planned: false`.
2. **Editor** (`author.html`, Master only): turn list with drag-and-drop order; inject composer; role and language pickers; timing controls of spec 04; objectives list; import/export of JSON/YAML.
3. **Validation** before saving and before starting: roles exist; gazetteer names and zone codes in commands resolve; `at_start` commands pass `validate()` against a scratch world; injects have text; the total of the jumps is sane.
4. **Preview.** Per language: the inject as each recipient would read it (translation by spec 01). **Dry-run**: simulate the plan forward on a copy of the world at maximum speed, and show the fire and stats at the end of each turn, without touching the live world.
5. **AI generation** (workflow L8): the Master gives a brief (theme, objectives, roles, number of turns, difficulty); the model proposes a draft game as JSON, validated against the schema and the gazetteer, and opened in the editor as a draft. Nothing is used without the Master saving it. Invalid drafts are shown with the errors, not silently repaired. Fallback: the editor works with no AI.
6. **Versioning.** Saving keeps the previous version; a session records the game file hash in its manifest (spec 03).

## Design
- New crate module `exercise::game` (Bevy- and HTTP-free) with the types, `Game::validate(&World)` and the runtime cursor (`current_turn`, `pending_injects`).
- API: `GET/PUT /api/games/{id}`, `POST /api/games/{id}/validate`, `POST /api/games/{id}/dryrun`, `POST /api/games/generate {brief}`; runtime `POST /api/master/game/{id}/load`, `POST /api/master/inject/{plan-id}/release`.
- L8 prompt: system = catalogue + gazetteer + schema; output = the `Game` JSON. Uses the general model, not the translation model.

## Tests
- Headless: schema round-trip; validation errors for unknown role, place and command; runtime releases injects by mode; improvised injects are marked; the migrated MSEL plays like the current one (regression on the demo cassette).
- Dry-run does not change the live world (state hash before and after).
- L8 with a mock: a valid draft loads, an invalid one reports each error.

## Depends on / used by
Depends on 04 (timing fields). Used by 08 (objectives, expected responses) and 09 (bot behaviour and assessment).

## Open questions
- Conditional turns (branching on what the players did)? Out of scope now: a linear list plus Master improvisation.
- Who authors for the pilot? If the Master edits the JSON by hand, the editor can wait.
