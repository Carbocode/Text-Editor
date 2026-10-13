# Developer Tools: osservabilità, diagnostica e analisi persistente

Stato: proposta di architettura e piano di implementazione. Nessuna delle API, variabili di ambiente o funzionalità descritte qui è ancora implementata.

## Obiettivo

Realizzare una modalità sviluppatore con una console grafica cross platform, sul modello dei DevTools dei browser, per osservare l'intera applicazione: frontend, core Rust, confini FFI, worker, code, filesystem, watcher, storage e aggiornamenti della UI.

Le registrazioni devono essere persistenti, consultabili dopo il riavvio, confrontabili ed esportabili. La stessa console deve poter analizzare sia un'app in esecuzione sia un pacchetto diagnostico offline.

La console ha una UI unica per macOS, GNOME e KDE, indipendente dallo stile del sistema operativo. I tre frontend mantengono piccoli adattatori di strumentazione; pannelli, protocollo e strumenti di analisi sono condivisi.

## Situazione attuale verificata

- Il core Rust è incorporato nei frontend: non esiste oggi un backend remoto centrale.
- SwiftUI comunica tramite UniFFI; GTK chiama Rust direttamente; Qt/QML usa CXX-Qt.
- Il core gestisce comandi, eventi transitori, stato autorevole e snapshot. Gli `AppEvent` non sono un registro persistente.
- La ricerca usa un worker `std::thread`, canali `std::sync::mpsc`, debounce di 150 ms e generazioni per invalidare risultati obsoleti. Non usa Tokio.
- Il watcher usa `notify`, non Watchman. Il core riconcilia ogni 30 secondi oppure ogni 2 secondi se il watcher non parte.
- Nel codice attuale macOS controlla il filesystem ogni secondo e i risultati di ricerca ogni 250 ms; GTK e Qt controllano entrambi ogni 250 ms. Le differenze devono essere visibili nelle registrazioni.
- Su macOS la minimappa viene preparata in background e applicata sul main thread; il bridge dell'applicazione usa un mutex.
- SQLite conserva già metadati dei workspace recenti, ma non dati di osservabilità.

Riferimenti: [architettura](../architecture.md), [sviluppo](../development.md), [core](../../crates/application/src/lib.rs), [filesystem](../../crates/filesystem/src/lib.rs), [bridge Swift](../../crates/ffi-swift/src/lib.rs).

## Decisioni di base

1. Console desktop separata in Rust con `egui` + `eframe`: scelta concordata per la UI comune alle piattaforme. Il prototipo valida le dipendenze, le prestazioni e l’usabilità dello stack scelto. Il browser non è un requisito.
2. Crate Rust condiviso per strumentazione e raccolta, con `tracing` e adattatori dei frontend.
3. Servizio diagnostico separato dall'app osservata, responsabile di persistenza, interrogazione e distribuzione della console. Non è il backend funzionale dell'editor.
4. DuckDB embedded nel servizio diagnostico, dedicato alle analitiche e distinto dallo SQLite dei workspace recenti. Scelta concordata; i benchmark servono a dimensionare e validare la soluzione.
5. Protocollo versionato per sessioni, eventi, comandi diagnostici e interrogazioni; formato di archivio versionato per import/export.
6. Raccolta locale e funzionamento offline. OpenTelemetry e backend esterni sono integrazioni successive, non prerequisiti della console.
7. Ogni limite, perdita di dati, ritardo o funzionalità non disponibile deve essere esplicito nella UI.

`egui` + `eframe` sono lo stack scelto per la console condivisa, in una finestra e un processo autonomi. Un prototipo verifica costo del rendering, virtualizzazione, accessibilità e gestione di registrazioni grandi prima dell'implementazione dei pannelli completi.

## Requisito prioritario: indipendenza del funzionamento dell'editor

L'editor deve continuare a funzionare correttamente con raccolta disattivata, servizio assente, UI chiusa, collector bloccato, archivio guasto o code sature. Un problema diagnostico non cambia l'esito di apertura, modifica, ricerca o salvataggio e non impone attese all'utente.

L'osservazione attiva ha comunque un costo: timestamp, registrazione degli eventi e trasferimento dei dati consumano risorse. Anche processi separati competono per CPU, GPU, RAM e disco. Il requisito è indipendenza funzionale e overhead limitato e verificato, non una promessa di impatto fisico nullo.

Vincoli obbligatori di implementazione:

- DevTools disattivati per default; flag di build per escludere del tutto il producer nei build che non lo richiedono. Quando presente ma disabilitato, nessun campionamento, trasporto o formattazione degli attributi: controllare il filtro prima del lavoro costoso.
- Sul percorso dell'editor soltanto operazioni brevi con payload limitato e accodamento non bloccante. Non attendere ACK, I/O, connessioni o spazio nella coda. Nessun lock conteso con writer, query o GUI; usare strutture appropriate e verificare allocazioni/contenzione nel benchmark.
- Redazione, serializzazione, compressione e trasferimento sul thread di trasporto; campionamento di processo nel collector quando possibile, senza chiamate OS pesanti sul main thread.
- Non clonare snapshot/documenti completi per la telemetria. Emissione di metadati e delta piccoli; checkpoint diagnostici filtrati con budget, evitando scansioni o copie estese mentre si detiene il lock dell'applicazione.
- Nessun hook diagnostico modifica comandi, ordine degli eventi, debounce, generazioni o politica di cancellazione. La pianificazione può essere perturbata dall'osservazione; verificarne il costo senza promettere scheduling identico.
- Per ogni buffer fissare limiti di byte, numero di record e dimensione del singolo evento. Saturazione → sampling/scarto e contatore di gap; mai backpressure verso l'editor. Tagliare gli attributi eccessivi con indicazione esplicita.
- Servizio e GUI in processi distinti dall'editor e fra loro. Un crash o blocco della GUI non interrompe la raccolta nel servizio; un guasto del servizio disabilita/degrada la raccolta senza impedire l'uso dell'editor.
- Limitare concorrenza query, thread DuckDB, memoria e spill; export, compattazione e query ampie hanno budget/cancellazione. Priorità OS ridotta dove disponibile, senza considerarla una garanzia di isolamento.
- Ridurre aggiornamenti live e campionamento quando il budget è superato; conservare il motivo della degradazione. In caso di risorse insufficienti rinunciare ai dati diagnostici prima di rallentare intenzionalmente l'editor.
- GUI senza loop di repaint continuo a vuoto. Aggiornamenti live aggregati e con frequenza limitata; finestra nascosta/minimizzata riduce il rendering, mantenendo la raccolta indipendente.

La console mostra il costo del producer, il costo del servizio, il consumo della propria UI e i gap separatamente. Un limite di memoria interno a DuckDB non sostituisce la misura della memoria totale del processo.

## Stack tecnologico proposto

### Strumentazione dell'editor

- Rust: `tracing` e `tracing-subscriber`, con un layer dedicato che produce il contratto diagnostico comune.
- SwiftUI/AppKit: adattatore Swift via UniFFI per span, eventi e contesto; signpost nativi opzionali per correlazione con Instruments.
- GTK: strumentazione Rust diretta e hook del ciclo eventi GLib.
- Qt/QML: adattatore CXX-Qt e hook espliciti per operazioni/rendering.
- Campionamento di CPU/memoria di processo tramite `sysinfo` nel servizio, validando significato e unità sulle piattaforme supportate; latenza del ciclo eventi misurata dagli adattatori dei toolkit.
- Buffer e thread di trasporto limitati nel producer; nessuna necessità di migrare il worker di ricerca a Tokio.

### Servizio diagnostico

- Rust, eseguibile separato `native-workspace-devtools`.
- Tokio per rete e coordinamento; Axum per API HTTP e WebSocket locali.
- Serde/JSON per envelope e protocollo versionato; crate dei contratti Rust condiviso fra producer, servizio e GUI. Numeri a 64 bit con rappresentazione esplicita nel protocollo e unità documentate.
- App → servizio: HTTP POST autenticato con batch, sequenze e ACK dopo commit. Un canale WebSocket separato inoltra i controlli di raccolta verso l'app; nessun comando nel main thread di rete.
- Console → servizio: HTTP per query, registrazioni ed export/import; WebSocket per notifiche live, cursori e stato dei controlli.
- Trasporto su loopback. Batch JSON come punto di partenza; un encoding binario è un'ottimizzazione successiva motivata dai benchmark.

### Storage e analisi

- DuckDB nativo attraverso il crate Rust `duckdb`, con versione del motore e binding fissata nel build.
- Thread writer dedicato, connessione di scrittura e Appender in transazioni esplicite; connessioni di lettura nello stesso processo su un pool limitato di thread bloccanti.
- Query SQL parametrizzate definite dal servizio; la console non riceve accesso al database o esecuzione SQL arbitraria.
- Budget espliciti per thread, memoria, directory temporanea e spill su disco: le analisi non devono consumare tutte le risorse dell'editor.
- Parquet per i dati esportati e ZIP per il contenitore `.nwdiag`; checksum SHA-256 e manifest JSON versionato.

### Console grafica

- Rust + `egui` per widget e layout, `eframe` per finestra, input e integrazione desktop, backend `wgpu` proposto per il rendering.
- `egui_extras::TableBuilder` con rendering delle sole righe visibili per log e tabelle; dati caricati a pagine e cache limitate, indipendentemente dalla virtualizzazione grafica.
- `egui_plot` per le serie temporali, con riduzione dei punti lato query; timeline personalizzata con il painter di egui, disegnando soltanto elementi/corsie visibili.
- Stato Rust della console per selezione temporale e filtri comuni; worker di rete/query asincroni con canali limitati verso il thread GUI. Nessuna chiamata sincrona al servizio nel frame di rendering.
- Repaint su input o risultati ricevuti, con aggiornamenti live aggregati; evitare repaint perpetuo per polling, animazioni o grafici non visibili.
- Stile egui condiviso e temi indipendenti dai toolkit SwiftUI, GTK e Qt. Verificare navigazione da tastiera, clipboard, font/DPI, screen reader, Wayland/X11 e macOS nel prototipo.
- Un eseguibile GUI separato `native-workspace-devtools-ui` si collega al servizio; non carica il renderer nell'editor e non apre il database live direttamente.

La UI concordata è egui/eframe; non è previsto un prototipo Iced parallelo. La scelta della UI non implica automaticamente migliori prestazioni: misurare consumo idle, memoria, costo dei frame e interazione su dataset realistici. La UI web resta un'opzione futura che può riusare le API, non un secondo frontend da implementare nell'MVP.

### Valutazione dei toolkit UI

Valutazione delle opzioni pertinenti al progetto, conservata come motivazione della scelta concordata di egui/eframe. Le alternative non fanno parte del piano di implementazione. Il confronto è basato su documentazione e funzionalità disponibili, non su un benchmark già eseguito. Priorità: tabelle/log grandi, timeline personalizzata, pannelli spostabili, costo idle, accessibilità, macOS/Wayland/X11, distribuzione e manutenzione. Non dedurre prestazioni dal solo linguaggio, dal rendering GPU o dal modello immediate/retained.

**egui + eframe — stack scelto per questi DevTools.** UI in Rust, rendering personalizzabile e componenti direttamente utili: `egui_extras` per righe visibili, `egui_plot` per grafici e `egui_dock` per pannelli agganciabili. È la combinazione più aderente al nostro strumento fra quelle valutate, a giudizio progettuale. Restano da verificare compatibilità delle versioni dei crate, accessibilità, selezione/copia del testo e repaint durante il live. Il codice UI immediate mode viene eseguito a ogni frame richiesto: lavoro proporzionale ai dati visibili e cache sono necessari. [egui](https://github.com/emilk/egui), [tabelle](https://docs.rs/egui_extras/latest/egui_extras/struct.TableBody.html), [docking](https://docs.rs/egui_dock/latest/egui_dock/).

**Iced — seconda scelta, pienamente valida.** Architettura esplicita di stato, messaggi, aggiornamento e vista, utile per navigazione, filtri e lifecycle delle registrazioni. Il Canvas consente timeline/grafici interattivi e cache geometriche. Un semplice `Scrollable` non è una strategia sufficiente per log grandi: paginazione e virtualizzazione vanno progettate e verificate; docking, timeline e interazioni complesse richiedono componenti scelti o costruiti appositamente. Iced non è incluso nel piano di implementazione concordato. Non affermare che Iced sia necessariamente più veloce, più lento o più accessibile di egui. [architettura](https://book.iced.rs/architecture.html), [Canvas](https://docs.rs/iced/latest/iced/widget/canvas/index.html).

**Slint — alternativa dichiarativa.** Integrazione con Rust e `ListView` che istanzia gli elementi visibili. Interessante per una UI desktop curata; valutare quanto lavoro richiedono una timeline densa, docking e interazioni specialistiche. Introduce il linguaggio `.slint` e richiede verifica delle condizioni della licenza scelta. Non scartarlo per prestazioni presunte. [ListView](https://docs.slint.dev/latest/docs/slint/reference/std-widgets/views/listview/), [progetto e licenze](https://slint.dev/).

**Qt con bridge Rust — alternativa basata sul toolkit già presente.** Model/view e componenti desktop sono pertinenti a tabelle e navigazione. Un solo frontend Qt per i DevTools può essere distribuito anche su macOS e GNOME: non occorrono tre implementazioni. Richiede però distribuire Qt per tutti gli utenti sviluppatori, mantenere il bridge e verificare licenze/packaging. Preferirlo se il riuso concreto dei componenti Qt già disponibili supera il vantaggio di una UI interamente Rust. [Qt model/view](https://doc.qt.io/qt-6/model-view-programming.html).

**GTK4 con gtk-rs — fattibile, meno aderente alla distribuzione prevista.** `ListView` riutilizza widget per le porzioni visibili; il progetto usa già GTK. Per una console comune occorre però distribuire e verificare lo stack GTK anche su macOS. La preferenza per egui/Iced è una scelta di dipendenze e uniformità, non una prova che GTK sia lento. [GTK ListView](https://docs.gtk.org/gtk4/class.ListView.html).

**Floem — candidato reattivo da seguire.** UI Rust con reattività fine e backend grafici; il progetto dichiara ancora maturazione e possibili breaking change prima di v1. Aumenta il rischio di manutenzione rispetto al percorso scelto; valutare solo se offre un vantaggio verificabile per i pannelli richiesti. [Floem](https://github.com/lapce/floem).

**GPUI — candidato per UI specialistiche.** Framework Rust GPU e controllo delle viste/elementi, usato nell'ecosistema Zed. La documentazione segnala sviluppo pre-1.0 e breaking change frequenti; verificare componenti, dipendenze e piattaforme della versione scelta. Non assumerne le prestazioni a partire dalla fluidità di Zed. [GPUI](https://docs.rs/gpui/latest/gpui/).

**Xilem/Masonry — non prioritario per l'MVP.** Framework/architettura Rust con rendering nativo, dichiarati sperimentali. Il vantaggio tecnico potenziale non giustifica da solo aggiungere rischio a uno strumento diagnostico che deve essere affidabile. [Xilem](https://github.com/linebender/xilem).

**Tauri o Dioxus Desktop — finestre dedicate con webview.** Consentono integrazione Rust e UI web; l'esistenza di una finestra autonoma non elimina DOM e motore web. Ecosistema web utile per grafici e accessibilità, ma introduce un ulteriore stack e comportamento da verificare nei motori di piattaforma. Dioxus Desktop qui indica specificamente il renderer webview documentato, non altri backend del progetto. [Tauri](https://v2.tauri.app/concept/architecture/), [Dioxus Desktop](https://dioxuslabs.com/learn/0.7/guides/platforms/desktop/).

**Browser o Electron — opzioni web.** Il browser resta utile per un accesso futuro senza installare la GUI e per export consultabili; non è dimostrato più lento in assoluto. Electron offre un runtime Chromium/Node distribuito e coerente, ma aggiunge dipendenze che non sono necessarie alla proposta Rust locale. Non lo sceglierei per questo MVP in assenza di un vantaggio misurato o di una UI web già da riusare. [Electron: process model](https://www.electronjs.org/docs/latest/tutorial/process-model).

Validare un prototipo egui/eframe con un dataset riproducibile: log con 100 mila e 1 milione di record caricati a pagine, timeline con molte corsie, zoom/selezione, filtri live e sessione offline. Provare GUI idle, live, minimizzata e durante export. Misurare RSS, CPU idle/live, tempi frame p95/p99, latenza di input e comportamento dell'editor osservato, documentando hardware e variabilità. Controllare tastiera, screen reader, clipboard e DPI su macOS e Linux. Questi sono carichi di prova proposti, non capacità garantite dei toolkit.

Il prototipo valida egui/eframe e i componenti specialistici senza alterare il producer per mascherarne il costo. Servizio, DuckDB, protocollo, dati e politiche di aggiornamento restano indipendenti dalla UI. Nessuna UI deve essere integrata nel processo dell'editor.

### Organizzazione proposta nel repository

- `crates/diagnostics-protocol`: record, envelope, versioni e capability, senza dipendenze UI/database.
- `crates/observability`: producer, layer tracing, contesto e buffer di trasporto.
- `crates/diagnostics-storage`: schema DuckDB, writer, query, migrazioni, retention ed export/import.
- `tools/devtools`: processo locale, API e lifecycle.
- `apps/devtools`: console desktop Rust/egui e test UI.

Nomi e dipendenze sono proposti; queste directory non sono ancora create. `cargo xtask devtools` coordinerà build/avvio. La distribuzione include servizio, GUI e DuckDB: nessun browser, Node.js, database server esterno o servizio cloud richiesto.

## Architettura e responsabilità

### 1. Producer nell'app osservata

Il crate proposto `crates/observability` espone span, eventi, metriche e aggiornamenti diagnostici. Gli adattatori Swift, GTK e Qt usano lo stesso contratto.

Ogni processo possiede una propria sessione. Le operazioni attraversano UI, FFI e worker conservando il contesto. La raccolta usa code limitate e invio in batch a un thread di trasporto: nessuna scrittura DuckDB o chiamata di rete bloccante sul main thread.

Un buffer circolare in memoria assorbe picchi e brevi disconnessioni. La sua capacità è limitata. La disponibilità del servizio diagnostico non deve determinare il successo di un comando dell'editor.

### 2. Servizio diagnostico locale

Un eseguibile proposto `native-workspace-devtools` riceve batch dai processi osservati, li valida, li persiste e serve le API alla GUI separata. Può gestire più istanze e più frontend contemporaneamente, mantenendo le sessioni distinte.

Avvio dal menu sviluppatore o da un futuro comando `cargo xtask devtools`. Il launcher riusa un servizio già attivo attraverso un descriptor locale protetto e una verifica di identità/versione; non assume che una porta occupata appartenga al servizio. L'app può partire anche se il servizio fallisce.

Separare il servizio consente di continuare a vedere i dati già ricevuti quando l'editor si blocca o termina. Non garantisce il recupero degli eventi ancora nei buffer dell'editor: la console mostra l'ultimo batch confermato e la sessione incompleta.

### 3. Console grafica

La console parla soltanto con il servizio diagnostico attraverso HTTP per interrogazioni e WebSocket per notifiche e aggiornamenti incrementali. Il flusso live è distinto dal database: chiudere la console non interrompe una registrazione attiva.

La UI interroga finestre temporali e pagine di dati. Non carica un'intera registrazione in memoria. Tabelle virtualizzate, aggregazioni e riduzione dei punti dei grafici avvengono senza perdere l'accesso al dettaglio originale conservato.

### 4. Analisi offline

Il servizio può avviarsi senza alcun editor collegato, aprire il database locale oppure importare un archivio. Nell'analisi offline i controlli sull'app sono disabilitati e tutti i pannelli indicano quali dati sono stati registrati.

## Esperienza della modalità sviluppatore

- Un'azione comune “Apri Developer Tools” in tutti i frontend apre la console e seleziona la sessione corrente.
- Barra superiore con sessione, frontend, versione, connessione, stato della registrazione e intervallo temporale.
- Controlli per iniziare/fermare una registrazione, aggiungere marker, scegliere il livello di dettaglio, esportare e aprire un archivio.
- Una selezione temporale comune aggiorna tutti i pannelli; cliccare un evento porta a trace, log, task e revisioni correlati.
- Filtri per componente, thread, tipo di operazione, livello, esito e durata; ricerca negli attributi e nei log.
- Pausa della visualizzazione live distinta da stop della registrazione. “Svuota vista” non cancella i dati persistiti.
- Navigazione da tastiera, temi chiaro/scuro, unità esplicite, righe virtualizzate e indicazioni accessibili oltre al colore.
- Nessun pannello vuoto ambiguo: distinguere “nessun evento”, “raccolta disabilitata”, “non supportato” e “dati persi”.

## Pannelli

### Overview

Salute della sessione, operazioni recenti, errori, CPU, memoria residente, ritardo del ciclo eventi, lavoro pendente, stato del watcher e stato della raccolta. Collegamenti dai picchi alle operazioni nell'intervallo selezionato.

### Console

Log strutturati con timestamp, livello, componente, thread, trace e attributi. Raggruppamento dei messaggi ripetitivi, filtri, dettaglio degli errori e copia/esportazione delle righe selezionate. Gli errori gestiti, i panic e le disconnessioni hanno categorie distinte.

### Performance e timeline

Tracce per operazione con corsie per main thread, worker, FFI, I/O e aggiornamenti UI. Mostrare separatamente durata totale, attesa di coda, debounce, lavoro attivo e consegna del risultato. Collegare causalmente operazioni asincrone; una barra lunga non significa automaticamente consumo CPU.

Marker, zoom, selezione temporale, dettaglio degli span, durate inclusive/esclusive quando calcolabili e confronto fra registrazioni. L'applicazione di uno snapshot e la presentazione di un frame sono misure differenti: non dichiarare “visibile” un risultato senza un hook verificabile del toolkit.

### Task, thread, code e lock

Job accodati, attivi, completati, cancellati, falliti e scartati. Generazione, causa della cancellazione, tempi di attesa, età del job più vecchio e contatori di invio/ricezione.

Per i lock strumentati: attesa e tempo di possesso. Per i thread: nome, ruolo e attività osservata. Non presentare un elenco dei job strumentati come inventario completo dei thread del sistema. Un lock lento è un indizio; il rilevamento automatico dei deadlock richiede dati aggiuntivi.

### Commands, events e state

Comandi ricevuti, esito, eventi prodotti, revisioni prima/dopo e snapshot applicati da ciascun frontend. Evidenziare no-op, errori e snapshot obsoleti. Consentire ispezione di stato e differenze tra checkpoint.

Il registro diagnostico non cambia la semantica degli `AppEvent` e non diventa lo storage autorevole dell'app. La visualizzazione dello stato passato è ricostruzione dei dati raccolti, non replay deterministico delle operazioni né ripristino dell'editor.

### Filesystem, watcher e storage

Eventi `notify`, numero di eventi aggregati, tempo tra callback e consumo, scansioni di riconciliazione, motivazione della scansione, file esaminati, letture/scritture, errori e conflitti disco/buffer.

Per SQLite applicativo: durata delle operazioni, attesa, transazioni ed errori, senza valori sensibili nei parametri. Lo storage diagnostico ha una sezione separata per dimensioni, scrittura, manutenzione e perdite.

I futuri LSP, terminali e processi esterni potranno aggiungere connessioni, messaggi e lifecycle allo stesso modello. Il pannello si adatta alle capability della sessione; non presuppone Watchman o servizi non presenti.

### Editor e rendering

Input, aggiornamento buffer, syntax highlighting, minimappa, layout e aggiornamento UI. Dimensione del documento, revisioni, generazioni e risultati scartati. Copertura esplicita per toolkit; prima misura comune: ritardo del ciclo eventi e durata dell'applicazione dello snapshot.

### Registrazioni e confronti

Archivio persistente con titolo, marker, note, versione, piattaforma, frontend, durata, dimensione, livello di raccolta ed esito della sessione. Filtri, sessioni fissate, confronto di due registrazioni, export/import e cancellazione esplicita.

Confrontare p50/p95/p99 insieme a numero di campioni, scenario, dimensione workspace e configurazione. Non attribuire una differenza al frontend quando hardware, corpus, cache o livello di raccolta non sono comparabili.

## Contratto dei dati e correlazione

Ogni envelope comprende almeno:

- `schema_version`, `session_id`, `producer_id`, `process_id` e sequenza monotona del producer;
- timestamp monotono relativo alla sessione e timestamp civile per orientamento;
- componente, nome del thread e identificatore del thread quando disponibile;
- tipo, nome, severità/esito e attributi tipizzati;
- `trace_id`, `span_id`, `parent_span_id` e link causali quando pertinenti;
- `operation_id`, job/generazione, workspace/documento pseudonimizzati e revisione quando pertinenti.

Nomi degli span e categorie sono stabili; percorsi, query e identificatori variabili non entrano nei nomi delle metriche. I batch hanno identificatore, range di sequenza, numero di record e dimensione dichiarata.

Una ricerca parte dall'input, attraversa debounce e canale, esecuzione, pubblicazione, polling e aggiornamento UI. Il job trasporta esplicitamente il contesto. Le ricerche superate terminano come cancellate o scartate, con la motivazione. La minimappa segue lo stesso modello con la propria generazione.

Per eventi indipendenti, come una modifica esterna al filesystem, si crea una nuova operazione. Se più eventi causano una scansione, si usano link causali o il riferimento al batch; non si inventa un singolo parent.

Gli adattatori convertono i clock nativi in una base temporale comune con un handshake e registrano l'incertezza. Tra processi differenti la UI non deve suggerire precisione o ordine causale non dimostrabili. Non usare l'orologio civile per misurare durate.

## Persistenza: storage dedicato alle analitiche

### Motore scelto: DuckDB

Il carico comprende due esigenze differenti: acquisizione continua e accesso puntuale per la vista live; scansioni, aggregazioni e confronti per l'analisi storica. Non esiste una graduatoria di velocità valida per entrambe senza misurare dati, query, batch e durabilità.

DuckDB è il motore scelto per l'analisi locale: è progettato per carichi analitici e operazioni in bulk. Nel modello embedded l'accesso read/write risiede nel processo del servizio diagnostico; i frontend e la console passano dalle sue API, senza aprire direttamente il database. Il writer usa l'Appender in batch, evitando una transazione per evento. Verificare anche latenza del dettaglio di una trace e costo degli aggiornamenti di span/stato.

SQLite rimane soltanto nello storage applicativo esistente; non aggiungere un secondo motore per i dati diagnostici nell'MVP.

Validare DuckDB su dataset e dimensioni crescenti: ingestione sostenuta e a raffiche, intervalli di timeline, ricerca di una trace, filtri dei log, aggregazioni/percentili e confronto di sessioni. Eseguire letture mentre la raccolta continua. Misurare throughput, latenza p95/p99, lag fino al commit, CPU/RAM, spazio, export e recupero dopo crash. Includere parsing, serializzazione, indici e manutenzione, non soltanto una `SELECT` isolata.

La vista live usa notifiche e cache limitate per la presentazione; i record confermati sono sempre persistiti in DuckDB e interrogabili dalle API. Se emergono limiti misurati, valutare segmenti storici Parquet interrogati con lo stesso motore. Questa estensione richiede protocollo di compattazione, deduplicazione, watermark dei dati consultabili e recupero dopo interruzioni; non introdurla automaticamente.

ClickHouse è un candidato per una futura raccolta centralizzata con molti processi/macchine e volumi elevati. Non è un requisito dell'installazione locale dei DevTools. L'interfaccia del repository diagnostico deve mantenere separati protocollo/UI e implementazione dello storage, senza imporre SQL identico ai diversi motori.

Query e ingestione restano nel medesimo servizio. Separare connessioni e code di lavoro; non eseguire chiamate DuckDB sincrone sui thread async di Tokio.

### Scelta e isolamento

Usare DuckDB persistente, transazioni in batch e migrazioni versionate. Un writer applicativo serializza l'acquisizione e la manutenzione; più connessioni nello stesso processo servono query concorrenti entro il budget. Non serve un servizio separato per il database.

Directory proposta nell'area dati dell'app: `diagnostics/`, con un database `analytics.duckdb`, una directory `artifacts/` e una directory temporanea con quota. Un override dedicato, proposto come `NATIVE_WORKSPACE_DIAGNOSTICS_DIR`, deve essere indipendente da `NATIVE_WORKSPACE_STATE_DB`.

Il database diagnostico non contiene workspace recenti, documenti autorevoli o dati necessari al normale funzionamento dell'editor. Il guasto o la cancellazione dell'archivio diagnostico non devono danneggiare l'applicazione. L'archivio attivo risiede su filesystem locale e viene aperto esclusivamente dal servizio. Gli archivi esportati possono essere trasferiti e aperti su una copia locale.

### Modello iniziale

- `sessions`: identità, build/commit quando disponibile, OS, frontend, configurazione, capability, orari e stato finale.
- `producers`: processi/componenti, clock mapping e versione protocollo.
- `spans`: trace, parent/link, nome, inizio/fine, esito e attributi.
- `events`: log, comandi, eventi applicativi, marker e cambiamenti dei task.
- `metric_samples`: campioni dei gauge e delta dei contatori con unità e attributi controllati.
- `metric_rollups`: bucket temporali con aggregazioni e istogrammi mergeabili; mai media dei percentili.
- `state_checkpoints` e `state_deltas`: stato diagnostico filtrato, revisione e riferimenti temporali.
- `ingestion_batches`: sequenze confermate per deduplicazione e recupero.
- `data_gaps`: perdite, eviction, sampling e intervalli non osservati.
- `artifacts`: profili, dump consentiti e altri file, con tipo, dimensione e checksum.

Identità dei record composta da sessione, producer e sequenza; attributi estensibili in JSON, campi interrogati spesso in colonne dedicate. Gli ID a 64 bit e i timestamp ad alta precisione vengono trasportati come stringhe o rappresentazioni sicure per JavaScript.

Organizzare i batch per sessione/tempo e sfruttare le scansioni colonnari. Aggiungere indici selettivi per trace e identità soltanto dove i benchmark ne mostrano l'utilità; non replicare automaticamente una strategia di indici da database transazionale. Ricerca testuale indicizzata da valutare con dataset realistici. Ricostruire lo stato con checkpoint e delta; se manca un delta, dichiarare il tratto incompleto e ripartire dal prossimo checkpoint valido.

### Durabilità e recupero

Il servizio conferma un batch soltanto dopo il commit. Il producer conserva temporaneamente i batch non confermati entro il budget e può ritrasmetterli; il writer deduplica usando identità e sequenze.

Configurazione proposta: commit entro 250 ms o al raggiungimento della soglia di batch, da verificare per throughput. Eseguire flush esplicito dell'Appender, verificare gli errori e completare la transazione prima dell'ACK: flush non equivale a commit. Persistire record e avanzamento delle sequenze nella stessa transazione. Il tempo fra emissione e conferma è visibile. Stop di una registrazione attende il flush e il commit entro un timeout e indica chiaramente se alcuni dati restano non confermati.

Dopo il riavvio, sessioni non chiuse vengono marcate come interrotte. Gli span senza fine restano incompleti; non assegnare una durata finale inventata. Affidare il recupero del WAL a DuckDB e validare migrazioni e consistenza diagnostica. Verificare le garanzie della versione distribuita con test di terminazione forzata e recupero; non trasferire pragma o procedure SQLite a DuckDB.

Se il servizio è scollegato o il disco è pieno, il producer resta limitato in memoria e conta le perdite. Uno spool persistente lato producer è una possibile estensione, non una garanzia dell'MVP. Crash, terminazione forzata e perdita di alimentazione possono perdere eventi non confermati.

### Retention e capacità

Valori iniziali proposti, modificabili dopo benchmark: buffer producer di 16 MiB; archivio automatico di 2 GiB e 14 giorni. Applicare i limiti per byte oltre che per numero di record, includendo WAL e artifact. Mantenere un margine di spazio libero e uno spazio riservato ai metadati delle perdite.

Eliminare prima le sessioni automatiche più vecchie. Sessioni fissate ed esportazioni esplicite non vengono eliminate automaticamente; se impediscono il rispetto del budget, fermare la raccolta persistente e mostrare il motivo. Non cancellare silenziosamente la sessione attiva.

Le aggregazioni possono sopravvivere ai dati dettagliati solo con retention dichiarata; la UI distingue chiaramente statistiche disponibili e dettaglio eliminato. La manutenzione di WAL, spazio recuperabile e compattazione avviene fuori dal percorso critico e con budget di lavoro.

## Registrazione e costo della raccolta

Tre profili proposti:

- **Disattivato:** nessun listener diagnostico, database o acquisizione dettagliata.
- **Base:** errori, operazioni principali, contatori, stato dei worker e campioni di processo a bassa frequenza; sessioni persistenti automatiche quando la modalità sviluppatore è attiva.
- **Dettagliato:** timeline completa dei componenti strumentati, code/lock e checkpoint più frequenti, per registrazioni esplicite con limite temporale.

Profilazione CPU/allocazioni tramite strumenti nativi è una capability aggiuntiva: i dati di `tracing` non sostituiscono il campionamento CPU né un debugger. Integrare avvio/importazione di profili solo dove supportato, mostrando strumenti richiesti e copertura.

Filtri e sampling vengono registrati nella sessione. Le metriche aggregate possono contare tutte le operazioni anche quando le tracce vengono campionate. Una registrazione selezionata per durata a posteriori richiede conservare dati sufficienti prima della decisione: non promettere il recupero di span già scartati.

Budget limitati, priorità per errori/lifecycle e contatori di eventi persi. Anche gli eventi prioritari possono andare persi in condizioni estreme. Quando la coda si satura, aggregare o scartare secondo una politica documentata senza bloccare il main thread. Misurare tempo speso nel producer, lag del writer, memoria della console e dimensione dei batch.

## Esportazione e importazione

Formato proposto `.nwdiag`: archivio ZIP con manifest JSON versionato, tabelle diagnostiche Parquet e artifact selezionati. Deve contenere versione app/schema, sessioni incluse, intervallo, profilo di raccolta, redazioni, capability, gap, dimensioni e checksum. Parquet è il formato portabile dell'archivio; il file DuckDB resta il formato di lavoro locale.

Esportazione dall'intera sessione, da un insieme di sessioni o da una selezione temporale. Per una selezione includere checkpoint necessario, antenati/link pertinenti e metadati; marcare gli span tagliati dall'intervallo. Il risultato non deve perdere il contesto indispensabile alla lettura.

Il servizio usa una connessione di lettura e una transazione con snapshot consistente per selezionare e redigere le tabelle, esportandole con `COPY ... TO ... (FORMAT PARQUET)`. Dichiarare nel manifest il limite di acquisizione incluso. Un export lungo ha timeout e budget: può trattenere versioni dei dati e occupare spazio aggiuntivo. Non copiare semplicemente il database aperto ignorando il WAL. Prima della consegna validare archivio e checksum. Scrivere su file temporaneo e completare con rinomina atomica dove supportata.

Prima dell'export mostrare sessioni, intervallo, dimensione stimata e categorie sensibili incluse. Default: niente contenuto dei documenti, testo delle query, parametri SQL, environment completo o percorsi assoluti. Identificatori pseudonimi stabili nell'archivio mantengono le correlazioni. I messaggi liberi di errore devono essere filtrati all'origine; una redazione postuma non garantisce di rimuovere ogni segreto.

L'import valida versione, checksum, dimensioni, limiti di decompressione e percorsi. Estrarre in una directory isolata; leggere solo le tabelle Parquet previste dal manifest con schema convalidato e senza eseguire SQL/script contenuti nell'archivio. Disabilitare installazione/caricamento automatico di estensioni e accessi remoti non necessari. Migrazioni solo su una copia. Una collisione con sessioni esistenti non sovrascrive dati locali; conservare provenienza e namespace di importazione. L'archivio validato può essere interrogato localmente con DuckDB senza essere unito all'archivio delle sessioni live.

La UI riapre l'archivio con gli stessi pannelli e riferimenti, senza app collegata. In aggiunta: JSONL per eventi/log, CSV per metriche e riepiloghi, formato trace compatibile con Perfetto per la timeline. Questi export parziali non sostituiscono il pacchetto completo.

## Protocollo locale e controlli diagnostici

Handshake con versione, sessione, producer, clock e capability. API per elenco sessioni, query paginate, dettagli di trace, stato/checkpoint, start/stop, marker ed export/import. Errori e risposte hanno codici stabili.

Le notifiche live trasportano cursori e aggiornamenti limitati; una riconnessione riprende dall'ultimo cursore o segnala il gap e ricarica dal database. Ogni client lento ha un budget indipendente e non rallenta l'ingestione. Le query costose hanno timeout/cancellazione e limiti di risultati.

Nell'MVP i controlli cambiano soltanto la raccolta. Operazioni quali forzare una riconciliazione o ripetere un comando possono essere aggiunte successivamente come azioni tipizzate, esplicite e registrate, rispettando le normali validazioni dell'editor. Non prevedere una console che esegua codice arbitrario o modifichi direttamente lo stato.

Il servizio ascolta soltanto su loopback. Autenticazione con token casuale, controllo Host/Origin e handshake autenticato anche per WebSocket; il fatto di essere locale non elimina richieste provenienti da pagine web. La GUI desktop si autentica esplicitamente: assenza di Origin non equivale ad autorizzazione. Il token non finisce nei log, negli archivi o negli URL persistenti.

Eseguibili e dipendenze necessari all'analisi sono distribuiti localmente. Separare i dati dell'editor osservato da quelli della console; evitare cicli in cui il collector registra indefinitamente le proprie scritture. Il bootstrap desktop passa il token tramite canali/file locali protetti, non nella command line visibile agli altri processi.

## Fasi di implementazione e criteri di completamento

### Fase 0 — Contratti e prototipo

Definire schema, capability, lifecycle, clock, protocollo e archivio. Creare un dataset sintetico comprendente ricerca cancellata, watcher aggregato, gap e sessione interrotta. Verificare la console egui in una finestra autonoma su macOS e Linux, con console/log e timeline virtualizzate. Validare rendering, accessibilità, idle e dipendenze nel prototipo.

Completamento: decisioni documentate, limiti fissati e prototipo che legge lo stesso dataset sulle piattaforme supportate. Benchmark DuckDB con risultati riproducibili, dimensionamento dei batch/pool e verifica di export Parquet e recupero prima della fase 1.

### Fase 1 — Percorso completo con persistenza

Implementare crate, servizio separato, database dedicato e console minima. Strumentare ricerca, dispatch, pubblicazione dei risultati e applicazione degli snapshot nei tre frontend. Aggiungere avvio sviluppatore, registrazione, elenco sessioni, console, timeline e gap.

Completamento: una ricerca si segue da input a aggiornamento UI su SwiftUI, GTK e Qt; i dati confermati restano dopo il riavvio; una query superata compare con la motivazione corretta. La console può collegarsi dopo l'inizio della registrazione.

### Fase 2 — Archivio offline ed export/import

Implementare manifest, export Parquet consistente, redazione, retention, gestione disco pieno, sessioni fissate, recupero delle interruzioni e analisi offline. Questa fase precede l'ampliamento dei pannelli: persistenza ed esportabilità sono requisiti fondamentali.

Completamento: esportazione durante una registrazione attiva, import su un'altra macchina compatibile e riapertura senza editor con correlazioni intatte; nessun documento o query in un export standard; perdita non confermata esplicita dopo una terminazione forzata.

### Fase 3 — Copertura operativa

Aggiungere watcher, riconciliazione, filesystem, storage applicativo, minimappa, highlighting, FFI, code/lock, stato e checkpoint. Pubblicare una matrice di copertura per frontend e pannello.

Completamento: scansione periodica, evento del watcher, conflitto di salvataggio e risultato obsoleto sono distinguibili; ricostruzione dello stato segnala i delta mancanti; le differenze di polling tra frontend sono osservabili.

### Fase 4 — Analisi e prestazioni

Overview, rollup, confronto di sessioni, scenari benchmark, regressioni, query ottimizzate e integrazione dei profili nativi. Estendere strumenti per futuri terminali/LSP e aggiungere export OpenTelemetry quando utile.

Completamento: confronto ripetibile di due build, archivio grande navigabile entro i budget concordati e overhead misurato per ciascun profilo.

## Verifiche richieste per l'implementazione

- Correlazione attraverso FFI, thread, debounce, cancellazione e callback UI nei tre frontend.
- Ordine, deduplicazione, ritrasmissione, riconnessione, versione incompatibile e clock incerti.
- Riavvio del servizio/editor, sessioni interrotte, batch non confermati, disco pieno, retention e database non disponibile.
- Export con WAL attivo, selezione temporale, redazione, round trip, archivio incompleto/malevolo e collisioni di sessioni.
- Code sature, molti eventi del watcher, query costose e client lento: UI dell'editor sempre utilizzabile e perdita segnalata.
- Stato ricostruito coerente con checkpoint/revisioni; errori e no-op non diventano eventi applicativi riusciti.
- Benchmark in release con raccolta disattivata/base/dettagliata, stesso corpus e configurazione: input, apertura file/workspace, ricerca, salvataggio e minimappa.
- Verifiche di indipendenza: GUI/servizio terminati o sospesi, coda piena, ACK ritardati, disco pieno e query/export pesanti non cambiano gli esiti dell'editor e non introducono attese diagnostiche sul main thread.
- Confrontare build senza producer, producer presente ma disattivato, profilo base, dettagliato e GUI aperta/chiusa. Misurare p95/p99, ritardo del ciclo eventi, CPU/RAM e contesa disco/GPU su hardware e corpus fissati, con ripetizioni e variabilità dichiarate.
- Target iniziale da validare: profilo base entro l'1% di incremento della latenza p95 negli scenari concordati; non attribuire significato a differenze inferiori al rumore del benchmark. Fissare anche limiti assoluti del ritardo UI e budget CPU/memoria/disco dopo il prototipo. Se il target non è raggiunto, ridurre la raccolta e documentare i risultati prima di estenderla.
- Verifica end-to-end macOS, GNOME e KDE su host compatibili; dichiarare esplicitamente le piattaforme non verificate.

## Confini e decisioni ancora aperte

L'MVP offre osservazione, analisi dello stato registrato e controlli di raccolta. Non include replay deterministico, debugger universale, ispezione completa degli heap o rilevamento garantito dei deadlock.

Prima dell'implementazione finalizzare: versioni e licenze delle dipendenze, packaging dei due eseguibili e di DuckDB, budget misurati, granularità dei checkpoint e naming delle API. Validare egui/eframe e backend grafico sulle tre piattaforme; verificare la strategia di distribuzione del binding DuckDB e l'accessibilità effettiva.

Se i volumi reali superano il budget del motore scelto, valutare partizionamento per sessione o artifact colonnari per analisi estese mantenendo il manifest e il protocollo. Non introdurre un secondo database senza misurazioni e una strategia esplicita di consistenza.

## Riferimenti tecnici

- [OpenTelemetry: propagazione del contesto](https://opentelemetry.io/docs/concepts/context-propagation/), modello per correlare tracce, metriche e log.
- [Rust tracing](https://docs.rs/tracing/latest/tracing/), base proposta per la strumentazione del core.
- [Perfetto UI](https://perfetto.dev/docs/visualization/perfetto-ui), consultazione di trace esportate anche nel formato JSON legacy.
- [DuckDB: obiettivi e carichi analitici](https://duckdb.org/why_duckdb) e [concorrenza](https://duckdb.org/docs/current/connect/concurrency), motore embedded del servizio.
- [DuckDB Rust](https://duckdb.org/docs/current/clients/rust/overview), [Appender](https://duckdb.org/docs/current/clients/rust/data_import) e [Parquet](https://duckdb.org/docs/current/data/parquet/overview), ingestione ed export.
- [Axum](https://docs.rs/axum/latest/axum/), API locali.
- [DuckDB: transazioni](https://duckdb.org/docs/current/sql/statements/transactions), isolamento dello snapshot e commit.
- [egui](https://github.com/emilk/egui), [eframe](https://docs.rs/eframe/latest/eframe/) e [righe virtualizzate](https://docs.rs/egui_extras/latest/egui_extras/struct.TableBody.html), console desktop scelta.
- [egui_plot](https://docs.rs/egui_plot/latest/egui_plot/), grafici, e [iced](https://iced.rs/), alternativa esaminata.
- [ClickHouse: architettura analitica colonnare](https://clickhouse.com/docs/get-started/about/intro), possibile evoluzione della raccolta centralizzata.

