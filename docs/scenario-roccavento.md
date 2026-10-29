# Isola di Roccavento — il fuoco del valico

Territorio `roccavento_island` e pack `roccavento_valico`. È l'isola del gioco
`propagator_abm` (`data/scenarios/rocca_ventosa`), portata nel TTX con i suoi
dati: terreno, combustibili EU12, edifici, strade, famiglie e i sei **borghi**.
Isola interamente sintetica: nessuna geografia reale.

## Avvio

Nella vista **Scenario** filtra **Isola di Roccavento** e avvia **Roccavento —
il fuoco del valico**. Da terminale:

```sh
cargo run --release -p app -- --scenario data/scenarios/roccavento_valico.pack.json
```

## Territorio

- **8 × 8 km**, griglia fisica 400 × 400 a 20 m. Il terreno visuale è a 10 m,
  sottocampionato dai 5 m della sorgente. Quote fino a circa 650 m. Zone 8 × 8
  da 1 km.
- **Mare.** Nella sorgente il mare è piatto a 0 m. Qui le celle a 0 m collegate
  al bordo scendono a −3 m: sono senza combustibile, quindi il fuoco non cambia,
  e il 3D disegna il mare.
- **392 edifici e 1.521 residenti**, ricavati dalle famiglie di
  `population.json`. Il TTX non modella le persone una per una: dell'ABM
  (percezione del rischio, canali di allerta, intenzioni) restano solo i numeri
  di occupanti.
- **66 strade.** Le strade carrabili non escono dall'isola: senza un'area di
  raccolta, un rifugio aperto o il porto non c'è dove andare.
- **Luoghi.** I sei borghi, i servizi con nomi univoci («Chiesa di San Rocco»,
  «Area di attesa di Fornaci»…), le aree di raccolta, i due presidi AIB, la
  Rocca, la Torre costiera e il Valico (punto più alto della Strada del Passo).
- **Non portati:** idranti e specchi d'acqua (nel TTX non c'è un modello di
  rifornimento idrico), la copertura del suolo a 5 m, il porto, la rocca e gli
  scogli come modelli 3D.

## Borghi

I borghi sono le località che la sorgente assegna a ogni famiglia, comprese le
case sparse (Le Ghiande ha tre nuclei, ma resta un solo borgo). Nel territorio
ogni borgo è:

- un **settore**: ogni edificio ha il settore del suo borgo, quindi l'impatto
  si conta per borgo;
- un **luogo** con le sue case;
- un **`District`** (`Scenario::districts`), con l'area di raccolta.

| Borgo | Case | Residenti | Area di raccolta (dalla sorgente) |
|---|---:|---:|---|
| Castelvento | 97 | 535 | Pianoro di nord-ovest (900 posti) |
| Pian dei Grilli | 88 | 256 | Incrocio con la Costiera (300) |
| Fornaci | 68 | 259 | Pianoro di nord-ovest |
| San Rocco | 78 | 273 | Porto di Roccavento (300) |
| Le Terrazze | 31 | 93 | Area di attesa di Castelvento (500) |
| Le Ghiande | 33 | 105 | nessuna: la più vicina raggiungibile |

Nel gioco le aree di raccolta non hanno una capienza: le capienze qui sopra
sono ipotesi di esercitazione e coprono i borghi serviti. Le aree di attesa
dentro i borghi hanno 150 posti: sono vicine alle case, non per forza sicure.

### Evacua borgo (`EvacuateDistrict`)

`{"type":"EvacuateDistrict","district":"Le Terrazze"}`, con `destination`
facoltativa. Nella console EXCON è lo strumento **Evacua borgo**: si clicca una
casa del borgo. Regole, dalla meccanica dei borghi del gioco:

- Tutte le case occupate del borgo, case sparse comprese.
- **Ritmo** (`Scenario::district_evacuation`, a Roccavento i valori del gioco):
  preparazione di `prep_min` = 5 minuti, poi al massimo `per_min` = 6 persone
  al minuto. Partono prima le case più vicine al punto di partenza, cioè il
  punto della rete stradale più vicino al centro delle case. Un edificio senza
  auto (scuola, ospedale) aspetta 10 minuti in più e richiede autobus, come
  per le altre evacuazioni.
- **Destinazione:** senza `destination` si va all'area di raccolta del borgo,
  che **non va aperta** con `OpenShelter`. Se è irraggiungibile o minacciata
  dal fuoco, si va alla più vicina raggiungibile tra le altre aree di raccolta,
  i rifugi aperti e le uscite della mappa. Con `destination` vale il rifugio
  indicato, come per le altre evacuazioni.
- Poi le auto seguono il modello di traffico esistente: code, strade chiuse
  dal fuoco o dalla polizia, deviazioni. Senza percorso un'auto non parte: chi
  è ancora in casa resta lì (autoprotezione) e si riprova ogni minuto.
- **Più borghi possono evacuare insieme**: la regola del gioco «un borgo alla
  volta» non è stata portata (scelta del 2026-10-29). La congestione la fa il
  traffico.
- Un ordine di evacuazione di zona o di edificio che nomina un borgo diventa
  «Evacua borgo» (`reconcile_command`, con nota all'EXCON).
- L'AI lo trova nel catalogo dei comandi dei ruoli che lo hanno, qui
  `comune`.

Il ritmo di 6 persone al minuto è stato calibrato nel gioco per un giocatore
solo, perché «evacua tutto» non dominasse. Nel TTX vale come ipotesi: va
rivisto se cambiano tempi o scala.

## Ruoli e risorse

| Ruolo | Risorse | Azioni |
|---|---|---|
| Comune di Roccavento (`comune`) | 2 autobus, 2 pattuglie | Evacua borgo, zona o edificio, avvisi, rifugi, strade |
| Direzione delle operazioni di spegnimento (`dos`) | 3 autobotti (Castelvento), 2 squadre a terra (Pian dei Grilli), 1 drone | Linee, lanci, autobotti, drone, ricognizione |
| Sala operativa provinciale (`sala_operativa`) | — | Rinforzi, avvisi, rifugi |

Le cinque squadre del gioco (3 a Castelvento, 2 a Pian dei Grilli) diventano
autobotti e squadre a terra. Tutte partono dalla base unica del TTX, il
Presidio AIB di Castelvento. Rinforzi dal continente: 1 mezzo aereo in 45
minuti, 2 squadre e 1 autobotte in 90 minuti (traghetto), 2 autobus in 60.

## Traccia del pack

Il pack riproduce il caso «medium» del gioco, cioè il fuoco della demo.

| Ora / inject | Contenuto |
|---|---|
| 13:00 (azione anticipata di RV-01) | Innesco nella pineta sopra Le Terrazze (5590, 4370), raggio 60 m; vento da 119° a 36 km/h, umidità 5% |
| 13:30, RV-01 (112) | Segnalazione tardiva, stima approssimata (±400 m) |
| 13:31, RV-02 (meteo) | Annuncio della rotazione; alle 13:45 il vento gira da 50° (azione con ritardo) |
| 13:50, RV-03 (squadre) | Rilievo esatto delle zone in fiamme e vicine (dos, sala) |
| 14:00, RV-04 | Le Ghiande non vuole partire, Le Terrazze chiede dove andare (comune) |
| 14:30, RV-05 (Capitaneria) | Traghetto: rinforzi oppure 300 posti per San Rocco |
| 15:00, RV-06 (radio) | Comunicazione pubblica; stima satellitare |
| 15:30, RV-07 | Quadro consolidato a tutti |

## Calibrazione

`scripts/lab/roccavento.json` riproduce i quattro casi del gioco senza ordini
(semi 1–3, 32 realizzazioni, soglia 0,5, modello operativo):

| Caso del gioco | Innesco, vento, umidità, durata | ha bruciati (semi 1–3) | Case colpite senza ordini, per borgo |
|---|---|---:|---|
| `medium` (questo pack) | (5590, 4370); 36 km/h da 119°, poi 50° a 45 min; 5%; 300 min | 76 / 85 / 71 | Le Terrazze 31/31, Le Ghiande 33/33, Castelvento 50–54, Fornaci 13–15 |
| `impatti_valico_due_lati_30_6` | (5590, 4370); 30 km/h da 119°, poi 50° a 75 min; 6%; 240 min | 74 / 67 / 80 | Le Terrazze 31, Le Ghiande 33, Castelvento 92–94, Fornaci 7–12 |
| `impatti_nuclei_valico_costante_lungo_28_7_0_60` | (5590, 4370); 28 km/h da 119°, costante; 7%; 300 min | 104 / 112 / 108 | Le Terrazze 31, Castelvento 97/97, Fornaci 67/67 |
| `impatti_nuclei_est_giro_lungo_28_6_75_60` | (5870, 4750); 28 km/h da 119°, poi 50° a 75 min; 6%; 300 min | 112 / 115 / 112 | Castelvento 97, Le Terrazze 31, Le Ghiande 33, San Rocco 55–57, Fornaci 13–19 |

«Colpite» = danneggiate o distrutte nel TTX. I borghi colpiti coincidono in
gran parte con quelli del gioco (`docs/02-TERRITORIO.md` della sorgente). Le
case colpite, però, sono molte di più: nel gioco le case non bruciano e conta
l'esposizione alla porta, mentre il TTX applica il suo modello di danno agli
edifici. Pian dei Grilli non viene mai raggiunto in 5 ore. Nessun parametro è
stato ritoccato per avvicinare gli esiti: va deciso se il danno agli edifici
del TTX è troppo severo per questa scala, con esperimenti a coppie prima di
cambiarlo.

## Rigenerazione

```sh
python3 scripts/scenario_factory.py roccavento   # --source per un'altra copia del pacchetto
scripts/lab/run.sh scripts/lab/roccavento.json NUOVA_CARTELLA
```

La ricetta (`scripts/factory/roccavento.py`) importa il pacchetto senza
modificarlo, con il convertitore Rust `import_factory`. Il manifesto
`roccavento_island.manifest.json` registra gli hash dei file sorgente.
