# Gaps: what we have vs. the v0.3 specs

Checked against the code on 2026-09-24 (commit `2255bcd`). Each row says what exists, what is missing, and where the change goes. Effort: S = days, M = about a week, L = more. The specs are in [`specs/`](specs/README.md); the wider to-do list is in [`todo.md`](todo.md).

## Per spec

### 01 Translation model
| Have | Gap | Where |
|---|---|---|
| One `Llm` client, one model (`OPENROUTER_MODEL`, default `deepseek/deepseek-v4.1-flash`); `translate` task in `crates/llm/src/tasks.rs`; English targets skip the call | No `OPENROUTER_TRANSLATION_MODEL`; no second client or fallback chain (translation model → general model → English); no masking of zone codes and names; no translation cache; source language is always English (no X → Y) | `llm/src/lib.rs` (`Backend::OpenRouter` holds one model), `tasks.rs::translate` |
| `scripts/bench_translation.py`, benchmark table in the spec | The benchmark is 10 short phrases and one LLM judge; no long injects, no human review of hy/ka/az; the choice (`google/gemini-3.1-flash-lite`) is not confirmed | script, spec 01 |

### 02 Message lifecycle
| Have | Gap | Where |
|---|---|---|
| `Message` created in one step, after translation is done (`Exercise::add_message`); replies get a `Proposal` immediately and are parsed in a spawned task (`flow::process_reply`) | No message `status` or timestamps (sent, translated, delivered, read); no placeholder while translating; no read receipts; no ordering guarantee if translations finish out of order; no timeout on translation (only the 90 s HTTP timeout) | `exercise/src/lib.rs` (`Message`, `inbox`), `server/src/flow.rs` |
| `/ws` sends only `{"generation": n}` and pages refetch everything | No typed events (`message`, `status`); the pages (`web/common.js`) have no per-message chip | `server/src/lib.rs::ws_loop`, `web/*` |

### 03 Session recording
| Have | Gap | Where |
|---|---|---|
| Append-only JSONL, one file per process start (`runs/run-<stamp>.jsonl`), 15 event kinds (`exercise_created`, `message`, `proposal_parsed`, `turn_resolved`, …) | No session directory, id, manifest or schema version; no config/models/git commit recorded | `app/src/main.rs:78`, `Exercise::record` |
| `message` events carry `text` and `text_en` | No message status transitions, languages or source text; no read or presence events; language changes only as `language_set` |  |
| Conversations in memory only, capped at `CONVERSATION_MAX = 60` | Not persisted; the cap also truncates what would be recorded | `Exercise::converse` |
| LLM calls: `tracing` lines (task, ms, size) and the demo cassette | No record of prompts, raw answers, model, tokens or fallbacks in the session log | `llm/src/lib.rs` (needs a hook) |
| `/api/debrief` returns the log as JSON | No export bundle, CSVs, response-time table; API key redaction is not tested | `server/src/lib.rs::get_debrief` |

### 04 Time compression
| Have | Gap | Where |
|---|---|---|
| Global `turn_minutes` and `speed` (`/api/master/settings`); `resolve {minutes}`; live 1-second steps; clamp 1 min – 24 h per resolution | No per-turn plan or stored defaults; `speed` is not per resolution; no time-jump mode or banner; no `checkpoint` jump; players are not told elapsed time or compression; no confirmation for long jumps; settings changes carry no reason | `Exercise::begin_resolution`, `post_resolve`, `web/master.html` |

### 05 Player chat and forwarding
| Have | Gap | Where |
|---|---|---|
| Role-to-role messages only as AI output (`MsgKind::RoleMessage` from `parse_reply`); `Role` has `allowed_actions` and `resources` | No player-initiated chat, threads, contacts, unread counts, on/off switch, rate limit; no `message_links` in `Role`; no forward; no `forward_of`, `channel` or `thread` in `Message`; no Chat tab in `player.html` / `play.html`; chat not fed into the AI conversation as such | `exercise` (`Message`, `MsgKind`), `scenario::Role`, `server` routes, `web/*` |

### 06 Restart and resume
| Have | Gap | Where |
|---|---|---|
| Per-turn `WorldSnapshot` in memory, rollback (`Exercise::rollback`, `/api/master/rollback`) | Snapshots are memory-only and not serialisable (`WorldSnapshot` derives only `Clone`); no autosave, `--resume`, `/api/master/restart` or `/api/sessions`; `Exercise` state (messages, proposals, conversations, players, counters) is not serialised or deserialised at all (types derive `Serialize` only) |`sim/src/lib.rs:163`, `exercise/src/lib.rs` |
| Rollback reseeds the fire ensemble: "deterministic (though not bitwise the original one)" (`FireSim::restore`) | **Spec 06 assumes an exact resume** and spec §9 says "restoring a checkpoint gives the same state". Today it does not: a resumed run diverges from an uninterrupted one. Needs either the RNG state in the snapshot, or the spec and tests changed to "statistically equivalent" | `fire/src/lib.rs:504` |
| No session concept | No new-session-id, archive or branch marker in the log; pages don't detect a session change; the big screen doesn't reset | `server`, `web/*`, `viz` |

### 07 Turn authoring
| Have | Gap | Where |
|---|---|---|
| `Scenario.msel: Vec<InjectTemplate>` (id, turn, title, sender, recipients, text, objective) inside the scenario JSON; injects are sent by hand | No `Game` / `TurnPlan` types, no file per game, no `at_start` commands, `release` modes, `forwardable`, `expected_response`, timing per turn; no runtime cursor; no editor page; no validation or dry-run; no L8 generation prompt; no scenario-parameter forms; roles can't be edited in the UI | `scenario/src/lib.rs`, new `exercise::game`, new `web/author.html` |

### 08 Evaluator dashboard
| Have | Gap | Where |
|---|---|---|
| The EXCON console shows everything in English; `/api/debrief` for after the fact | No evaluator role, token, language or read-only enforcement; no metrics module (`T_read`, `T_reply`, `T_decision`), which also needs the timestamps of spec 02; no dashboard page, notes, objectives view, chain view; no on-demand translation with cache; no UI localisation | new `exercise::metrics`, `server`, `web/evaluator.html` |
| Per-run EXCON/player capability keys and server-side endpoint authorization (`docs/api.md`) | Three evaluator identities and a read-only evidence dashboard are implemented; optional evidence-cited AI assessment is implemented; translation and saved annotations remain separate | `server` |

### 09 AI players and AI assessment
| Have | Gap | Where |
|---|---|---|
| One conversation per player, cassette replay, the same reply path for every role | No bot config in `Role`; no `bot_turn` (L9) workflow; on-demand read-only assessment is implemented; no scheduler, delay or message limits; no `actor_kind` in the log; no labels in the UIs; depends on 03 and 07 | `llm/src/tasks.rs`, `server/src/flow.rs` |

## Across specs

- **No persistence layer.** Only the event log survives a restart. 03 and 06 both need one (session directory, serialisable state).
- **Only `{"generation": n}` goes over the WebSocket.** 02, 05 and 08 need typed events, and the pages need to handle them.
- **Extended evaluator workflow** (spec §12.4): EXCON/player/evaluator authorization and evidence UI are implemented; evaluator translation and saved notes remain separate enhancements.
- **The message model is too small.** `Message` (`exercise/src/lib.rs`) needs the fields of specs 02 and 05 (status, timestamps, source text and language, channel, thread, `forward_of`) in one change, not three.
- **Tests.** The 111 headless tests use a mock LLM and don't cover concurrency of translation, ordering or restore. New tests are listed in each spec. The browser recording (`scripts/record/session.js`) has to be extended for chat, forwarding and an evaluator page.
- **Docs.** `docs/api.md`, `docs/llm.md`, `docs/architecture.md` describe the built system and must be updated as each spec lands (`CLAUDE.md`: keep docs in step with code).

## Open decisions that block work

| Decision | Blocks |
|---|---|
| Confirm the translation model and languages (is Belarusian needed?) | 01 |
| Who reviews hy / ka / az translations before the pilot? | 01, pilot |
| Exact resume (store RNG state) or "equivalent" resume? | 06 |
| Consent and retention wording for recording chats | 03, 05, 08 |
| Are bots disclosed to players? Is the AI assessment observations only? | 09 |
| Pilot date, roles and languages; are evaluators present? | priorities of all |
