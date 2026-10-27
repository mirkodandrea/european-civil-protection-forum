# Selvariva — il festival e i due abitati

Scenario alternativo per **60 minuti reali**, con tre ore simulate (14:00–17:00), tre turni da 60 minuti simulati e otto inject. Tutti i nomi, la geografia e i quantitativi sono di fantasia. Non ricostruisce alcun paese reale.

## Avvio

Nella vista **Scenario**, filtra **Selvariva** e avvia **Selvariva — il festival e i due abitati**. Funziona nel server e nello standalone ricostruito; Valverde rimane disponibile. Da terminale:

```sh
cargo run --release -p app -- --scenario data/scenarios/selvariva_festival.pack.json
```

La base `selvariva_basin` è un territorio senza incendio e senza MSEL. Il pack aggiunge la traccia dell’esercitazione. Avvia l’esercitazione e invia **SV-01**, lasciando selezionati i due inneschi: il fuoco nasce all’invio, non al caricamento. Il cambio meteo parte quando invii **SV-04**. Gli altri inject non impongono danni, interruzioni o risultati. Usa «Avanza al prossimo inject» e fermati per le decisioni; 60 minuti reali non sono un timer automatico. Evita di far trascorrere tutta la simulazione durante la discussione.

## Territorio e posta in gioco

Conca boscata di **6 × 6 km**: griglia fisica 300 × 300 a 20 m, terreno visuale a 10 m derivato dallo stesso DEM; quote circa 155–570 m. Vegetazione EU12, radure e versanti diversi, 172 edifici con **1.102 occupanti iniziali**, 27 strade/accessi. Le strade non sono tagliafuoco impliciti.

- **Roverechiare**, nella parte alta, è vicino al Bosco delle Fonti e a Montebruno. Vi si trovano scuola, deposito, piazza di attesa e cascina.
- **Selvariva**, nel fondovalle, ospita municipio e presidio sanitario.
- **Area festival delle Fonti**: 360 occupanti aggregati. Il boato iniziale è una segnalazione narrativa; non esiste una seconda fisica delle esplosioni.
- **Rifugio della scolaresca**: 40 occupanti (37 bambini e 3 accompagnatori), modellato come struttura scolastica, separata dalla scuola del borgo (80 persone). La duplicazione non rappresenta gli stessi individui.
- **Pianalto**, a est, offre accoglienza per 800 persone e la base operativa; il polo dei **Prati**, a sud di Selvariva, offre altri 400 posti. Entrambi vanno aperti. La piazza di Roverechiare ha 120 posti di attesa, ma è vicina al fronte: la capienza non la rende sicura.
- La **Strada delle Fonti** collega festival e borgo al fondovalle; il **Belvedere** offre un’alternativa verso est. Tre uscite raggiungono i margini ovest, est e sud; il sentiero di Montebruno è pedonale.

I collegamenti delle strutture e le uscite sono verificati con il vero grafo di routing. Le chiusure da fuoco e polizia possono cambiare i percorsi durante l’incendio; non promettere un’uscita sempre aperta. Nel laboratorio Pianalto resta una destinazione utilizzabile, ma EXCON deve verificarne stato, capienza e accessi nella sessione corrente.

## Ruoli e risorse

| Ruolo | Risorse iniziali | Decisioni |
|---|---|---|
| Coordinamento comunale | 4 autobus, 3 pattuglie | Avvisi, apertura accoglienza, evacuazione e viabilità |
| Direzione antincendio | 4 squadre, 2 autobotti, 1 mezzo aereo | Priorità di difesa, linee e lanci, tempi di trasferimento |
| Sala di coordinamento | Richiesta di rinforzi | Quadro comune, aiuti esterni e comunicazione |
| Coordinamento assistenza | Iniziative narrative | Famiglie, bisogni specifici, ricongiungimenti e assistenza |

Rinforzi disponibili: 2 autobus in 35 minuti, 2 squadre in 45, 1 autobotte in 50, 1 mezzo aereo in 75, dopo approvazione. Sono tempi simulati, non reali. Il modello non riusa automaticamente gli autobus in navette; scuola e presidio sanitario richiedono mezzi, mentre l’area festival usa il normale modello aggregato di evacuazione. Le esigenze individuali di mobilità e sanitarie restano narrative.

## Conduzione in un’ora

| Minuti reali indicativi | Tempo simulato / inject | Consegna e decisione |
|---|---|---|
| 0–5 | 14:00, prima di SV-01 | Orientamento sulla mappa, ruoli, risorse, distinzione tra stato fisico e narrazione |
| 5–12 | 14:00, SV-01 | Due inneschi dopo il boato: quadro iniziale, allerta e prime priorità |
| 12–18 | 14:15, SV-02 | Ricognizione narrativa, lettura del fronte e richiesta di informazioni; primi ordini approvati |
| 18–26 | 14:35, SV-03 | Scolaresca, festival e bisogni specifici: destinazione, autobus e assistenza |
| 26–34 | 15:00, SV-04 | Rotazione e rinforzo del vento: rivedere accessi, impieghi e difesa del borgo |
| 34–41 | 15:25, SV-05 | Verificare entrambe le strade e il deposito usando lo stato corrente |
| 41–47 | 15:50, SV-06 | Assistenza, responsabilità e rinforzi con tempi residui |
| 47–52 | 16:20, SV-07 | Comunicato pubblico: fatti, notizie non confermate e prossimo aggiornamento |
| 52–60 | 16:50, SV-08; fino alle 17:00 se utile | Passaggio di consegne e debrief di decisioni ed esiti |

La scaletta è flessibile: accorcia la discussione o salta un inject narrativo se serve. Il debrief ha priorità rispetto all’arrivo alle 17:00. Il modello non dichiara spento il fuoco alla conclusione della demo.

## Simulazione e narrazione

Sono fisici: propagazione ensemble, meteo approvato, esposizione di edifici, percorsi e segmenti interrotti, viaggio e lavoro delle risorse, acqua, traffico aggregato, occupanti evacuati/in viaggio/arrivati e capienza. Gli ordini dei player richiedono approvazione EXCON.

Restano nella narrazione: causa del boato, ricerca di dispersi, droni e sopralluoghi, triage e assistenza individuale, pasti, sostegno psicologico, ricongiungimenti, contenuto pericoloso del deposito, voci e gestione dei media. I player inviano iniziative narrative e EXCON ne registra l’esito. Un esito narrativo non sposta persone o mezzi nel modello.

Per il debrief chiedi: quali informazioni hanno cambiato una decisione? Quali destinazioni erano effettivamente aperte e raggiungibili? Chi era ancora in viaggio? Quali interventi erano solo richiesti, quali arrivati? Quali bisogni sono stati gestiti nella narrazione e con quali responsabilità?

## Riproduzione e controlli

```sh
OPENBLAS_NUM_THREADS=1 python3 scripts/scenario_factory.py selvariva
scripts/lab/run.sh scripts/lab/selvariva.json runs/selvariva-nuova-prova
cargo test --release -p sim --test selvariva
cargo test --release -p scenario --features embedded
```

La ricetta è `scripts/factory/selvariva.py`, seme 33; usa i kernel Factory già presenti, con provenienza EUPL-1.2 conservata in `scripts/factory/provenance.json`. Non importa geodati. Il manifest registra ricetta, parametri, quote, pendenze e hash di template, territorio e pack. Il grading della rete ora accetta la dimensione della cella e ricava l’altezza del territorio, mantenendo i valori predefiniti di Valverde.

Il laboratorio applica gli stessi inneschi e lo stesso meteo in entrambi i bracci. Confronta nessun intervento con una risposta dopo 10–20 minuti simulati: apertura Pianalto, allerta, evacuazione festival/rifugio/scuola, lancio, autobotte e linea. Usa veri ordini di ruolo, risorse limitate e tempi di viaggio; non modifica direttamente danni o popolazione. È una verifica tecnica di fattibilità, non il piano obbligatorio per i partecipanti. Non combinare un’evacuazione di zona con strutture già assegnate a missioni ancora attive: il modello rifiuta la sovrapposizione. Un’autobotte deve raggiungere un bersaglio compatibile con la strada e la lunghezza della manichetta; una casa vicina a una strada disegnata non garantisce che il percorso scelto termini abbastanza vicino.

Le fixture in `scripts/fixtures/selvariva.json` e `scripts/qa/territories.cjs` verificano rispettivamente il 3D invisibile con ordini approvati e la libreria offline con cambio di territorio e ripristino. I test sul territorio controllano strutture/uscite raggiungibili, ingombri, combustibile nelle celle degli edifici, arrivo degli evacuati e rollback.


### Esito della verifica del 27 ottobre 2026

Ensemble di 32 realizzazioni, soglia 0,5, semi appaiati 1/2/3, orizzonte 180 minuti. Territorio SHA-256 `5dcb876766b084704b69b56b671d398f77bd1f96b0488154749c68db8aa1196d`.

| Seme | Nessun intervento, ha | Risposta ritardata, ha | Persone arrivate a Pianalto | Ordini rifiutati |
|---|---:|---:|---:|---:|
| 1 | 77,28 | 69,52 | 480 | 0 |
| 2 | 77,48 | 70,76 | 480 | 0 |
| 3 | 72,76 | 72,08 | 480 | 0 |

Il piano rende fattibile l’evacuazione dei tre aggregati (festival, rifugio e scuola) e usa realmente l’autobotte, il lancio e la squadra. L’effetto sull’area bruciata varia molto; in queste prove **non evita la distruzione di 61 edifici**. Non è quindi una strategia ottimale di difesa del borgo. La scelta di evacuare altri edifici e di rivedere gli impieghi rimane centrale per i partecipanti. Nessuna significatività statistica o garanzia di esito è dedotta da tre semi.

Evidenza locale: [report del laboratorio](../runs/selvariva-lab-final-terrain/report.md), [metriche per seme](../runs/selvariva-lab-final-terrain/metrics.csv), [snapshot 3D](../runs/selvariva-visual-final/report.html), [verifica browser](../runs/territories-qa-final/manifest.json). Le cartelle `runs/` sono artefatti locali ignorati da Git; ricrearle con i comandi sopra, conservando revisione del simulatore e manifest. Test dell’intero workspace, catalogo incorporato e build WASM server/standalone verificati separatamente.
