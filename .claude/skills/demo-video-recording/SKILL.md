---
name: demo-video-recording
description: Registrare un video dimostrativo narrato del TTX (sessione guidata, voce TTS, sottotitoli) quando servirà di nuovo.
---

# Video dimostrativo narrato

La pipeline di registrazione è stata rimossa il 29/10/2026: si era fermata
dietro l'interfaccia (flusso Rispondi, scelta della lingua, prompt AI nuovi) e
non valeva la pena mantenerla. L'ultima versione funzionante è nella storia git
di `scripts/record/` (commit `fa3740e`, il padre della rimozione):
`git show fa3740e:scripts/record/README.md`. Riusala come riferimento, non
ripristinarla alla cieca: rifai la sessione sull'interfaccia del momento.

Restano nel codice, e si riusano così come sono:
- `crates/llm/src/cassette.rs`: `TTX_LLM_CASSETTE=<file.jsonl>` con
  `TTX_LLM_CASSETTE_MODE=auto|record|replay` registra e rigioca le risposte
  del modello.
- `crates/viz` (`viz::record`): con `TTX_CINEMATIC=1`, finché esiste
  `runs/rec/ON` il simulatore manda un fotogramma ogni due direttamente a
  ffmpeg (H.264 hardware, timestamp reali) in `runs/rec/sim.mkv`.

## Decisioni da rispettare

- La narrazione racconta **la storia dell'incidente**, non le meccaniche del
  gioco o i pulsanti.
- Voce: OpenRouter `POST /api/v1/audio/speech`, modello
  `google/gemini-3.1-flash-tts-preview`, voce `Charon`, PCM 24 kHz → WAV.
- **Ogni clip va trascritta** (`openai/gpt-4o-mini-transcribe` via
  `/api/v1/audio/transcriptions`) e confrontata con il testo; si rigenera
  (fino a 4 volte) se perde frasi o cambia lingua. È successo: una riga che
  conteneva "in Italian" è stata letta in inglese.
- Cache dell'audio per (modello, voce, testo): si paga solo ciò che cambia.
- Sottotitoli bruciati nel video **e** traccia morbida, più `.srt` e `.vtt`.
  L'ffmpeg di questa macchina non ha `drawtext`: i sottotitoli si rendono come
  PNG trasparenti con Chrome e si sovrappongono.
- La chiave OpenRouter si inserisce prima di iniziare a registrare e non deve
  mai apparire sullo schermo.

## Come era fatta e cosa ha funzionato

Due varianti, entrambe guidate da Playwright:

1. **Server** (`record.sh`): `ttx` su una porta propria (8090, mai :8080) in una
   cartella temporanea, cassetta LLM, simulatore cinematico in ffmpeg; Chrome a
   480×1040 su `play.html` (tutti i ruoli in formato telefono) accanto al 3D.
2. **Standalone** (`standalone/session.cjs`): l'app nel browser con il 3D
   visibile, registrata con lo screencast CDP (`Page.startScreencast`, JPEG,
   nessuna finestra visibile necessaria). `TTX_DRY=1` faceva una prova senza
   registrare, con uno screenshot per didascalia.

Principi che hanno reso la registrazione affidabile:
- `caption(id)` segna dove inizia una riga e **aspetta che la precedente sia
  finita di leggere**: è la voce a dare il ritmo alla sessione.
- Si aspetta lo **stato** (API o attributi `data-*`), mai pause fisse sui
  tempi del modello.
- Le attese lunghe del modello si registrano come intervalli di accelerazione
  (`speedups`, es. 8×) in `marks.json`; il montaggio accelera l'immagine e
  rimappa i segni, così la voce resta allineata.
- Un solo passaggio ffmpeg finale: video della pagina + simulatore
  (sincronizzati con i tempi di avvio), titolo, sottotitoli, narrazione
  mixata sui segni (AAC stereo).
- Entrambi i flussi sono già codificati quando la sessione finisce: una nuova
  registrazione dura quanto il video più ~20 s.

## Prima di registrare di nuovo

- Rigenera la cassetta LLM (`TTX_LLM_CASSETTE_MODE=record`): ogni modifica ai
  prompt la invalida.
- Riscrivi la sessione sui flussi attuali (Rispondi con destinatario, tipo e
  fondamento; conversazioni avviate da un inject; conferma della lingua di ogni
  ruolo prima di avviare) riusando gli helper degli script `scripts/qa/*.cjs`,
  che sono mantenuti al passo con l'interfaccia
  (`scripts/qa/standalone-languages.cjs` per le lingue).
- Fai prima una prova a secco con screenshot e guarda ogni immagine.
- Ascolta o fai trascrivere ogni clip e guarda il video finale prima di
  consegnarlo.
