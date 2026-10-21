# 06 — Restarting and resuming a session

## Goal
A session can be restarted without losing what happened, and can be resumed after the process dies mid-exercise.

## Today
Rollback to a per-turn snapshot exists (`Exercise::rollback`, `POST /api/master/rollback`), but snapshots are in memory. Restarting the `ttx` process starts from zero, and players and EXCON reload empty pages. The spec (§4) lists `Reset`, `SaveCheckpoint` and `LoadCheckpoint`, and none is exposed.

## Three cases
| Case | Meaning | Session id |
|---|---|---|
| **Resume** | The process crashed or was stopped. Continue exactly where it was | same |
| **Restart from turn n** | EXCON goes back to a turn start (rollback), and plays forward again, on purpose | same, with a branch marker |
| **Restart from scratch** | Same scenario and game, new run, same roles and languages | new (the old one is archived, never deleted) |

## Requirements
1. **Autosave.** The complete exercise state is written to the session directory (spec 03) at every turn boundary and after every state change, debounced (default 5 s). State = world snapshot, messages and threads, proposals, conversations, players and languages, settings, id counters, current turn plan position.
2. **Resume.** `ttx --resume <session-id|latest>` loads the last autosave and continues. Browsers reconnect by themselves (the WebSocket already reconnects) and show the restored state. A resumed session logs `session.resume` with the gap in wall-clock time.
3. **Restart from turn n.** The console offers "Restart from turn n" using the snapshots on disk (not only memory), with a confirmation. The events after turn n stay in the log, marked `branch: abandoned`, and the new events carry a `branch` counter, so an analysis can tell them apart.
4. **Restart from scratch.** One control in the EXCON console with a typed confirmation. Steps: archive the current session (mark ended, `reason: restart`), create a new session id, reload the scenario and game, keep players' roles and languages (option: also keep the AI conversations, default no). Connected pages reload by themselves.
5. **During resolution.** A restart aborts the running resolution cleanly (no half-applied turn), because the state is restored from a snapshot.
6. The big screen follows: it clears the effects (fire, smoke) and re-establishes the scene.
7. Restart and resume never delete recordings (spec 03).

## Design
- Snapshot format: JSON (serde) of `Exercise` minus derived caches. The world snapshot already exists for rollback and should be serialisable. The propagator ensemble is stored by its seed and the checkpoint the core provides (`propagator-core::checkpoint`), otherwise by replaying the applied commands from the last turn snapshot, which is exact because the model is seeded and step-size invariant.
- Files: `runs/<session-id>/state.json` (latest), `runs/<session-id>/snapshots/turn-<n>.json`, written to a temp file and renamed, so a crash cannot leave a half-written state.
- API: `POST /api/master/restart {mode: "scratch"|"turn", turn?, keep_conversations?}`, `GET /api/sessions`, and the CLI flag above. The EXCON console and `play.html` handle a `session` field in `/api/state` and reload when it changes.

## Tests
- Headless: run three turns, save, load into a fresh `Exercise`, and compare state (messages, proposals, world digest) and the next resolution result.
- Kill test: stop the process during turn 2, resume, and finish; the fire result equals an uninterrupted run with the same seed.
- Restart from scratch produces a new session directory and leaves the old one unchanged.
- Restart during resolution leaves no partial state.

## Depends on / used by
Depends on 03. Used by 04 (`checkpoint` jumps) and 07 (starting a game from a prepared turn).

## Open questions
- Do players see a "the exercise is being restarted" message with a reason, or just reconnect?
- Is authentication needed before restart is exposed? Proposed: the EXCON console only, plus the confirmation, until spec §12.4's authentication exists.
