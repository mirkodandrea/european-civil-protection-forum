# 03 — Complete session recording

## Goal
Everything that happens in a session is stored, including the text of every conversation, so the session can be analysed and reported on afterwards, with no need to be there.

## Today
`runs/run-*.jsonl` logs inject, reply, proposal, decision, command and turn events (`Exercise::record`, one JSON line per event, appended immediately). The AI conversations (`Exercise::conversations`) live only in memory, capped at the last 60 entries, and are lost on exit. LLM prompts and raw outputs are not stored (only in the demo cassette). Read times, connections and language changes are not logged.

## Requirements
1. One **session directory** per session, `runs/<session-id>/`, that is never modified by later sessions or by a restart (spec 06).
2. Events recorded, each with wall clock, simulated clock, turn, actor and actor kind (`master | player | bot | evaluator | system`):
   - messages with full text: original, translation, English pivot, languages, status transitions (spec 02), chat threads and forwards (spec 05);
   - every LLM call: task, model, system prompt, messages, raw response, parsed result, latency, token counts, errors, fallbacks used;
   - conversation entries of each player and of the EXCON, complete (not the 60-entry window used for prompts);
   - proposals, decisions, edited commands, validation results, applied actions, master notes;
   - sim commands and the events they caused, turn start/resolve/end, time settings, jumps (spec 04);
   - presence: joins, leaves, language changes, message reads, evaluator notes (spec 08);
   - session lifecycle: start, restart, resume, end (spec 06).
3. Append-only, flushed per event, so a crash loses at most the last event.
4. A **manifest** with the session id, start/end time, scenario and game file hashes, git commit, models used, roles and languages, seed, ensemble settings.
5. Export bundle for analysis: `GET /api/export` (zip) with `manifest.json`, `events.jsonl`, `conversations.jsonl`, `llm_calls.jsonl`, flat CSVs (`messages.csv`, `proposals.csv`, `response_times.csv`) and the turn snapshots.
6. Secrets are never recorded (API key, tokens). Participant data are pseudonymous by role; the join screen states that the session is recorded (wording to be agreed for the pilot).

## Design
- Keep the JSONL format and `Exercise::record`. Add `kind` values in a documented namespace (`message.*`, `llm.call`, `conversation.entry`, `presence.*`, `session.*`, …) in `docs/api.md`. Add a `schema` version to the manifest, and to each line when the format changes.
- `llm.call` events are written from `Llm` through a hook (as the cassette is), so no task can forget to log. Large prompts are stored in `llm_calls.jsonl`, and the main log holds a reference (call id) to keep it readable.
- The in-memory conversation cap stays for prompting; storage is unbounded.
- Replay for analysis: `scripts/analyse/` reads a session directory and produces the per-turn timeline (the demo's `readme_demo.py` is the starting point).

## Tests
- Headless: a scripted session leaves a session directory in which every message and every LLM call can be found by id; the export bundle opens and the CSV row counts match the log.
- A crash test: kill after N events, the log parses and has N lines.
- The API key never appears in any file of the directory (grep in the test).

## Depends on / used by
Used by 05 (thread log), 06 (resume), 08, 09. No dependencies.

## Open questions
- Consent and retention for the pilot: who owns the data, how long is it kept, is anonymisation needed before sharing?
- Are the LLM prompts, which contain scenario data, allowed in the export bundle that evaluators receive, or only the conversation text?
