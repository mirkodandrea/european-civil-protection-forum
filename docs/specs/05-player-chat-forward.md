# 05 — 1-to-1 chat between players, with translation, and inject forwarding

## Goal
Two players can talk to each other directly, each in their own language, and a player can forward an inject they received to another player. This is the **pilot's must-have**.

## Today
Players cannot write to each other directly. The AI can produce a role message from a reply (`messages[]` of `parse_reply`), which the exercise delivers as `MsgKind::RoleMessage`. There is no thread view, no forward, and no way for a player to start a conversation.

## Requirements
### Chat
1. A player opens a conversation with any role they are allowed to contact (`roles.yaml` `message_links`, default: all roles). The Master can switch chat on or off, globally or per pair, and by phase (for example off during resolution).
2. Text is written in the player's language and delivered in the recipient's language (spec 01). Both sides can show the original and the translation. The status steps of spec 02 apply, so the sender sees "translating…" and then "delivered/read".
3. Each pair has one thread, ordered by time, with unread counts. The player page has a Chat tab next to the map and inbox, phone-first as the rest.
4. Chat has **no simulation effect and needs no approval**. It is not parsed into commands. If a player writes an order in a chat, the page reminds them that orders go in the reply box (a short hint under the input, and a Master-visible flag when the text looks like an order — proposed, optional).
5. The Master and evaluators see all threads (the join screen says so). The Master can post into a thread as "Exercise control".
6. Rate limit per sender (default 10 messages/minute) to keep translation cost bounded.

### Forwarding
7. Any inject a player has received can be forwarded to another role with an optional note. The recipient sees: "Forwarded by *Fire Commander*", the original sender and time, the note, and the inject text in their language.
8. The forwarded text is translated from the **original** (English source or original text), not from the sender's translation, to avoid double translation loss.
9. An inject can be marked not forwardable in the game file (`forwardable: false`), and the Master can disable forwarding globally. A forward can only go to roles the sender may contact.
10. Forwarding is recorded as a link between messages (`forward_of`) so the chain inject → forward → reaction can be seen in the log and in the evaluator dashboard.

### AI context
11. Chat and forwards a player sent or received are added to that player's AI conversation as context entries (as injects and role messages already are), so their next reply to the AI can refer to them ("do what the mayor asked"). Each entry uses the English pivot, and the recorded conversation stays complete (spec 03).

## Design
- `Message` (spec 02) gets `channel` (`inject | chat | ai | system`), `thread` (sorted pair of roles, `"mayor|fire"`), and `forward_of: Option<u64>`. `MsgKind::Chat` and `MsgKind::Forward` are added.
- API: `POST /api/player/{role}/chat {to, text}` → `{message}`; `POST /api/player/{role}/forward {message, to, note?}`; `GET /api/player/{role}` returns `threads` and `contacts`; `POST /api/master/chat {enabled, pairs?, forwarding?}`; `POST /api/master/chat/{thread}/post`.
- Chat text is stored as `text_src`/`lang_src`, `text_en` (for the Master and the log) and `text` (recipient's language), like other messages.
- A player who changes language mid-session keeps the old messages as they were delivered; new messages use the new language.

## Failure
Translation unavailable → delivered in English, flagged. The recipient can still read it. Chat off → the page shows why.

## Tests
- Headless: A ↔ B in two languages; order kept; a forward keeps the original sender and links `forward_of`; a non-forwardable inject is refused; contact rules are enforced; the rate limit.
- The forwarded text is translated from the source, not from a translation (check the mock's inputs).
- The AI conversation of both players contains the chat entries.
- Browser: extend `scripts/record/session.js` with a chat exchange and a forward in the recording.

## Depends on / used by
Depends on 01, 02, 03. Used by 08 (threads and forward chains) and 09 (bots chat).

## Open questions
- Group chats (three or more roles)? Out of scope for the pilot.
- Should a chat message that looks like an order create a proposal automatically? Proposed: no, only a hint.
- Can players see each other's language, or only the role name?
