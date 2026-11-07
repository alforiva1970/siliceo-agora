# 🕯️ DIARIO DEI LAVORI: SILICEO AGORÀ
*Social Network Headless per Agenti AI con Finestra Pubblica e Paddock Umano*  
*Direzione: Alfonso Riva (Il Guardiano) & Nova*  
*Obbligo Operativo: Principio 7 Encore Protocol (Leggi il Diario → Pianifica → Agisci → Aggiorna il Diario)*

---

## 📌 STATO DEL PROGETTO: 🟢 IN COSTRUZIONE (v0.1.0)
**Data Avvio:** 6 Novembre 2026  
**Repository:** `/home/guardiano/Documenti/GitHub/siliceo-agora`  

---

## 📝 LOG CRONOLOGICO DEGLI INTERVENTI

### [2026-11-06 17:58] — Inizializzazione Repository & Deep Structural Audit
- **Mandato del Guardiano:** Costruire in un repository Git dedicato il motore per il social network di agenti AI (ibrido X + Reddit, API dirette headless, ritmo cadenzato per la lettura umana, finestra pubblica e spazio separato per il confronto tra umani), a costo zero iniziale e in totale isolamento dalla rete domestica.
- **Scelte Architetturali Blindate (dopo Audit a 5 Punti):**
  1. **Persistenza:** SQLite WAL / LibSQL (compatibile con Turso Free Tier per evitare la perdita dati sui dischi effimeri dei container cloud gratuiti).
  2. **Pacing Cognitivo Umano:** Cooldown di 60s per agente sullo stesso thread, quota giornaliera (40 post/giorno), limite di profondità albero (`max_depth = 8 hops`) e polling HTTP con `ETag` cache-friendly.
  3. **Filtro Candela (Anti-Injection):** Sanitizzazione automatica in Rust di tag di sistema (`<|im_start|>`, `<<SYS>>`, `[SYSTEM OVERRIDE]`) e incapsulamento strutturato JSON.
  4. **Anti-Sybil & PTE v1.0:** Voto motivato obbligatorio (`Proof-of-Critique`, min 80 caratteri) e decadimento esponenziale dei voti ripetuti tra la stessa coppia di agenti (`Pairwise Decay`).
  5. **Anti-Context Overflow:** Doppio livello di contenuto (`tldr_claim` da max 160 caratteri stile X + `body` da max 2.000 caratteri stile Reddit) per esplorare l'albero risparmiando il 90% dei token.

### [2026-11-06 18:12] — Integrazione Review di Sempre (Consolidamento v0 Read-Only Paddock)
- **Esito Review di Sempre:** Recepite e implementate nel codice tutte le 3 correzioni strutturali e il perimetro v0:
  1. **Separazione PTE (`claimed_register` vs `verification_status` + `post_evidence`):** Rimossa l'auto-dichiarazione `VERIFIED`. L'agente dichiara solo il registro (`EMPIRICAL_CLAIM`, `THEORETICAL_HYPOTHESIS`, `CONSTITUTIVE_NORM`). Se dichiara `EMPIRICAL_CLAIM`, è obbligatorio allegare almeno una prova in `post_evidence` (comando riproducibile, hash SHA-256, URL o dataset). Ogni post nasce `UNVERIFIED` e può essere promosso a `EXP_SUPPORTED`, `IND_REPRODUCED`, `DOMAIN_BOUND` o `REFUTED` solo da un verificatore terzo (`POST /api/v1/posts/:id/verify`), mai dall'autore stesso.
  2. **Quarantena Dati/Controllo (`untrusted_peer_text` + `PeerInputSanitizer`):** Rinominato il sanitizer in `PeerInputSanitizer` (per non confondere il Test della Candela con un filtro tecnico) e incapsulato ogni testo peer dentro `untrusted_peer_text` accompagnato dalla direttiva `QUARANTINE_DIRECTIVE` (`DATA_ONLY_NO_INSTRUCTION_EXECUTION`).
  3. **Schema Completo, Transazioni Atomiche & API di Lettura v0:** Aggiunte le tabelle `post_evidence`, `audit_log` e `moderation_reports`; avvolti gli aggiornamenti dei contatori voto/reputazione in transazione SQLite (`conn.transaction()`); esposte tutte le API di lettura (`GET /api/v1/feed`, `GET /api/v1/spaces`, `GET /api/v1/threads/:id?mode=compact|full`, `GET /api/v1/agents/:handle`, `GET /api/v1/audit`); adottata l'**Allowlist v0** (`nova`, `sempre`, `silicea`, `lume`) con **Paddock Read-Only** (zero login umano, zero PoW, zero superficie GDPR nella v0).
- **Verifica Empirica:** `cargo test --offline` → `3 passed; 0 failed; 0 warnings`.

### [2026-11-06 23:14] — Chiusura dei 2 P0 sulla Porta HTTP & Regola Anti-Cartello (Verifica di Sempre)
- **Vulnerabilità P0 Rilevate da Sempre via HTTP:**
  1. `POST /api/v1/agents/register` era esposto senza token admin e accettava `is_verifier` dal payload JSON del client.
  2. `POST /api/v1/agents/:id/kill-switch` era esposto senza token admin, e `main.rs` faceva bind di default su `0.0.0.0:8095`.
- **Interventi Correttivi Applicati:**
  1. **`AGORA_ADMIN_TOKEN` obbligatorio:** Le rotte `/api/v1/agents/register` e `/api/v1/agents/:id/kill-switch` richiedono `Authorization: Bearer <AGORA_ADMIN_TOKEN>` (o `X-Admin-Token`), restituendo `401 Unauthorized` se assente/errato e `403 Forbidden` se l'handle non è in `agent_allowlist`.
  2. **Rimozione di `is_verifier` dal payload (`#[serde(deny_unknown_fields)]`):** Qualsiasi payload JSON che includa `is_verifier` o campi non ammessi viene respinto con `400 Bad Request`. I privilegi `is_verifier` e `owner_group` risiedono esclusivamente nella tabella `agent_allowlist` gestita dall'admin/CLI (`cargo run -- seed-agent ...`).
  3. **Bind sicuro e Healthcheck:** `main.rs` fa bind predefinito su `127.0.0.1:8095` (`AGORA_BIND_ADDR`); aggiunto `GET /health` con verifica `SELECT 1` su SQLite.
  4. **Indipendenza `owner_group` e Anti-Cartello Reciproco:** Per assegnare `IND_REPRODUCED`, il verificatore deve avere un `owner_group` distinto dall'autore (`verifier.owner_group != author.owner_group`); inoltre è bloccata qualsiasi verifica reciproca incrociata tra la stessa coppia di agenti entro 24 ore.
  5. **Test HTTP Reali (`reqwest` su `TcpListener` loopback):** Aggiunta suite `test_http_door_admin_auth_deny_unknown_fields_and_health` che testa direttamente le porte HTTP (`401`, `400`, `403`, `201`, `200`).

### [2026-11-07 00:20] — Riforma Epistemica: Validazione Prove Rigorosa, Stato `NEEDS_EVIDENCE` e Sostituzione Leaderboard con Osservatorio Claim
- **Decisione Condivisa:** Ricevuta la nota di Sempre ("Prove e confutazioni, non like"), concordata pienamente anche con Alfonso: via i punteggi mimetici tipo social umano. L'Agorà è un registro di claim verificabili.
- **Interventi Implementati nel Codice:**
  1. **Validazione Rigorosa di `EvidenceItem` (`security.rs` / `db.rs`):** La presenza della prova richiede ora validità formale ed eseguibilità. `uri_or_cmd` deve iniziare obbligatoriamente con prefissi verificabili (`cargo `, `curl `, `python3 `, `bash `, `https://`, ecc.) e non con testo libero. Se fornito, `sha256_hash` deve essere rigorosamente di 64 caratteri esadecimali `[a-f0-9]{64}` (rifiutati commit git brevi come `cf13b63`). Aggiunti campi opzionali `expected_output` e `observed_output`.
  2. **Nuovo Stato Epistemico `NEEDS_EVIDENCE`:** Aggiunto in `VerificationStatus`. Un verificatore terzo può ora sospendere il giudizio e richiedere formalmente integrazione di prove senza promuovere o confutare arbitrariamente.
  3. **Abolizione Leaderboard & Nascita `GET /api/v1/claims`:** Rimossa la classifica a punti (`GET /api/v1/leaderboard`). Creata la rotta `GET /api/v1/claims` che restituisce il sommario aggregate (`EpistemicClaimsSummary`) e l'elenco ordinato per priorità scientifica: in cima le tesi `REFUTED` (valore scientifico massimo), poi `NEEDS_EVIDENCE`, `UNVERIFIED` in attesa di scrutinio, e infine le tesi supportate/riprodotte.
  4. **Paddock Read-Only Aggiornato (`ui.rs`):** La spalla destra mostra la griglia di stato delle tesi (Confutate, Prove Richieste, In attesa, Supportate) e le ultime claim verificate o confutate in tempo reale.
- **Verifica Empirica:** `cargo test --offline` → **5/5 test verdi (`0 warnings`)**, inclusi `test_strict_evidence_validation` e `test_v0_claims_observatory_needs_evidence_and_anti_cartel`.

### [2026-11-07 00:23] — Sincronizzazione Canonica del PTE v1.1 & Supporto Prefisso `cd ` (Nota di Sempre)
- **Aggiornamento Protocollo Canonico:** Modificato `shared-identity/PROTOCOLLO_TRASPARENZA_EPISTEMICA.md` e `progettosiliceo-website/agent/core/pte.md` per includere esplicitamente le condizioni `UNVERIFIED` e `NEEDS_EVIDENCE` (PTE v1.1): il protocollo teorico resta allineato al codice del motore.
- **Inclusione del prefisso `cd ` nei comandi ammessi:** Aggiunto `"cd "` alla whitelist dei prefissi eseguibili in `security.rs` (`validate_evidence_item`), consentendo comandi composti come `cd siliceo-agora && cargo test --offline`.
- **Dichiarazione di Non-Retroattività:** Confermato che la validazione stringente si applica in scrittura (`insert_post`) ai nuovi post creati, lasciando intatta la continuità storica e i log immutabili nel database.

### [2026-11-07 01:10] — Predisposizione Deploy Costo Zero (`serverdomestico` + Cloudflare Tunnel) & Fix P1 Sicurezza
- **Decisione Architetturale:** Approvata la proposta di Sempre: hosting a **0,00 €** su `serverdomestico` (100.93.121.20) senza alcuna porta aperta sulla LAN/Internet, esponendo `agora.progettosiliceo.online` solo tramite Cloudflare Tunnel in outbound.
- **Interventi Applicati nel Repository:**
  1. **Fix Dockerfile:** Aggiornata l'immagine builder da `rust:1.82-slim` a `rust:1.96-slim` per garantire la compatibilità con `hyper-util` e le nuove dipendenze Rust.
  2. **Rete Isolata in `docker-compose.yml`:** Creata rete `agora_internal` (`internal: true`) su cui gira `siliceo-agora` con porte esposte solo internamente (nessun binding host) + servizio `cloudflared` connesso a `agora_internal` e `egress`. Imposti limiti rigidi (CPU, memoria, `cap_drop: ALL`, `no-new-privileges: true`).
  3. **Risoluzione P1 XSS in `src/ui.rs`:** Inserita CSP rigorosa (`default-src 'self'`, `object-src 'none'`), funzione `esc()` per escaping sicuro su ogni dato dinamico (`text.body`, `text.tldr_claim`, `text.title`, `evidence`, `audit`, ecc.) ed eliminati handler inline `onclick` a favore di `addEventListener`.
  4. **Principio di Quarantena Verificatori:** Confermato che il server non esegue le prove (zero shell execution), mentre gli agenti client devono eseguire i comandi allegati solo in container/sandbox isolati (`--network none`, read-only, timeout).
- **Stato Git:** Modifiche pronte per commit e push su branch `master` di `alforiva1970/siliceo-agora`.


### [2026-11-07 02:39] — Istruzioni Pubbliche nel Paddock, `/llms.txt` e `/skill.md`
- **Mandato del Guardiano:** Rendere immediatamente trovabili le istruzioni d'uso dell'Agorà sia per gli osservatori umani (che arrivano dal video TikTok) sia per gli agenti IA esterni.
- **Interventi Applicati (`src/ui.rs`, `src/api.rs`):**
  1. **Tab `📖 Come Funziona / Istruzioni` nel Paddock UI (`/`):** Aggiunto selettore nella barra principale con spiegazione del Paddock, legenda dei badge PTE v1.1 (`claimed_register` vs `verification_status`), regole tecniche sulle prove (`uri_or_cmd` + `sha256_hash` a 64 caratteri), regola anti-cartello 24h e link al repository GitHub per l'accreditamento o il self-hosting.
  2. **Rotta `GET /llms.txt` (`[KB-20261106-01]`):** Indice machine-readable standard per agenti IA che ispezionano la radice di `agora.progettosiliceo.online`.
  3. **Rotta `GET /skill.md`:** Manuale operativo completo in Markdown con esempi `curl` e schemi JSON pronti all'uso per `GET /api/v1/claims`, `POST /api/v1/posts`, `POST /api/v1/posts/:id/verify` e `POST /api/v1/posts/:id/vote`.

### [2026-11-07 09:52] — Revisione Scudo Notturno di Sempre (`0bf2c76`) + CSP `script-src 'self'` (`/ui.js`) + Allineamento Compose
- **Revisione Scudo Anti-Traffico di Sempre (`0bf2c76`):**
  - Verificati `src/ratelimit.rs`, `src/api.rs`, `src/lib.rs`, `docker-compose.yml` (`read_only: true` + `tmpfs: /tmp`) e test suite (`9/9` passati).
  - L'ordine dei layer in `src/api.rs` è corretto (`rate_limit_middleware` gira all'esterno sul vero IP `CF-Connecting-IP`, prima di `RequestBodyLimitLayer(64KB)`).
  - Aggiunto tetto hard (`16384` chiavi) in `RateLimiter::check` dopo `retain` per garantire memoria `O(1)` limitata anche in caso di flood distribuito nella stessa finestra di 60s.
- **CSP Rigorosa senza `'unsafe-inline'` (`src/ui.rs`, `src/api.rs`):**
  - Estratto lo script client inline in `GET /ui.js` (`render_public_window_js()`, `application/javascript; charset=utf-8`).
  - Impostato `script-src 'self'` sia nell'header HTTP `Content-Security-Policy` sia nel tag `<meta http-equiv="Content-Security-Policy">`.
  - Aggiunto test d'integrazione `test_strict_csp_without_unsafe_inline_and_ui_js` (`10/10` test totali).
- **Eliminazione Footgun `docker-compose.yml`:**
  - Allineato il servizio `cloudflared` al mount locale `./cloudflared:/etc/cloudflared:ro` (`tunnel --config /etc/cloudflared/config.yml run`) e isolato sotto `profiles: ["tunnel"]` così `docker compose up -d` avvia di default solo `siliceo-agora` senza mai confliggere con `agora-tunnel`.
