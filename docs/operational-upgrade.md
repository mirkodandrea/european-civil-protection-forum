# Aggiornamento operativo dal progetto Roccavento

Implementazione del 2026-10-20 a seguito della scansione in
[lessons-from-propagator-abm.md](lessons-from-propagator-abm.md).

## Avvio e compatibilità

```sh
TTX_QUALITY=projector cargo run --release -p app -- --operational-model
```

Gli scenari storici mantengono i loro interruttori quando caricati normalmente.
`--operational-model` abilita anche `segment_fire_closures`, `terrain_travel`,
`emergency_congestion` e `local_autonomy`. Nel JSON dello scenario si possono
disattivare separatamente, senza usare il flag CLI che li riabiliterebbe.
La grafica è indipendente dalla scelta del modello.

## Cosa è stato trasferito

| Ambito | Implementazione |
|---|---|
| Grafica seria e leggibile | Strade con bordo, larghezze per classe, giunti e ricampionamento; mezzi riconoscibili, etichette e destinazioni distinte dalle posizioni; camera stabile e regia opzionale. |
| Terreno e vegetazione | Heightfield visuale opzionale separato dalla fisica, importer Factory, variazione deterministica, LOD per chunk, carbonizzazione progressiva e ripristino delle piante al rollback. |
| Qualità e verifica | Profili projector/low/high, fixture approvate dal vero Exercise e cattura invisibile senza server o LLM. |
| Routing | Pericolo per segmento diretto, chiusure amministrative separate, accesso a componenti raggiungibili, velocità da pendenza/curve e code al dispatch. ETA e movimento usano gli stessi tempi delle gambe. |
| Politiche delle unità | Settore autorizzato, ricerca di lavoro utile, coordinamento degli obiettivi, limiti d'umidità e consumo effettivo; ricongiungimento prima dei trasferimenti locali, rifornimento fisico, sospensione e ripresa. |
| Previsione | Checkpoint del presente e bollettino EXCON esplicito, perturbazioni meteo e orizzonti; bozza privata, revisione e pubblicazione per ruolo, invalidazione di risultati obsoleti e rollback. |
| Calibrazione | Laboratorio sul vero World, copioni e varianti, semi accoppiati con riferimento senza azioni, telemetria, manifest e replay. |
| Metodo riusabile | Dieci skill adattate al TTX, con provenienza; indice in `ttx-skills.md` e istruzioni d'ingresso in `AGENTS.md`. |

Il composer completo a nodi non è importato: la sua lezione è applicata come
politiche tipizzate, validate e spiegabili. Le persone civili restano aggregate;
il ricongiungimento delle squadre è un tempo di rimobilitazione aggregato, non
una simulazione individuale di tre pompieri per ogni mezzo. Gli ordini espliciti
di linea conservano gli estremi approvati. Non sono introdotti punteggi,
classifiche, ricariche implicite o vincoli artificiali alle evacuazioni.

## Politiche di esecuzione

Lo scenario contiene `tactical_policy` con valori predefiniti:

```json
{
  "version": "ttx-local-v1",
  "sector_radius_m": 300,
  "hose_reach_m": 60,
  "wet_limit_pts": 30,
  "reassess_s": 60,
  "parking_danger": 0.10,
  "work_danger": 0.35,
  "share_work": true,
  "resume_after_withdrawal": true,
  "regroup_s": 60
}
```

Con autonomia attiva, `DeployEngine.at` è il centro della missione: il mezzo può
ricollocarsi su strada entro `sector_radius_m`, senza acquisire altri mezzi o
cambiare ordine. Il raggio della manichetta resta distinto dal settore.
Il tempo di ricongiungimento precede il viaggio; durante questo intervallo il
mezzo resta al posto di partenza e non bagna il combustibile. La ricerca viene
rifatta sul tempo simulato, non sui frame; il lavoro già assegnato ad altri
mezzi non conta come nuovo obiettivo se `share_work` è attivo.

Ogni task espone centro della missione, versione della politica, motivazione,
tempi, percorso, progresso e consumi. Eventi e snapshot conservano decisioni e
stato. La sospensione mantiene l'allocazione esistente; una restituzione
confermata impedisce la successiva ripresa delle unità restituite.

Questi parametri sono ipotesi d'esercitazione. Non rappresentano distanze di
sicurezza o produttività validate sul campo.

## Previsioni revisionate

La console EXCON contiene **Forecast review**. Fermare il clock, scegliere
gruppi meteo, orizzonti ed eventuale bollettino; calcolare la bozza; esaminare
assunzioni e contorni; selezionare destinatari e fonte della revisione;
pubblicare. Le mappe dei partecipanti mostrano soltanto i prodotti ricevuti.
Le probabilità del fuoco corrente e i contorni futuri restano livelli distinti.

La proiezione conserva acqua e linee già applicate, esclude il lavoro futuro
delle squadre e ignora modifiche future delle condizioni al contorno. Non
legge inject futuri. Usa gruppi con meteo perturbato, ognuno con lo stesso
numero di realizzazioni del modello autorevole. I gruppi non sono singole
realizzazioni indipendenti e non costituiscono intervalli di confidenza calibrati.

Il calcolo usa un checkpoint proprio fuori dal mutex dell'esercizio; un solo
job può essere attivo. La generazione deve essere ancora quella revisionata
al completamento e alla pubblicazione. Qualunque modifica richiede una nuova
bozza; un rollback rimuove dalle mappe i prodotti del ramo precedente e ne
conserva la traccia nel debrief. Le osservazioni storiche ordinarie conservano
la semantica preesistente.

Le bozze sono temporanee e limitate a otto per generazione; le pubblicazioni
sono registrate come messaggi e osservazioni. La traduzione del prodotto non
dipende da una chiamata LLM: dati e assunzioni sono disponibili in inglese.

## Verifica riproducibile

```sh
cargo test --release --workspace --offline
scripts/lab/run.sh scripts/lab/smoke.json /tmp/ttx-smoke-new
scripts/lab/run.sh scripts/lab/valley.json /tmp/ttx-valley-new
sh scripts/snapshot.sh runs/visual-review --realizations 4
```

Verifica finale: **205 test passati**, compreso un doctest, nessun fallimento.
Il test live LLM mantiene il proprio opt-in e non ha effettuato chiamate reali.
La suite copre autorizzazione delle previsioni, pubblicazione per ruolo,
invalidazione, assenza di mutazione del modello, determinismo, ripristino,
chiusure locali, percorsi alternativi, pendenze e curve, code e coerenza ETA,
settore delle squadre, coordinamento, rimobilitazione e acqua effettiva.
Fixture grafiche, comandi e limiti sono descritti in [graphics.md](graphics.md);
copioni e telemetria in [operational-lab.md](operational-lab.md).

Eseguite anche 72 corse sintetiche con zero ordini rifiutati e replay identico
su tutti i 60 record della suite principale. Le differenze d'efficacia sono
miste, non una dimostrazione di superiorità della politica locale.
`node scripts/qa/forecast.cjs` ha verificato in Chrome headless il calcolo privato,
l'anteprima e la pubblicazione revisionata su desktop 1440×1100 e mobile 390×844,
senza errori JavaScript. Il test usa l'installazione Playwright del recorder,
una porta locale temporanea e nessuna chiamata LLM.

Ispezionate catture GPU invisibili di scena incendiata, mezzo al lavoro e
rollback, anche dopo il trasferimento delle 15 varietà vegetali. Le immagini
sono in `runs/visual-review/`. L'import del pacchetto sorgente Roccavento con
heightfield visuale a 5 m è riuscito: 1601×1601 nodi, 392 edifici, 82 strade,
1521 occupanti, senza alterare il DEM fisico. L'import non aggiunge inneschi o MSEL.

## Limiti da considerare nel pilot

- Le chiusure da incendio restano persistenti fino a reset/rollback: nessuna
  riapertura automatica dedotta solo dall'età delle fiamme.
- Il ritardo dei soccorsi fotografa la coda al dispatch/rerouting. Non inserisce
  il mezzo con priorità dinamica nel FIFO civile.
- La microtattica non è un modello operativo validato. Una strategia che
  migliora uno scenario non dimostra un miglioramento su tutti i territori.
- Screenshot e browser headless non sostituiscono verifica sul proiettore,
  misure FPS sulla macchina EXCON e prova con partecipanti reali.
- I bot dei ruoli e l'editor comportamentale a nodi restano funzionalità
  distinte dalle politiche deterministiche delle squadre implementate qui.


## Completamento grafico PBR (2026-10-20)

La prima scansione non aveva trasferito la pipeline grafica più recente.
Ora sono collegati materiali distinti per terreno, foglie e strutture, shader
di bruciatura GPU, texture di normali dal DEM visuale, cielo procedurale e
illuminazione ambientale con riflessi. Autobotte, aereo e squadra usano i nuovi
modelli articolati e i relativi identificatori di superficie; le abitazioni
hanno tetti, infissi e facciate dettagliate. La provenienza comprende anche le
modifiche locali del progetto sorgente successive al commit `f359dc0`.

La mappa web resta operativa 2D; il PBR riguarda il grande schermo Bevy. Gli
stati del modello e l'allocazione delle risorse restano autorevoli. I dettagli
grafici non aggiungono persone o decisioni alla simulazione. Pipeline, assunzioni
solari e copione GPU riproducibile sono in [graphics.md](graphics.md).

Il collaudo del DEM fine ha inoltre individuato e corretto la perdita del tipo
visuale nell'importer Factory: `visual_kind` conserva piazza, parcheggio, campo,
area di attesa e categorie dei monumenti. Le superfici aperte seguono il terreno;
chiese e caserme usano i modelli dedicati. Il campo è opzionale e non modifica
tipo operativo, occupanti, capacità o esposizione. I vecchi scenari restano
compatibili. Gli ingombri importati restano quelli rettangolari del formato TTX.


Completati anche il codice delle fiamme del gioco (nastri animati, shader,
texture, fumo, braci e bagliore) e l'harness a stati multipli con misure CPU,
render e GPU. Quattro test Python verificano il parser; 205 test Rust passano.
Il collaudo nativo ha prodotto e ispezionato incendio, dettaglio, vista zenitale
con UI e rollback. Specifiche e comandi in [graphics.md](graphics.md).
