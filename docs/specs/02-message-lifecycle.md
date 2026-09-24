# 02 — Asynchronous sending and the visible translation step

## Goal
When someone sends something, they see it move through the steps — sent, translating, delivered, read — instead of waiting on a silent spinner or seeing it appear after a delay. The same for receiving: the recipient knows a message is coming.

## Today
The server translates first, then calls `Exercise::add_message` once with both texts. Between the click and the message appearing, the UI shows nothing about what is happening. The pages refresh from `/ws`, which pushes only a generation number.

## Requirements
1. Every message has a **status**: `sending → translating → delivered → read`, or `failed` (translation failed and the English original was delivered, flagged) .
2. The sender sees their own text immediately, with a status chip: "Translating to Armenian…", then "Delivered", then "Read". Failed shows "Delivered in English (translation unavailable)".
3. The recipient sees a placeholder as soon as translation starts ("Message from Fire Commander is being translated…") and the text when it is ready. A delivered message can show the original text in the sender's language on demand.
4. Replies to the AI have the same visibility: `received → interpreting → waiting for the Master → decided`. This makes the 5–15 s wait explicit.
5. Order is kept: messages from one sender to one recipient are delivered in the order sent, even if translation finishes out of order.
6. Timeouts: translation is abandoned after 20 s (configurable) and the message is delivered in English, flagged.
7. Master and evaluators see the status and the timing of every message (feeds the response-time metrics of spec 08).

## Design
- `Message` gets `status`, `sent_ms`, `translated_ms`, `delivered_ms`, `read_ms` (wall clock), `translation { model, latency_ms, failed }`, and `text_src` + `lang_src` (the sender's original). `text` is filled when translation completes. The inbox filter (`Exercise::inbox`) returns messages for a recipient with status `translating` as placeholders (no text).
- Flow: `create_message` (status `translating`, returns id) → background task translates (spec 01) → `complete_message(id, text)` (status `delivered`). The mutex is not held during the call.
- Read receipts: the player page reports visible messages (`POST /api/player/{role}/read {ids}`), sets `read_ms`.
- WebSocket: keep `{"generation": n}` for compatibility, and add typed events `{"event":"message","id":..,"status":..}` so the pages update a single chip without refetching everything. Clients reconnect and fetch state by generation as today, so a missed event is harmless.
- Log events: `message.created`, `message.translated`, `message.delivered`, `message.read`, `message.failed` (see spec 03).

## Failure
Translation error → deliver in English, flagged, log `message.failed`. The exercise never waits on translation for more than the timeout.

## Tests
- Headless: the status sequence with a mock translator; out-of-order completion still delivers in order; timeout produces a flagged English delivery; the read receipt is stored.
- Server integration: a slow mock translation shows `translating` in `/api/player/{role}` and then `delivered`.
- Browser check with the recording pipeline: the chip transitions are visible in a screenshot.

## Depends on / used by
Depends on 01. Used by 05 (chat), 08 (response times).

## Open questions
- Does the sender see the translated text (useful for a bilingual player, confusing for others)? Proposed: hidden by default, a "show translation" toggle.
