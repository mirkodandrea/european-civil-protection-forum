# Laboratorio operativo TTX

Il laboratorio esegue il vero `sim::World`: stessi comandi, autorità, risorse, routing,
traffico e incendio del TTX, senza renderer o LLM. È uno strumento per calibrazione e
debrief; non produce voti dei partecipanti o certificazioni operative.

## Esecuzione

Dalla radice del repository:

```sh
scripts/lab/run.sh scripts/lab/smoke.json /tmp/ttx-smoke
scripts/lab/run.sh scripts/lab/valley.json /tmp/ttx-valley
```

Ogni destinazione deve essere nuova, per conservare le evidenze. Il percorso della
suite può essere assoluto; i percorsi degli scenari sono relativi al file della suite.
Lo smoke dura 120 secondi simulati con un seme e una realizzazione: verifica il
percorso tecnico, non l'efficacia delle strategie. La suite completa dura due ore
simulate, usa tre semi e otto realizzazioni; per studi confermativi aumentare semi e
realizzazioni e misurare il costo di calcolo.

`valley.json` confronta autonomia locale disattivata/attivata mantenendo invariati gli
altri meccanismi operativi. Comprende attacco precoce, difesa strutture, evacuazione
precoce/tardiva, piano combinato, priorità invertite e due motopompe coordinate.
I copioni evacuano le abitazioni singolarmente; per la scuola richiedono un autobus
aggiuntivo e aspettano il rinforzo: i due autobus iniziali non bastano per 120 persone.
`legacy-comparison.json` mantiene un confronto distinto tra modello storico e modello
operativo con gli stessi copioni compatibili. I risultati fra modelli sono differenze
di ipotesi, non il beneficio isolato dell'autonomia.

## Suite e variazioni

Una suite dichiara `scenarios`, `seeds`, `duration_s`, `sample_s`, `realizations`,
`cutoff`, `variants`, `strategies` e facoltativamente `injects`.

```json
{
  "scenarios": [{"name":"valley","path":"../../data/scenarios/synthetic_valley.json"}],
  "seeds": [1,2,3], "duration_s": 7200, "sample_s": 60,
  "realizations": 8, "cutoff": 0.5,
  "variants": [{"name":"autonomy_off","operational":true,
    "parameters":{"/operations/local_autonomy":false}}],
  "strategies": [{"name":"warning","orders":[
    {"at_s":300,"actor":"mayor","command":{"type":"IssueWarning","zone":"F4"}}
  ]}],
  "injects": []
}
```

`operational:true` applica `Scenario::enable_operational_model()` prima dei parametri.
`parameters` usa JSON Pointer su campi esistenti dello scenario serializzato: un nome
inesistente è errore, non una modifica ignorata. Per esempio
`/tactical_policy/hose_reach_m`, `/roles/1/resources/ground_crews`,
`/weather/wind_speed_kmh` o `/operations/emergency_congestion`. Ogni variante salva
l'intero scenario effettivo, evitando dipendenze da default cambiati in seguito.

Gli ordini sono ordinati stabilmente per `at_s`; a parità di istante gli inject
ambientali precedono gli ordini della strategia. Gli inject sono di `master` e valgono
anche per il riferimento senza interventi. Le strategie possono usare ruoli reali o
`master` per copioni EXCON espliciti. Il runner applica sempre la validazione del core
e conserva i rifiuti; non concede risorse o poteri extra. `AdvanceTime` è vietato nei
copioni: il tempo si esprime solo con `at_s`, entro la durata della prova.

Ogni combinazione scenario × variante × seme esegue un riferimento `no_action`, che
riceve gli stessi inject ambientali. Il confronto della strategia è sempre con quel
riferimento, non con il riferimento di un'altra variante o di un altro seme.

## Evidenze e replay

- `manifest.json`: versione del formato, revisione git/stato modificato, identificatore FNV-1a del binario effettivo, suite originale,
  risultati per seme, differenze dal riferimento e riferimenti alle tracce.
- `scenario-*.json`: scenari effettivi, con raster, parametri e risorse.
- `suite.json`: suite portabile che punta agli scenari salvati, senza riapplicare varianti.
- `run-*.json`: metriche finali, campioni temporali, comandi con esito e registro eventi.
- `metrics.csv`: una riga per scenario, variante, seme e strategia.
- `summary.csv` / `report.md`: sintesi generata da `scripts/lab/report.py`, con intervalli
  osservati e numero di ordini rifiutati; le righe per seme restano l'evidenza primaria.

```sh
cargo run --release --offline -p sim --bin operational_lab -- /tmp/ttx-valley/suite.json /tmp/ttx-replay
python3 scripts/lab/report.py /tmp/ttx-replay
```

Il replay conserva scenario e copioni, non una copia dell'eseguibile. Per ottenere gli
stessi risultati serve la stessa versione del motore; un albero modificato va archiviato
o identificato oltre al commit. Il fingerprint del binario identifica la build effettiva
(non è una firma crittografica e il bundle non archivia il binario). I nomi delle varianti nel replay diventano parte del
nome dello scenario; confrontare le metriche, non i byte dei manifest di due percorsi.

## Significato e limiti delle metriche

I tempi sono in secondi simulati; il runner avanza di un secondo, indipendentemente
dal campionamento dei file. I tempi per stato sommano secondi-unità per task attivi;
non sono durata dell'esercitazione. I dettagli del task vengono aggregati come motivi
di inattività solo quando un task in lavoro non incrementa metri o acqua rispetto al
secondo precedente. La prossimità di task in lavoro entro 60 m è una diagnostica di
sovrapposizione geometrica, non una misura di acqua sprecata o effetto duplicato.

Litri e metri provengono dai task reali, danni ed evacuati da `World::stats`. Le persone
in transito sono i pacchetti partiti ma non arrivati; le persone esposte in transito
sono pacchetti marcati a rischio, anche se successivamente arrivati. Il totale delle
persone già evacuate include l'eventuale percorso storico privo di traffico.
Primo arrivo e primo lavoro utile sono rilevati al secondo successivo al cambiamento
di stato; usare timestamp/eventi del core per analisi sub-secondo.

Non inferire efficacia da uno smoke o da un unico seme. Un beneficio medio può
nascondere un peggioramento in un seme. Una strategia con ordini rifiutati va analizzata
come piano non fattibile prima di valutarne l'efficacia. Evacuazioni ancora in corso
alla fine non sono automaticamente fallimenti. Nessuna significatività statistica è
attribuita agli intervalli del report.

## Verifica esplorativa del 2026-10-20

Eseguiti 18 run di un'ora, tre semi, due realizzazioni, due varianti di autonomia e
riferimento/strategia combinata/motopompe. Nessun ordine rifiutato. Evidenze conservate
in [lab-results/2026-10-20/report.md](lab-results/2026-10-20/report.md), con suite,
metriche per seme e identificazione della build; è una verifica esplorativa ridotta,
non l'intera suite a otto realizzazioni.

La strategia combinata ha evacuato 251 persone in ogni seme e risparmiato da 0 a
0,16 ha. In questo scenario non è emersa differenza tra autonomia spenta/accesa.
I mezzi schierati non hanno consumato acqua: entrambi i task hanno trovato la strada
interrotta dal fuoco dopo 30 secondi. È un esito del piano e della rete, non un
miglioramento da attribuire alla politica. Servono scenari dedicati per misurare il
valore della riallocazione locale. Lo smoke separato ha verificato sei record di
metriche identici su sei nel replay salvato.

## Scenari sintetici degli algoritmi

```sh
python3 scripts/lab/generate_cases.py runs/lab-fixtures-new
scripts/lab/run.sh runs/lab-fixtures-new/suite.json runs/lab-results-new
scripts/lab/run.sh runs/lab-fixtures-new/resume-suite.json runs/lab-resume-new
```

Il generatore usa solo la libreria standard Python e il formato Scenario reale:
pianura, pendio, crinale, accesso unico e vento che cambia. Crea reticoli di 640 m
a celle di 20 m, edifici con occupanti aggregati, strade e base esplicite. Sono
fixture dichiarate, non un territorio operativo calibrato. La variante `single_road`
riceve un secondo innesco EXCON e quella `wind_shift` due cambi di vento; gli stessi
inject sono applicati al riferimento senza azioni. Non compaiono risorse nascoste.

Eseguite 60 corse (5 casi × 2 politiche × 3 semi × riferimento/mezzi), più 12 corse
dedicate all'interruttore di ripresa; **zero ordini rifiutati**. Le motopompe hanno
effettivamente erogato acqua e le politiche hanno registrato ricollocamenti.
Il confronto è misto: in ettari evitati medi, pianura 0,25→0,21; pendio 0,21→0,29;
crinale 0→0,03; accesso unico 0,53→0,51; cambio vento 0,20→0,17. L'autonomia aumenta
il consumo medio d'acqua in quattro casi e non dimostra un beneficio generale.
Nel caso scelto per la ripresa non si sono verificate ritirate: quella suite non
dimostra l'efficacia del rientro in missione; i test di stato coprono la semantica
di sospensione/ripresa e restituzione parziale delle unità.

Le metriche per seme, le ricette, i riepiloghi e l'identificativo della build sono
conservati in [lab-results/2026-10-20-synthetic/](lab-results/2026-10-20-synthetic/main-report.md).
Il replay della suite salvata ha prodotto **60 record di metriche identici su 60**.
Le tracce complete e gli scenari effettivi di questa esecuzione sono in
`runs/lab-synthetic-results/` e `runs/lab-synthetic-resume-results/` (output ignorati
da git); il generatore e i manifest permettono di ricrearle con lo stesso motore.
