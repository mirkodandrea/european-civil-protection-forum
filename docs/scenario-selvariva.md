# Selvariva — crisi coordinata in cinque turni

Pack `selvariva_crisi_integrata`: **3 ruoli**, cinque turni da 30 minuti simulati (16:00–18:30) e dieci inject. Tutti i nomi, la geografia e i quantitativi sono di fantasia; tempi e dotazioni sono ipotesi di esercitazione. Sostituisce il precedente pack `selvariva_festival`.

## Avvio

Nella vista **Scenario**, filtra **Selvariva** e avvia **Selvariva — coordinated crisis in five turns**. I testi del pack (ruoli, risorse, inject, note EXCON) sono in inglese, con l’originale italiano in `edits.translations.it`; i nomi dei luoghi sulla mappa restano quelli del territorio. Funziona nel server e nello standalone ricostruito; Valverde rimane disponibile. Da terminale:

```sh
cargo run --release -p app -- --scenario data/scenarios/selvariva_crisi_integrata.pack.json
```

La base `selvariva_basin` è un territorio senza incendio e senza MSEL. Il pack ridefinisce ruoli, risorse, rinforzi e traccia. Il primo inject (SCS-01, 16:00) è solo il bollettino: il fuoco nasce all’invio di **SCS-02** (16:15), lasciando selezionati i due inneschi. Il cambio meteo parte con **SCS-05**. Gli altri inject non impongono danni, interruzioni o risultati. Usa «Avanza al prossimo inject» e fermati per le decisioni.

## Territorio e posta in gioco

Conca boscata di **6 × 6 km**: griglia fisica 300 × 300 a 20 m, terreno visuale a 10 m derivato dallo stesso DEM; quote circa 155–570 m. Vegetazione EU12, radure e versanti diversi, 172 edifici con **1.102 occupanti iniziali**, 27 strade/accessi. Le strade non sono tagliafuoco impliciti. Griglia delle zone 8 × 8 da 750 m.

- **Roverechiare** (zona D4), nella parte alta, è vicino al Bosco delle Fonti e a Montebruno (C3). Vi si trovano scuola, deposito, piazza di attesa e cascina.
- **Selvariva** (E5), nel fondovalle, ospita municipio e presidio sanitario (28 persone).
- **Area festival delle Fonti** (C3): 360 occupanti aggregati. Il boato iniziale è una segnalazione narrativa; non esiste una fisica delle esplosioni.
- **Rifugio della scolaresca** (C4): 40 occupanti (37 bambini e 3 accompagnatori), separato dalla scuola del borgo (80 persone).
- **Pianalto** (G5), a est, offre accoglienza per 800 persone e la base operativa da cui partono tutte le risorse; il polo dei **Prati** (E6) offre altri 400 posti. Entrambi vanno aperti. La piazza di Roverechiare ha 120 posti di attesa, ma è vicina al fronte.
- La **Strada delle Fonti** collega festival e borgo al fondovalle; il **Belvedere** offre un’alternativa verso est.

## Ruoli e risorse

| Ruolo | Risorse iniziali | Decisioni |
|---|---|---|
| Coordinamento civile e assistenza (`coordinamento_civile`) | 4 autobus, 3 pattuglie | Avvisi, accoglienza, evacuazione, viabilità, assistenza e comunicati (narrativi) |
| Direzione dei soccorsi AIB e VVF (`direzione_soccorso`) | 4 squadre, 2 autobotti, **1 drone**, nessun mezzo aereo | Linee, autobotti, lanci (dopo rinforzo), monitoraggio con il drone, ricognizione a terra con una squadra (`Reconnoitre`) |
| Sala operativa e previsione (`sala_operativa`) | — | Richiesta rinforzi, quadro comune, SITREP, raccordo nazionale/ERCC (narrativo) |

Rinforzi, dopo approvazione della richiesta della sala: 2 squadre in 35 minuti e 1 mezzo aereo in 45 (alla direzione dei soccorsi), 2 autobus in 30 (al coordinamento civile).

**Drone** (`MonitorArea`): fino a 4 zone contigue, un rilievo ogni 5 minuti per 40 minuti in posizione, poi 15 minuti di rientro e ricarica a Pianalto; limite di vento 45 km/h, quindi resta operativo con il vento di SCS-05 (38 km/h) e viene richiamato solo se EXCON alza il vento oltre il limite. I rilievi aggiornano solo la mappa della direzione dei soccorsi: gli altri ruoli li vedono se vengono condivisi.

## Visibilità della mappa

Nessun ruolo vede il fuoco all’inizio. Le informazioni arrivano dagli inject (azioni `reveal`), dal drone e da ciò che i ruoli si scambiano:

| Inject | Destinatari | Rivelazione |
|---|---|---|
| SCS-02 (16:15, 112) | tutti | stima approssimata del fuoco, ±500 m, area ±40 % |
| SCS-03 (16:35, polizia locale) | civile, soccorsi | strade ed edifici esatti in C3, C4, D3, D4 (senza fuoco) |
| SCS-06 (17:20, squadre) | tutti | strade ed edifici delle zone in fiamme e adiacenti |
| SCS-08 (17:50, satellite) | tutti | stima approssimata, ±300 m, area ±30 % |
| SCS-10 (18:20, sala) | tutti | quadro consolidato: fuoco, edifici, strade, linee, unità delle zone in fiamme e adiacenti |

Tra 16:15 e 17:20 il fronte si conosce solo dalla stima iniziale, dal drone e dalle condivisioni: è voluto. Il pulsante «Aggiorna mappe dei ruoli» resta uno strumento manuale di EXCON. Le stime approssimate leggono il fuoco reale: se EXCON disattiva gli inneschi di SCS-02 deve disattivare anche la stima, e le rivelazioni «zone in fiamme» falliscono se non c’è fuoco.

## Conduzione

| Ora simulata / inject | Consegna e decisione |
|---|---|
| 16:00, SCS-01 | Bollettino di pericolo: misure preventive, destinatari, primo aggiornamento |
| 16:15, SCS-02 | Due focolai, stima incerta: coordinamento sul posto, prime priorità, uso del drone |
| 16:35, SCS-03 | Festival, rifugio, scuola, presidio: destinazioni, autobus, accessi |
| 16:50, SCS-04 | Rinforzi disponibili: richieste motivate con luogo e orario utile |
| 17:05, SCS-05 | Vento da ONO a 38 km/h: rivedere piano, accessi e copertura del drone |
| 17:20, SCS-06 | Fumo sulla Strada delle Fonti e al deposito: viabilità, strutture, gestori (narrativi) |
| 17:35, SCS-07 | Bisogni specifici e richieste di contatto (narrativi) |
| 17:50, SCS-08 | Punto di situazione nazionale, rilevamento satellitare, ERCC |
| 18:05, SCS-09 | Stampa e voci: comunicare incertezza e condizioni di rientro |
| 18:20, SCS-10 | Consegne al turno subentrante, quadro consolidato |

La fine della traccia non equivale a incendio spento. Per il debrief: quali informazioni (e di che ora) hanno cambiato una decisione? Quando è stato usato il drone e chi ha ricevuto i suoi rilievi? Quali destinazioni erano aperte e raggiungibili? Quali interventi erano solo richiesti e quali arrivati?

## Simulazione e narrazione

Sono fisici: propagazione ensemble, meteo approvato, esposizione di edifici, percorsi e segmenti interrotti, viaggio e lavoro delle risorse, drone, acqua, traffico aggregato, occupanti evacuati/in viaggio/arrivati e capienza. Gli ordini dei player richiedono approvazione EXCON.

Restano nella narrazione: causa del boato, ricerca di dispersi, sopralluoghi a terra, gestori elettrico/idrico, triage e assistenza individuale, ricongiungimenti, contenuto del deposito, voci e media, raccordo nazionale ed ERCC.

## Riproduzione e controlli

```sh
OPENBLAS_NUM_THREADS=1 python3 scripts/scenario_factory.py selvariva
scripts/lab/run.sh scripts/lab/selvariva.json runs/selvariva-nuova-prova
cargo test --release -p sim --test selvariva
cargo test --release -p exercise --test selvariva_crisi
cargo test --release -p scenario --features embedded
```

La ricetta è `scripts/factory/selvariva.py`, seme 33; genera solo il territorio (il pack è scritto a mano e il manifest ne registra l’hash); usa i kernel Factory già presenti, con provenienza EUPL-1.2 conservata in `scripts/factory/provenance.json`. Non importa geodati. Il manifest registra ricetta, parametri, quote, pendenze e hash di template, territorio e pack. Il grading della rete ora accetta la dimensione della cella e ricava l’altezza del territorio, mantenendo i valori predefiniti di Valverde.

Il laboratorio applica gli stessi inneschi e lo stesso meteo in entrambi i bracci. Confronta nessun intervento con una risposta dopo 10–20 minuti simulati: apertura Pianalto, allerta, evacuazione festival/rifugio/scuola, lancio, autobotte e linea. Usa veri ordini di ruolo, risorse limitate e tempi di viaggio; non modifica direttamente danni o popolazione. È una verifica tecnica di fattibilità, non il piano obbligatorio per i partecipanti. Non combinare un’evacuazione di zona con strutture già assegnate a missioni ancora attive: il modello rifiuta la sovrapposizione. Un’autobotte deve raggiungere un bersaglio compatibile con la strada e la lunghezza della manichetta; una casa vicina a una strada disegnata non garantisce che il percorso scelto termini abbastanza vicino.

Le fixture in `scripts/fixtures/selvariva.json` e `scripts/qa/territories.cjs` verificano rispettivamente il 3D invisibile con ordini approvati e la libreria offline con cambio di territorio e ripristino. I test sul territorio controllano strutture/uscite raggiungibili, ingombri, combustibile nelle celle degli edifici, arrivo degli evacuati e rollback.


### Esito della verifica del 27 ottobre 2026 (pack precedente)

Ensemble di 32 realizzazioni, soglia 0,5, semi appaiati 1/2/3, orizzonte 180 minuti. Territorio SHA-256 `5dcb876766b084704b69b56b671d398f77bd1f96b0488154749c68db8aa1196d`.

| Seme | Nessun intervento, ha | Risposta ritardata, ha | Persone arrivate a Pianalto | Ordini rifiutati |
|---|---:|---:|---:|---:|
| 1 | 77,28 | 69,52 | 480 | 0 |
| 2 | 77,48 | 70,76 | 480 | 0 |
| 3 | 72,76 | 72,08 | 480 | 0 |

Il piano rende fattibile l’evacuazione dei tre aggregati (festival, rifugio e scuola) e usa realmente l’autobotte, il lancio e la squadra. L’effetto sull’area bruciata varia molto; in queste prove **non evita la distruzione di 61 edifici**. Non è quindi una strategia ottimale di difesa del borgo. La scelta di evacuare altri edifici e di rivedere gli impieghi rimane centrale per i partecipanti. Nessuna significatività statistica o garanzia di esito è dedotta da tre semi.

Evidenza locale: [report del laboratorio](../runs/selvariva-lab-final-terrain/report.md), [metriche per seme](../runs/selvariva-lab-final-terrain/metrics.csv), [snapshot 3D](../runs/selvariva-visual-final/report.html), [verifica browser](../runs/territories-qa-final/manifest.json). Le cartelle `runs/` sono artefatti locali ignorati da Git; ricrearle con i comandi sopra, conservando revisione del simulatore e manifest. Test dell’intero workspace, catalogo incorporato e build WASM server/standalone verificati separatamente.
