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
