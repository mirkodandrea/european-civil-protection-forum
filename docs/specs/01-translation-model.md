# 01 — Dedicated translation model

## Goal
Translate with a model chosen for translation, called through OpenRouter, instead of the general model (`deepseek/deepseek-v4.1-flash`) that also parses replies and answers EXCON prompts, covering the PPRD East languages.

## Today
`llm::tasks::translate` uses the single `Llm` client and model from `OPENROUTER_MODEL`. English targets skip the call. Quality is good for Italian and Armenian, but the model is shared with slow JSON tasks (5–15 s) and every task competes for the same latency and cost budget.

## Requirements
1. A separate model setting for translation, in `.env`: `OPENROUTER_TRANSLATION_MODEL` (falls back to `OPENROUTER_MODEL` if unset). Same key and base URL.
2. All translation goes through one entry point, so every feature (injects, replies, feedback, chat, forwards, dashboard) uses the same model and glossary. Today this means `translate`; feedback, SITREP and the AI answers that are written directly in the player's language stay with the general model, because they are generation, not translation.
3. Any language pair, not only English ↔ X. Player-to-player text (spec 05) goes source → target directly, and `text_en` is kept as the pivot for the log and for the EXCON.
4. Names, zone codes (`F4`, `F4:N`), place names and numbers are preserved exactly.
5. Failure: on timeout or error, retry once, then use the general model, then deliver the English text flagged `translation_failed` (spec 02).
6. Languages: the PPRD East languages — **Armenian (hy), Georgian (ka), Azerbaijani (az), Romanian for Moldova (ro), Ukrainian (uk)**, plus **Russian (ru)** as a common working language, **Italian (it)** and English. Belarusian is not covered (PPRD East 3 activities with Belarus are suspended, to be confirmed). The model must be good on all of them, so the weakest language decides, not the average.
7. Model choice: see [Model selection](#model-selection). Default `google/gemini-3.1-flash-lite`, fallback `openai/gpt-5.6-luna`.

## Design
- `LlmConfig { model, translation_model, … }`. The cassette key already includes the model, so replay stays exact.
- Dedicated MT models are often poor at following instructions such as "keep zone codes". So **mask before, restore after**: place names from the gazetteer, zone codes and numbers with units are replaced by tokens (`⟦1⟧`, `⟦2⟧`), sent to the model, and restored in the result. If a token is missing or duplicated in the output, the translation is rejected and retried with the general model.
- Interface: `translate(text, from: Option<Lang>, to: Lang, glossary) -> Translation { text, model, latency_ms, masked_ok }`. `from = None` lets the model detect the language.
- Result cache in memory keyed by (text hash, from, to), used by forwards and by the evaluator dashboard (spec 08), so a text is translated once per language.

## Model selection

Benchmarked on 2026-09-24 with `scripts/bench_translation.py`, against the OpenRouter model list (458 models). The only dedicated translation models on OpenRouter are Tencent's `hy-mt2-*`, so those were included, with the other cheap, fast multilingual models. Ten exercise phrases (orders with zone codes and place names, numbers, urgent short messages, a clarification question) were translated from English into each language. `anthropic/claude-sonnet-5` judged **adequacy / fluency** (1–5, mean of 10); **kept** is the share of phrases with zone codes, numbers and names intact (checked by code, over all languages); time is the median per call.

| Model | hy | ka | az | ro | uk | ru | it | kept | s | $/M in/out |
|---|---|---|---|---|---|---|---|---|---|---|
| `google/gemini-3.1-flash-lite` | 5.0/4.6 | 5.0/4.6 | 5.0/4.8 | 5.0/4.9 | 4.9/5.0 | 4.6/4.8 | 5.0/4.8 | 99% | 1.0 | 0.25 / 1.50 |
| `google/gemini-3.1-flash-lite-preview` | 4.9/4.9 | 5.0/4.7 | 5.0/4.7 | 4.7/5.0 | 4.8/4.8 | 4.6/4.8 | 5.0/4.9 | 99% | 0.8 | 0.25 / 1.50 |
| `openai/gpt-5.6-luna` | 4.8/4.8 | 5.0/4.7 | 4.9/4.6 | 4.9/4.8 | 4.9/4.9 | 4.8/5.0 | 4.8/4.4 | 100% | 2.7 | 0.20 / 1.20 |
| `google/gemini-3.5-flash-lite` | 4.6/4.0 | 4.5/4.6 | 5.0/4.5 | 4.9/4.8 | 4.7/4.6 | 4.7/4.6 | 5.0/4.9 | 83% | 1.0 | 0.30 / 2.50 |
| `google/gemini-2.5-flash-lite` | 4.8/4.8 | 4.4/4.2 | 4.1/4.0 | 5.0/4.9 | 5.0/5.0 | 4.8/5.0 | 4.6/4.7 | 84% | 0.6 | 0.10 / 0.40 |
| `google/gemma-4-31b-it` | 3.7/3.4 | 4.3/3.7 | 4.8/4.7 | 4.6/4.4 | 4.9/4.4 | 4.8/4.4 | 4.9/4.4 | 100% | 1.1 | 0.09 / 0.34 |
| `deepseek/deepseek-v4.1-flash` (current) | 3.0/2.9 (5 failed) | 4.6/4.2 (1) | 4.1/4.0 (1) | 5.0/4.8 | 5.0/4.6 | 4.8/4.7 | 4.6/4.4 | 90% | 4.4 | 0.14 / 0.42 |
| `qwen/qwen3.8-flash` | failed (1) | 4.1/3.7 (2) | 4.1/3.9 (2) | 4.9/4.7 | 4.6/4.4 (1) | 5.0/4.5 | 5.0/4.8 | 91% | 8.8 | 0.15 / 0.47 |
| `mistralai/mistral-small-2603` | 1.7/1.5 (8) | 0.0 | 3.9/3.7 | 4.8/4.9 | 4.8/4.6 | 4.9/4.4 | 4.5/4.6 | 79% | 0.8 | 0.15 / 0.60 |
| `tencent/hy-mt2-30b-a3b` (translation model) | **0.0** | **0.0** | **0.0** | 4.6/4.2 | 4.4/4.6 | 4.6/4.8 | 4.6/4.8 | 81% | 1.0 | 0.07 / 0.29 |
| `tencent/hy-mt2-7b` (translation model) | 1.0/1.0 | **0.0** | **0.0** | 4.6/4.2 | 4.9/4.6 | 4.8/5.0 | 5.0/4.9 | 83% | 1.0 | 0.07 / 0.29 |

Numbers in brackets are failed calls out of 10 (error or empty answer); 0.0 means no usable translation.

Findings:
- **The dedicated translation models do not cover the languages we need.** Hy-MT2 handles Romanian, Ukrainian, Russian and Italian well, but returns nothing usable for Armenian, Georgian or Azerbaijani, the three languages the exercise is about. They are not an option, even though they are the only models sold as translators.
- **The current default, `deepseek/deepseek-v4.1-flash`, is the weak point for Armenian** (3.0/2.9, half the calls failed) and is slow (4.4 s). The Armenian player in the demo worked only because the answers came from a cassette.
- **`google/gemini-3.1-flash-lite` is the best all-round: 4.6–5.0 in every language, about 1 s per call, and zone codes and names kept in 99%.** It is cheap enough for chat, where every message is a call.
- `openai/gpt-5.6-luna` is nearly as good, keeps everything (100%), but is slower (2.7 s). It is the fallback, on a different provider, so an outage at one does not stop translation.
- Caveats: a single LLM judge (Sonnet 5, a model of another family than the candidates, but not a human); 10 short phrases per language, no long injects; Belarusian and other languages not tested. The EXCON's partners who read these languages should review a sample (a checklist in the pilot) before the Forum. Re-run the script when models change: `python3 scripts/bench_translation.py` (raw output in `runs/bench_translation.json`, git-ignored).

Decision (to confirm): `OPENROUTER_TRANSLATION_MODEL=google/gemini-3.1-flash-lite`, fallback `openai/gpt-5.6-luna`, then the general model, then English.

## Tests
- Unit, with a mock: masking and restoring; a missing token triggers the fallback; English target skips the call; the cache is hit on the second call.
- Benchmark script `scripts/bench_translation.py` (exists): fixed phrases into all PPRD East languages for each candidate; LLM-judged adequacy and fluency, token preservation and latency. Extend to longer injects and to the pilot's reviewers' sample.
- Live test extended (`TTX_LIVE_LLM=1`) with the translation model.

## Depends on / used by
Used by 02, 05, 08. No dependencies.

## Open questions
- Which languages will the pilot and the Forum use exactly? Is Belarusian needed?
- Is a general fast model acceptable instead of a dedicated translator? The benchmark says yes, and a dedicated one is not available for hy/ka/az.
- Who can review a sample of the translations in Armenian, Georgian and Azerbaijani before the pilot?
