# 04 — Time compression per turn, and time jumps

## Goal
The pace of simulated time is set in advance and can be changed for each turn, including long jumps ("six hours later") between phases of the incident.

## Today
`POST /api/master/settings {turn_minutes, speed}` sets a global turn length (default 30) and global speed (simulated seconds per real second, 0 = maximum). `resolve {minutes}` overrides the length for one resolution. `AdvanceTime` runs the model forward. Issue #11 adds a separate within-turn clock (`ratio`, running/pause controls), explicit training phases, scheduled MSEL delivery timestamps and phase/clock history. Player views disclose current phase and compression. The longer-term turn-plan/jump/checkpoint requirements below remain separate from this delivered clock workflow.

## Vocabulary
- **Turn length** Δt: simulated time covered by one resolution.
- **Speed** (compression): simulated seconds per real second while a turn is watched. Affects only the pace on screen, never the result (the model is step-size invariant).
- **Time jump**: a Δt much longer than usual (hours or days), where the intermediate minutes are not watched.

## Requirements
1. Defaults live in the game file (spec 07) as `default_turn_minutes` and `default_speed`, and each turn plan may override `minutes`, `speed` and `jump`.
2. The Master can change all of these for the next resolution from the console, before pressing resolve, and the change is logged with the reason (free text, optional).
3. A jump has a mode:
   - `simulate`: run the model forward at maximum speed (as `AdvanceTime` does now). The big screen shows a "+6 h" banner and the ticker summarises what happened; the camera follows the fire at the end.
   - `checkpoint`: load a prepared state (spec 06 snapshot), for large skips such as Day 3 → Day 10.
4. During a jump, no player actions are applied silently. Approved actions apply at the start, as for any turn, and the Master is warned if a long jump is chosen while proposals are pending.
5. Players see the clock and the elapsed time in the situation update ("Since your last turn: 6 h 00"). The situation update (L5) is written for the whole jump, not per minute.
6. Limits: a jump above a configured maximum (default 24 simulated hours) asks for confirmation, because of the compute time (resolving 1 h must take ≤ 10 s, spec §9).

## Design
- `TurnTiming { minutes, speed, jump: Option<Jump> }` in the turn plan and in the `resolve` request; `Exercise::begin_resolution` takes it instead of separate arguments. The resolved values are stored in the `TurnReport` and in the log event `turn.resolve`.
- The console shows a small "time" control next to Resolve: minutes (presets 15 m, 30 m, 1 h, 6 h, 1 day), speed (presets 1×, 60×, 600×, max), and the effective wall-clock estimate ("about 4 s").
- No change to the model, only to how Δt and speed reach it.

## Tests
- The fire result of a 6 h jump at speed 0 equals six 1 h turns (extends the existing step-size test).
- The settings from a turn plan are applied, and a Master override wins and is logged.
- The `checkpoint` mode restores the given snapshot and the clock jumps.

## Depends on / used by
Used by 07. No dependencies. Uses the snapshots of 06 for `checkpoint`.

## Open questions
- Do players see the compression factor, or only the elapsed time?
- Is `checkpoint` needed for the pilot, or is `simulate` enough?
