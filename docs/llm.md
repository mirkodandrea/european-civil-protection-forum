# LLM workflows

`crates/llm` is the only code that calls a model, and it never touches the simulation. It returns text, or candidate actions as JSON, which the exercise validates and the Master approves.

## Configuration

`.env` in the repo root (loaded by `ttx`):

```
OPENROUTER_API_KEY=sk-or-...
OPENROUTER_MODEL=deepseek/deepseek-v4.1-flash   # default if unset
OPENROUTER_BASE_URL=https://openrouter.ai/api/v1  # default
```

JSON workflows use `response_format: json_object`, temperature 0.2, a 90 s timeout, and one retry on unparseable output. Answers are extracted tolerantly: code fences and surrounding prose are stripped. Each call is logged with `tracing` (task, latency, size).

## Workflows (`crates/llm/src/tasks.rs`)

| Task | When | Input | Output |
|---|---|---|---|
| `translate` | Each inject and role message, per recipient | English text, target language | Translation; names, zone codes and numbers kept. English targets skip the call |
| `parse_reply` | Each player reply | System: role, language, allowed-action catalogue, other roles, gazetteer. Messages: **the player's conversation so far**, then the current digest + the new reply | `detected_language`, `translation_en`, `actions[]`, `messages[]` (to other roles), `clarification`, `summary` |
| `master_prompt` | Director's AI prompt | System: full catalogue, roles, gazetteer. Messages: **the Director's conversation so far**, then the full digest + the instruction | `actions[]`, `injects[]` (drafts), `clarification`, `summary` |
| `feedback` | After each decision on a player proposal | Decision facts (English) | 2–4 sentence EXCON reply in the player's language |
| `situation_update` | After each turn, per player | Role digest + turn events | ≤ 120-word SITREP in the player's language |

Grounding comes from `World::gazetteer()` (map and zone grid description, places with aliases, facilities, roads, resource meanings, reinforcement offers) and `World::digest(role)` (clock, weather, fire summary by zone, facilities, populated zones, roads, the role's resources, arrivals).

## Guardrails in the prompts

- Only documented action types and shapes; only gazetteer names or zone codes.
- Add actions only for what the player clearly ordered.
- Ambiguous → no action + one clarification question in the player's language.
- Do not judge feasibility (the simulator does).
- Feedback and SITREPs: facts from the data only, no advice.

## Conversations

Each player, and the Director, has one ongoing conversation with the AI (`Exercise::conversations`, last 60 entries). It collects, in order:

- everything the player received: injects, messages from other roles, exercise-control feedback, situation updates (added automatically when delivered, as "user" context entries);
- each reply the player wrote;
- the AI's interpretation of it (actions, messages, clarification, summary) as the "assistant" turn.

`parse_reply` sends that conversation (consecutive context entries merged) plus the current situation and the new reply. Follow-ups keep their meaning: an answer to a clarification, "send them there", or a reference to an earlier order. The AI is told not to repeat actions from earlier replies. The Director's prompts work the same way. `GET /api/master` exposes the conversations for inspection.

## Record / replay (cassette)

`TTX_LLM_CASSETTE=<file>` makes every request go through a JSONL cassette keyed by (model, task, system prompt, user prompt, JSON mode) (`llm::cassette`). In `auto` mode hits are replayed and misses recorded; `replay` refuses misses; `record` always asks the model. The demo recording uses it: the simulator is seeded, so replayed answers make every later prompt identical, and the whole session replays exactly, with no model latency.

## Fallback

`Llm::off()` (no key) or any error:

- injects and messages are sent in English;
- a reply that cannot be parsed becomes a `pending` proposal with the summary "(AI unavailable: …) — enter the actions manually", and the Master edits in the commands;
- feedback and situation updates fall back to English facts.

## Testing

- Unit tests with `Llm::mock(|task, system, user| ...)`: JSON extraction, retry, parse mapping, English shortcut.
- Server integration tests run the full loop with a mock.
- Live: `TTX_LIVE_LLM=1 cargo test -p llm --test live -- --nocapture` checks translation plus parsing of an evacuate/warn/message reply.

Observed with `deepseek/deepseek-v4.1-flash`: about 5–15 s per parse. Italian and Armenian translations are good. Zone edges (`F4:N`), place names and role messages are extracted reliably, and vague orders ("send the crews over there") produce a clarification question.
