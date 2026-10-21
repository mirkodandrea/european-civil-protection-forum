# 09 — AI players, and AI assessment of the exercise

Two related parts. Both are opt-in and never replace the EXCON's approval. (See the assumption in `README.md`.)

## Part A — AI players (bots)

### Goal
A role can be played by an AI, so an exercise can run with fewer humans, be rehearsed alone, or be tested, with bots behaving like participants.

### Today
Every role is a human in a browser. The LLM only translates and interprets what humans write.

### Requirements
1. A role can be configured as a **bot** in the game or scenario (`roles.yaml`): `bot: {persona, competence: novice|competent|expert, language, response_delay_s, initiative}`. The EXCON can turn a bot on or off during play, and take over the role by connecting a human.
2. A bot uses the **same interfaces** as a human: it receives injects and messages, writes replies in its language, chats, and forwards injects (spec 05). Its replies go through the normal path — translation, interpretation into a proposal, EXCON approval, validation — so the guardrails are unchanged. Nothing gives a bot more power.
3. A bot sees only what its role sees (role digest, its inbox, its own AI conversation). It is not told the objectives, expected responses or other roles' data.
4. Behaviour is bounded: a response delay (real seconds, drawn around `response_delay_s`, to look like a person and to save cost), a limit on messages per turn, and imperfection by competence level (a `novice` may ask for clarification, or omit a step).
5. Bots are **labelled** as bots for the EXCON and evaluators everywhere (dashboard, log: `actor_kind: bot`). Whether other players are told is a per-session setting, default yes for the pilot.
6. Failure: if the LLM is unavailable a bot stays silent, and the EXCON is told, so nobody waits for it without knowing.

### Design
- New workflow **L9 `bot_turn`**: system = persona, role manifest, allowed actions, language; messages = the role's conversation and current digest; output = `{reply?, chat: [{to, text}], forward: [{message, to, note}], wait_s}`. The output is applied through the same server functions a human's request uses (`player_reply`, `chat`, `forward`), so validation and logging come for free.
- A scheduler in the server runs a bot when something new arrives for it (inject, message, turn opened) after its delay. It re-locks the exercise only to read and to store, never across the LLM call.
- The bot uses the general model, not the translation model. Cassette replay makes bot sessions reproducible.

### Tests
- A scripted session with two bots and one human completes a turn; every bot action appears as a proposal needing approval; a bot cannot use a command outside its manifest.
- Delay and message limits are respected (mock clock). LLM off → silent bot and a EXCON warning.
- Replay from the cassette gives an identical log.

## Part B — AI assessment of the exercise (including context)

### Goal
The evaluators get an AI-drafted reading of how the exercise is going and how each player responded to the objectives, in context, as a suggestion they can accept or discard.

### Implemented issue #17 scope
On-demand English evaluator analysis is available in `evaluator.html` and
`GET /api/evaluator/assessment`. It combines recorded timings, communication chains,
objectives, execution outcomes and cited decision evidence. Strict schema and
citation validation reject unsupported output. A bounded snapshot with explicit
omissions keeps the original evidence accessible; failed/disabled AI falls back to
that snapshot. Analysis never mutates the exercise or sends operational messages.
Reports are optional derived interpretations, not scores. Reference document
contents are not fetched. See `docs/api.md` for bounds and caching.

The broader requirements below retain future evaluator localisation, saved
accept/edit/dismiss annotations, staged long-session summaries and turn-triggered
analysis. Bot players in Part A remain separate.

### Requirements
1. Workflow **L10 `assess`**, run on request (button) and optionally at the end of each turn. Input: the game objectives and expected responses (spec 07), the turn's injects, the players' replies and decisions, the timings (spec 08 metrics), the simulation context (what the fire did, and what was at risk when a decision was made), and the chat and forwards.
2. Output, in the evaluator's language: per objective, `met | partly | not met | not observed`, with **evidence** (links to log events, so the evaluator can check), per-player observations, notable delays, missed opportunities, and questions for the debrief. It replaces and extends L7 (debrief narrative).
3. Marked as AI-drafted, never shown to players, never counted as a score. An evaluator can accept, edit or dismiss each item, and the choice is stored (spec 03).
4. Context handling: long sessions are summarised in stages (turn summaries, then a session summary), so prompts stay bounded, and every claim keeps event ids.
5. Failure: assessment is optional; the dashboard and the export work without it.

### Tests
- Mock model: the output schema is validated; every item has at least one valid event id; items with unknown ids are rejected.
- Staged summarisation stays within the prompt budget on the demo log.
- Live test on the recorded demo session (`TTX_LIVE_LLM=1`), reviewed by a human once.

## Depends on / used by
Depends on 03, 07 (both parts); Part B also on 08. Used by nothing.

## Open questions
- Do bots pretend to be human, or is disclosure to players required? (Proposed: disclosed.)
- Which language do the bots speak with each other when both are AI?
- What does "evaluation" have to produce for the Forum: a score, or observations only? (Proposed: observations only.)
- The pilot's need for bots: rehearsal, filling absent roles, or a demonstration?
