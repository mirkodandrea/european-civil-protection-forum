# Interfaccia conversazionale — prima implementazione

Questa implementazione preliminare vive nel worktree `.worktrees/chat-interface`,
branch `refactor/chat-interface`. Il worktree include lo stato locale del progetto
esistente al momento della creazione, senza un commit di baseline aggiuntivo.

## Frontend Rust/WebAssembly

Le superfici web sono ora componenti Yew nel crate `crates/frontend`, compilati
con `./scripts/build_frontend.sh` e serviti da Rocket da `web/dist/`. Non restano
moduli JavaScript applicativi scritti a mano. La shell HTML e il loader JavaScript
sono generati da Trunk/wasm-bindgen; lo stile resta CSS. Chat, bozze private,
revisione, mappe canvas, console avanzata e valutatore sono implementati in Rust.
I nomi degli entry point e le API restano compatibili. Nella console avanzata i
parametri strutturati sono modificabili nei moduli Rust; l'interpretazione manuale
EXCON può aggiungere comandi JSON nella scheda della proposta.

## Avvio e superfici

```sh
./scripts/build_frontend.sh
cargo run --release -p app -- --headless
```

Aprire il link EXCON stampato all'avvio. `master.html` presenta conversazioni per
player, invio degli inject nella stessa conversazione, mappa completa e revisione delle azioni direttamente nella conversazione; `player.html` presenta conversazioni con
EXCON e gli altri ruoli e una mappa tattica. `play.html` apre EXCON, senza iframe che
impersonano i player. I link individuali si trovano nella tab mappa di EXCON.
`advanced.html` conserva la console precedente per forecast, interventi, rollback, debrief e altri strumenti non ancora riprogettati.
Il clock interno resta disponibile solo nella console avanzata; il percorso
principale usa il pulsante EXCON di avanzamento del turno.

## Flusso

- Ogni coppia ha una conversazione, ricavata da mittente e destinatario dei messaggi
  persistenti di Exercise. Gli inject da mittenti simulati compaiono nella chat EXCON.
- Un messaggio non richiede un inject precedente né una base decisionale compilata.
  I documenti e le motivazioni della vecchia dichiarazione rimangono nel modello e
  nelle API precedenti; la chat non inventa una motivazione a nome del partecipante.
- I messaggi informativi possono essere inviati direttamente disattivando
  «Distingui automaticamente chat e azioni con l’AI». Con l'opzione attiva, l'AI
  classifica il messaggio al primo invio: se non rileva azioni, lo traduce e lo
  consegna come chat; se rileva azioni, prepara una bozza privata da verificare e
  confermare. Disattivando l'opzione il testo viene inviato direttamente come chat.
- L’assistente individua le azioni e, se mancano dati essenziali, rivolge una
  domanda precisa al player nella sua lingua. Il player risponde nel composer;
  non deve interpretare un badge generico «serve un chiarimento».
- Quando il testo è completo, il player vede il messaggio finale e le azioni e
  conferma esplicitamente quando contiene azioni. Un'azione viene inviata a EXCON
  per la revisione; se la chat è diretta a un altro player, il messaggio arriva al
  contatto dopo la conferma. Un testo senza azioni viene invece consegnato come
  chat tradotta. Modificare il testo richiede una nuova analisi; la conferma non
  avvia una seconda estrazione AI.
- Le domande private non vengono condivise con EXCON nemmeno dopo l’invio.
  EXCON riceve il testo finale confermato e le azioni da revisionare. Le approvazioni
  e le conseguenze fisiche restano nel percorso Exercise / Command esistente.
- Per le azioni complete la conferma è richiesta dall'interfaccia con un pulsante
  esplicito e l'elenco dei comandi; l'AI chiede chiarimenti solo quando mancano dati
  necessari per identificare l'azione.
- Se l’AI non è disponibile, la bozza resta privata finché il player non sceglie
  esplicitamente l’invio per interpretazione manuale di EXCON.
- Il rollback conserva i messaggi come evidenze storiche e annulla le proposte del
  ramo ripristinato. Una traduzione terminata dopo un cambio turno/rollback/clock
  viene rifiutata; il testo resta nel composer per un nuovo invio. Una risposta AI
  tardiva non riapre proposte già annullate.

## Traduzione e accesso

`POST /api/player/{role}/chat` e `POST /api/master/chat` ricevono
`{to, text, interpret?: false}` e restituiscono
`{message, proposal, translation_failed}`. Il primo endpoint è vincolato alla chiave
del ruolo. Un player riceve solo i messaggi di cui è mittente o destinatario. Le chat
1:1 tra player non compaiono nella vista EXCON né nei log restituiti alla sua console;
EXCON vede separatamente le proposte operative dopo la conferma del player. La
directory `contacts` dello static player espone soltanto id e nome degli altri ruoli,
senza inventario o autorità operative.

Si conserva il testo originale, la lingua sorgente, la copia inglese e la versione
per il destinatario. La traduzione usa un modello dedicato configurabile, di default
`google/gemini-3.1-flash-lite`, tramite il gateway LLM esistente: lingua sorgente →
inglese → lingua destinatario. EXCON scrive in inglese in questa prima versione.
Se la traduzione fallisce, viene consegnato il testo disponibile con un indicatore
esplicito, senza presentarlo come una traduzione riuscita. Il sender vede il proprio
originale e il destinatario può espandere il testo originale. L'invio attende la
traduzione; lo stato intermedio è locale al composer, senza ricevute di lettura.

Durante la preparazione privata la chat mostra un indicatore animato che specifica
che l'assistente sta ragionando; invio e traduzione hanno un messaggio di attesa
distinto. Nelle chat dirette tra player, un heartbeat temporaneo mostra quando il
contatto sta scrivendo. La presenza scade dopo cinque secondi e resta fuori da
Exercise, snapshot, log e debrief.

## Mappe

La mappa EXCON legge la vista completa del simulatore. Quella player usa esclusivamente
`player_static` e `player_map`, più le missioni proprie già autorizzate: nessuna richiesta
alla vista globale. La presentazione tattica omette gli edifici anonimi e il readout
metrico del puntatore; mantiene strade, riferimenti e rapporti ricevuti. EXCON può
pubblicare esplicitamente il perimetro corrente per un singolo ruolo dalla nuova tab.
La chat testuale non aggiorna da sola la conoscenza cartografica.

## Verifica

```sh
cargo test --release --offline -p exercise -p server -p llm
cargo test --release --offline --workspace
cargo build --release --offline -p app --no-default-features
node scripts/qa/chat-interface.cjs
```

Il runner browser usa Playwright già previsto dal recorder, un server temporaneo e
Chrome headless, senza LLM esterno né finestre in primo piano. Se si condivide un
target o un'installazione Playwright esterna, impostare `TTX_BINARY` e
`TTX_PLAYWRIGHT` con percorsi assoluti. `TTX_QA_PORT` cambia la porta (default 8097).
Le fixture passano dalle vere API: inject, chat, comando IssueWarning revisionato dentro la chat,
rapporto cartografico e risoluzione. Immagini desktop/mobile e manifest rimangono in
`runs/chat-interface-review/` e vanno ispezionati.

## Limiti della prima tranche

La lingua dell'interfaccia è italiana; la lingua dei messaggi è scelta dal player.
Non sono ancora presenti gruppi, allegati nella chat, inoltro inject, associazione
manuale della risposta a uno specifico messaggio, ricevute server di lettura,
ordinamento garantito di invii simultanei durante traduzioni, salvataggio dei draft
tra ricaricamenti o ripresa completa della sessione dopo riavvio del processo.
I badge di lettura sono locali alla pagina. La revisione EXCON è incorporata nella conversazione del player: azioni e parametri sono presentati con etichette leggibili e modificabili; l'interpretazione AI usa il modello configurato esistente.
Il recorder della vecchia interfaccia richiede un adattamento; il runner forecast
continua a verificare la console conservata in `advanced.html`.

Questa scelta aggiorna la direzione di `specs/05-player-chat-forward.md`: l'esclusione
assoluta di comandi ricavati dalla chat non si applica alla nuova interfaccia.
L'autorizzazione EXCON delle azioni fisiche resta in Exercise.

## Esito della verifica preliminare

Workspace headless: 220 test passati; successivamente aggiunto e superato anche il
test HTTP di traduzione terminata dopo rollback (221 test distinti complessivi).
Build release del server senza viz riuscita. Runner Chrome headless superato su
1440×1000 e 390×844, senza errori JavaScript né overflow orizzontale; composer mobile
verificato entro il viewport. Cinque catture finali ispezionate in
`runs/chat-interface-final/`. Traduzione e interpretazione coperte con mock; nessuna
chiamata a modelli esterni durante la verifica. Il test live conserva il suo opt-in.

## Inject nelle conversazioni

Gli inject si preparano e si inviano nella conversazione EXCON–player, usando
lo stesso campo di testo e lo stesso pulsante dei messaggi ordinari. Non esiste
una tab o pagina Inject nella nuova interfaccia. EXCON sceglie «Scrivi come →
Inject» e può compilare titolo e mittente simulato oppure caricare un testo MSEL
previsto per quel ruolo. Per default il destinatario è il player della conversazione
aperta; un template può essere inviato ai destinatari originali dello scenario
scegliendoli esplicitamente dal selettore. Il testo resta modificabile nel composer.

Se un template viene inviato inalterato ai suoi destinatari originali, viene
registrato come invio MSEL. Una copia modificata, un invio al solo ruolo corrente
o un template già inviato producono un inject personalizzato. Il tipo, il testo e
i metadati della bozza rimangono associati alla singola conversazione anche quando
EXCON passa a un altro player. Gli invii compaiono nella cronologia della chat;
non esiste una cronologia separata da consultare. Il player vede messaggi del
mittente simulato e risponde semplicemente nello stesso flusso, senza controlli
di authoring o badge tecnico «INJECT». La mappa e il tempo fisico non cambiano
per il solo invio.

## Dispositivi player

Il riferimento primario sono tablet da 8–10 pollici, usati in verticale o in
orizzontale. I controlli player mantengono aree di tocco da almeno 44 px e testo
principale da 16 px anche sui dispositivi ibridi. La chat sfrutta una colonna
contatti laterale; la mappa si adatta all'altezza disponibile e, in verticale,
le risorse passano sotto la mappa. Le etichette tattiche sono ingrandite e le
bozze rimangono presenti passando tra viste e orientamenti.
La verifica automatica usa viewport CSS 800×1280, 1280×800, 1024×1366 e 1366×1024,
più una vista ridotta a 1280×480 per approssimare la tastiera a schermo. Non è una
prova su dispositivi fisici: pollici e viewport CSS dipendono da densità e browser.

Verifica tablet completata con Chrome headless: tutte e quattro le dimensioni e
il viewport ridotto passano i controlli su area dei pulsanti, visibilità del composer,
mappa entro il viewport, conservazione della bozza e assenza di overflow. Sono passati
anche invio MSEL dal composer della conversazione, inject personalizzato riservato
al sindaco e risposta in chat. Le 16 catture sono state ispezionate in
`runs/tablet-interface-complete/`, con panoramica in `contact-sheet.jpg`.

La verifica corrente degli inject nelle conversazioni è in
`runs/conversation-inject-final/`: invio MSEL dal campo chat, inject personalizzato
al solo player corrente, bozza con metadati conservata tra conversazioni, assenza
di tab Inject e di badge tecnico nella vista player. Passa anche la suite browser
per tablet in entrambi gli orientamenti; le 16 catture sono state ispezionate.

## API di preparazione privata

`POST /api/player/{role}/draft {to,text}` prepara una bozza senza inviarla.
Per rispondere al chiarimento, includere `draft` e `revision` restituiti dalla
chiamata precedente. Il risultato `draft` contiene `status` (`needs_clarification`,
`ready`, `ai_unavailable`), `question`, `message` e le azioni individuate.
`GET /api/player/{role}` restituisce solo le bozze del ruolo autenticato.
`POST /api/player/{role}/draft/submit {draft,revision}` invia esattamente la revisione
confermata; il client non può sostituire i comandi. Solo una bozza non interpretabile
per indisponibilità AI accetta `manual:true` per l’invio manuale esplicito.
La vecchia chiamata chat con `interpret:true` viene rifiutata: non può consegnare
un messaggio operativo prima del chiarimento. Il vecchio endpoint `/reply` resta
compatibile con gli script e con la console avanzata.

Verifica corrente: `scripts/qa/player-preparation.cjs` usa un gateway finto locale
per mostrare domanda specifica, chiarimento del player, revisione e conferma; controlla
che il traduttore sia separato e l'estrattore rimanga quello esistente, senza chiamate
AI esterne. Catture ispezionate in `runs/private-preparation-review/`.
