# LLM workflows

`crates/llm` is the only code that calls a model, and it never touches the simulation. It returns text, or candidate actions as JSON, which the exercise validates and the EXCON approves.

## Configuration

`.env` in the repo root (loaded by `ttx`):

```
OPENROUTER_API_KEY=sk-or-...
OPENROUTER_MODEL=deepseek/deepseek-v4.1-flash   # default if unset
OPENROUTER_BASE_URL=https://openrouter.ai/api/v1  # default
OPENROUTER_PROVIDERS=Together,CoreWeave,Modal      # default; `auto` = OpenRouter's choice
```

Requests carry `provider: {order, allow_fallbacks: true, require_parameters: true, data_collection: "deny"}`. The default order is the fastest hosts of the default model by OpenRouter's P50 latency (Together 0.41 s, CoreWeave 0.58 s, Modal 1.01 s; Sep 2026). The turn loop waits for whole replies, so latency shows directly. `require_parameters` skips hosts that ignore `response_format`; `data_collection: "deny"` skips hosts that keep prompts (if no host qualifies the call fails and the exercise falls back to manual flow). Re-check the order when changing the model.

JSON workflows use `response_format: json_object`, temperature 0.2, a 90 s timeout, and one retry on unparseable output. Answers are extracted tolerantly: code fences and surrounding prose are stripped. Each call is logged with `tracing` (task, latency, size).

## Workflows (`crates/llm/src/tasks.rs`)

| Task | When | Input | Output |
|---|---|---|---|
| `translate` | Each inject and role message, per recipient | English text, target language | Translation; names, zone codes and numbers kept. English targets skip the call |
| `parse_reply` | Each player reply | System: role, language, allowed-action catalogue, other roles, gazetteer. Messages: **the player's conversation so far**, then the current digest + the new reply | `detected_language`, `translation_en`, `actions[]`, `messages[]` (to other roles), `clarification`, `summary` |
| `master_prompt` | EXCON's AI prompt | System: full catalogue, roles, gazetteer. Messages: **the EXCON's conversation so far**, then the full digest + the instruction | `actions[]`, `injects[]` (drafts), `clarification`, `summary` |
| `translate` | After each decision on a player proposal | Recorded decision facts (English) | Faithful translation of those facts |
| `translate` | After each turn, per player | Deterministic summary of recorded state and events | Faithful translation, without operational additions |

Grounding comes from `World::gazetteer()` (map and zone grid description, places with aliases, facilities, roads, resource meanings, reinforcement offers) and `World::digest(role)` (clock, weather, fire summary by zone, facilities, populated zones, roads, the role's resources, arrivals).

## Guardrails in the prompts

- Only documented action types and shapes; only gazetteer names or zone codes.
- Add actions only for what the player clearly ordered.
- Ambiguous → no action + one English clarification draft for EXCON review.
- Do not judge feasibility (the simulator does).
- Feedback and SITREPs: facts from the data only, no advice.

## Conversations

Each player, and the EXCON, has one ongoing conversation with the AI (`Exercise::conversations`, last 60 entries). It collects, in order:

- everything the player received: injects, messages from other roles, exercise-control feedback, situation updates (added automatically when delivered, as "user" context entries);
- each reply the player wrote;
- the AI's interpretation of it (actions, messages, clarification, summary) as the "assistant" turn.

`parse_reply` sends that conversation (consecutive context entries merged) plus the current situation and the new reply. Follow-ups keep their meaning: an answer to a clarification, "send them there", or a reference to an earlier order. The AI is told not to repeat actions from earlier replies. The EXCON's prompts work the same way. `GET /api/master` exposes the conversations for inspection.

## Record / replay (cassette)

`TTX_LLM_CASSETTE=<file>` makes every request go through a JSONL cassette keyed by (model, task, system prompt, user prompt, JSON mode) (`llm::cassette`). In `auto` mode hits are replayed and misses recorded; `replay` refuses misses; `record` always asks the model. The demo recording uses it: the simulator is seeded, so replayed answers make every later prompt identical, and the whole session replays exactly, with no model latency.

## Fallback

`Llm::off()` (no key) or any error:

- injects and messages are sent in English;
- a reply that cannot be parsed becomes a `pending` proposal with the summary "(AI unavailable: …) — enter the actions manually", and the EXCON edits in the commands;
- feedback and situation updates fall back to English facts.

## Testing

- Unit tests with `Llm::mock(|task, system, user| ...)`: JSON extraction, retry, parse mapping, English shortcut.
- Server integration tests run the full loop with a mock.
- Live: `TTX_LIVE_LLM=1 cargo test -p llm --test live -- --nocapture` checks translation plus parsing of an evacuate/warn/message reply.

Observed with `deepseek/deepseek-v4.1-flash`: about 5–15 s per parse. Italian and Armenian translations are good. Zone edges (`F4:N`), place names and role messages are extracted reliably, and vague orders ("send the crews over there") produce a clarification question.

## EXCON control of operational suggestions

Parsing never delivers operational messages. Role messages and clarification
questions are stored as `communication_drafts`; scenario injects remain
`inject_drafts`. The EXCON sees, edits, approves or rejects drafts alongside
actions. Action-dependent communications are sent once after successful
execution; approved communication-only proposals send immediately. Rejection,
failed execution and rollback do not authorize delivery. Delivery records link
the reviewed proposal to the resulting messages/injects.

Feedback and end-of-turn summaries use deterministic recorded facts and the
translation workflow. Its prompt forbids added facts, advice, decisions or
changing proposed actions into completed actions; original English text remains
available as evidence. Model failures fall back to the original facts. This is
a model instruction, not a formal semantic guarantee; EXCON-reviewed operational
drafts remain the control boundary for new operational content.

EXCON prompts can recommend a complete `resource_limits` configuration. The
prompt includes current training phase, effective limits and resource accounting;
recommendations are retained as proposal drafts. Only explicit EXCON approval
applies them, with edits and before/after policy evidence recorded. Pending or
rejected recommendations have no operational effect.
