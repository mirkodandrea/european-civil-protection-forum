# To do

What is not built yet. The v0.3 items have their own draft specs in [`specs/`](specs/README.md), with priorities and dependencies.

## Planned for v0.3 (pilot session and after)

Priority order proposed in [`specs/README.md`](specs/README.md); the pilot's must-have is player chat with inject forwarding.

1. [Dedicated translation model](specs/01-translation-model.md)
2. [Asynchronous message states (visible translation step)](specs/02-message-lifecycle.md)
3. [Complete session recording, including conversation logs](specs/03-session-recording.md)
4. [1-to-1 player chat with auto-translation; forwarding injects](specs/05-player-chat-forward.md)
5. [Restart and resume a session](specs/06-session-restart.md)
6. [Time compression per turn, time jumps](specs/04-time-compression.md)
7. [Master pre-authors turns; editor and generation](specs/07-turn-authoring.md)
8. [Live evaluator dashboard in the evaluator's language](specs/08-evaluator-dashboard.md)
9. [AI players; AI assessment of the exercise](specs/09-ai-players.md)

## Later (from the v0.2 draft)

- Agents: civilians, ground crews and vehicles (ABM from `propagator_abm`), traffic on the road network
- L6/L7 AI workflows (inject suggestions, debrief narrative; L7 is folded into spec 09)
- A real scenario for the Forum (the synthetic Valverde valley stands in)
- Role/permission editing in the UI
- Authentication for the web UIs before any non-private deployment
- A local model (Ollama) as an LLM fallback; today only OpenRouter is wired in
