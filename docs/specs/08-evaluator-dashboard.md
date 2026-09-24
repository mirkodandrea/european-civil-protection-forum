# 08 — Live evaluator dashboard, in the evaluator's language

## Goal
Evaluators follow the exercise live from their own devices, in their own language, and see the context, the response times and the actions of every player, without asking anyone and without influencing the play.

## Today
No evaluator interface. The Master console shows everything, in English, mixed with controls. The debrief JSON has the log after the fact. Spec §8 lists an "Observer/Evaluator UI (P2)".

## Requirements
1. **Evaluator role**: read-only, a token like the players', with its own language, chosen at join and changeable. It cannot change anything (server refuses every POST except notes and language).
2. **Context panel**: clock, turn, phase, weather, fire summary (zones burning, area, buildings and people at risk), current turn's title, objectives and planned injects (spec 07), what is pending with the Master.
3. **Role cards**, one per player: the last inject received and when; **time since delivery** (a running timer until answered); status of the last reply (interpreting, waiting, decided); counts of replies, proposals approved/edited/rejected, clarifications, chat messages, forwards; last action.
4. **Live timeline**, filterable by role, kind and turn, showing each event with both clocks (wall and simulated) and, for messages, the text in the evaluator's language with a toggle to the original.
5. **Chains**: inject → (forward) → reply → proposal → decision → consequence (turn report). Clicking a card opens its chain.
6. **Metrics** per player and per turn, computed from the log (spec 03), defined exactly:
   - `T_deliver` = delivered − sent (the translation delay, spec 02);
   - `T_read` = read − delivered;
   - `T_reply` = reply sent − delivered;
   - `T_decision` = Master decision − reply sent (the Master's queue time);
   - both wall and simulated clocks, and against the turn's `response_deadline_s` when set.
7. **Objectives view**: for each objective, the injects meant to test it, the expected response, and what actually happened, with an evaluator tick or note.
8. **Notes**: an evaluator writes timestamped notes, optionally tied to a role, event or objective. They are stored in the log (spec 03) and included in the export. They are visible only to evaluators and the Master.
9. **Language**: everything the evaluator reads that came from players or the Master (messages, replies, proposals, feedback, chat) is translated on demand to their language, cached per (event, language), never blocking the dashboard. The interface texts are localised (English and Italian first).
10. **Chat visibility**: evaluators see all threads. The join screen tells players (spec 05).
11. Works on a laptop and a tablet; updates within 1 s of the event (spec §9), 3 evaluators at once.

## Design
- Page `evaluator.html` in `web/`, plain HTML/JS like the others. API: `GET /api/evaluator` (context, role cards, metrics, timeline page), `POST /api/evaluator/note`, `POST /api/evaluator/language`, `GET /api/evaluator/translate?event=&lang=` (cached).
- Metrics are computed in a small pure module `exercise::metrics` from log entries, so the same code serves the dashboard, the CSV export and the tests.
- Event push: the typed WebSocket events from spec 02, plus `event: "log"`.

## Failure
Translation unavailable → show the original with a marker. The dashboard reads state only, so an AI failure never affects it.

## Tests
- `metrics` unit tests on a synthetic log (known times → known values, including unanswered and re-answered injects).
- Server: an evaluator token cannot call any mutating endpoint except notes and language.
- Translation cache: one call per (event, language) with several evaluators.
- Browser recording: an evaluator page added to the recorded session.

## Depends on / used by
Depends on 01, 02, 03 (and 05, 07 for threads, objectives). Extended by 09 (AI assessment).

## Open questions
- For the pilot, is a read-only view of the exported log enough, with the dashboard following?
- Which metrics does the PPRD methodology require exactly? Adjust the definitions above to the evaluation grid.
- Do evaluators need to see the Master's private notes and AI prompts, or only player-side events?
