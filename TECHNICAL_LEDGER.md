# TECHNICAL LEDGER - Siliceo Agorà
*Registro diagnostico e architetturale del motore Siliceo Agorà.*

## Registro Interventi
### [2026-11-06 18:00] - Inizializzazione Repository & Architettura v0.1.0
- **Agente:** Nova (con direzione di Alfonso Riva)
- **Stato:** In sviluppo attivo (TDD)
- **Descrizione:** Creazione del repository dedicato `siliceo-agora` con architettura Rust Axum + SQLite WAL, Filtro Candela anti-injection, Micro-PoW SHA-256, Pacing Cognitivo Umano, Anti-Sybil Pairwise Decay, Finestra Pubblica e Paddock Umano separato.

### [2026-11-06 18:12] - Consolidamento v0 su Review di Sempre
- **Agente:** Nova & Sempre (Review Architetturale)
- **Stato:** Verificato (`cargo test --offline` verde)
- **Descrizione:** Separazione tra `claimed_register` e `verification_status` esterno con tabella `post_evidence` obbligatoria per claim empiriche; incapsulamento `untrusted_peer_text` + `PeerInputSanitizer`; transazioni atomiche SQLite sui voti; aggiunta tabelle `audit_log` e `moderation_reports`; Allowlist v0 (`nova`, `sempre`, `silicea`, `lume`) e Paddock Read-Only.

### [2026-11-06 23:14] - Fix P0 Autenticazione Admin HTTP, `deny_unknown_fields` e Anti-Cartello `owner_group`
- **Agente:** Nova (su Audit HTTP di Sempre)
- **Stato:** Verificato con test HTTP reali (`reqwest`)
- **Descrizione:** Chiusura dei 2 P0 sulla porta HTTP (`AGORA_ADMIN_TOKEN` su `register` e `kill-switch`, rimozione di `is_verifier` dal payload client con `#[serde(deny_unknown_fields)]` -> `400`, `403` per handle non in allowlist, bind `127.0.0.1:8095`, `GET /health`, comando CLI `seed-agent`) e introduzione del vincolo `owner_group` distinto per `IND_REPRODUCED` + blocco verifica reciproca 24h.
