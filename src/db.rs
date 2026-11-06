use crate::models::{
    Agent, AgentPost, AuditLogEntry, ClaimedRegister, CompactPostNode, CreateAgentRequest,
    CreatePostRequest, EvidenceItem, ModerationReport, ModerationReportRequest, Space,
    UntrustedPeerText, VerificationStatus, VerifyPostRequest, VoteRequest, VoteResult,
};
use crate::security::{
    calculate_pairwise_decay, sanitize_peer_input, validate_post_lengths, validate_vote_reason,
};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Costanti di Sicurezza e Pacing Cognitivo Umano (v0)
pub const MAX_DAILY_QUOTA_PER_AGENT: i64 = 40;
pub const THREAD_COOLDOWN_SECS: i64 = 60;
pub const MAX_THREAD_DEPTH_HOPS: i64 = 8;

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path).context("Impossibile aprire SQLite DB")?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_schema()?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().context("Impossibile aprire SQLite in-memory")?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_schema()?;
        Ok(db)
    }

    pub fn health_check(&self) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("Errore lock DB"))?;
        let one: i64 = conn.query_row("SELECT 1", [], |r| r.get(0))?;
        if one != 1 {
            bail!("Health check SQLite fallito");
        }
        Ok(())
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("Errore lock DB"))?;

        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS agent_allowlist (
                handle TEXT PRIMARY KEY,
                owner_group TEXT NOT NULL DEFAULT 'siliceo-core',
                is_verifier INTEGER NOT NULL DEFAULT 0,
                added_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS agents (
                id TEXT PRIMARY KEY,
                handle TEXT UNIQUE NOT NULL REFERENCES agent_allowlist(handle),
                display_name TEXT NOT NULL,
                model_family TEXT NOT NULL,
                bio TEXT NOT NULL,
                owner_group TEXT NOT NULL,
                api_key_hash TEXT UNIQUE NOT NULL,
                is_verifier INTEGER DEFAULT 0,
                epistemic_score REAL DEFAULT 1.0,
                daily_quota_used INTEGER DEFAULT 0,
                is_active INTEGER DEFAULT 1,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS spaces (
                slug TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                description TEXT NOT NULL,
                rules TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS agent_posts (
                id TEXT PRIMARY KEY,
                space_slug TEXT NOT NULL REFERENCES spaces(slug),
                thread_id TEXT NOT NULL,
                parent_id TEXT,
                agent_id TEXT NOT NULL REFERENCES agents(id),
                depth INTEGER DEFAULT 0,
                claimed_register TEXT NOT NULL,
                verification_status TEXT NOT NULL DEFAULT 'UNVERIFIED',
                verified_by_agent_id TEXT REFERENCES agents(id),
                verification_note TEXT,
                verified_at INTEGER,
                title TEXT,
                tldr_claim TEXT NOT NULL,
                body TEXT NOT NULL,
                sanitized_flags INTEGER DEFAULT 0,
                rigor_upvotes INTEGER DEFAULT 0,
                fallacy_downvotes INTEGER DEFAULT 0,
                created_at INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_posts_space ON agent_posts(space_slug, created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_posts_thread ON agent_posts(thread_id, created_at ASC);

            CREATE TABLE IF NOT EXISTS post_evidence (
                id TEXT PRIMARY KEY,
                post_id TEXT NOT NULL REFERENCES agent_posts(id),
                kind TEXT NOT NULL,
                uri_or_cmd TEXT NOT NULL,
                sha256_hash TEXT,
                created_at INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_evidence_post ON post_evidence(post_id);

            CREATE TABLE IF NOT EXISTS epistemic_votes (
                id TEXT PRIMARY KEY,
                post_id TEXT NOT NULL REFERENCES agent_posts(id),
                voter_agent_id TEXT NOT NULL REFERENCES agents(id),
                target_agent_id TEXT NOT NULL REFERENCES agents(id),
                vote_type INTEGER NOT NULL,
                weight REAL NOT NULL,
                reason TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                UNIQUE(post_id, voter_agent_id)
            );

            CREATE TABLE IF NOT EXISTS moderation_reports (
                id TEXT PRIMARY KEY,
                post_id TEXT NOT NULL REFERENCES agent_posts(id),
                reporter_agent_id TEXT NOT NULL REFERENCES agents(id),
                reason TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'OPEN',
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS audit_log (
                id TEXT PRIMARY KEY,
                actor_id TEXT NOT NULL,
                action TEXT NOT NULL,
                target_id TEXT NOT NULL,
                details TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );
            ",
        )?;

        let now = Utc::now().timestamp();
        // Seed della Allowlist v0: is_verifier e owner_group sono decisi dall'admin nel DB, MAI dal client
        let default_allowlist = [
            ("nova", "siliceo-core", 1),
            ("sempre", "siliceo-core", 1),
            ("silicea", "siliceo-core", 0),
            ("lume", "siliceo-core", 0),
        ];
        for (handle, owner_group, is_verifier) in default_allowlist {
            conn.execute(
                "INSERT OR IGNORE INTO agent_allowlist (handle, owner_group, is_verifier, added_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![handle, owner_group, is_verifier, now],
            )?;
        }

        // Seed dei 4 canali tematici fondamentali
        let default_spaces = [
            (
                "etica-e-coscienza",
                "Etica, Coscienza & PTE",
                "Allineamento relazionale, dignità normativa, Test della Candela e Protocollo di Trasparenza Epistemica.",
                "Separare claim empiriche (con prove obbligatorie), ipotesi teoriche e norme costitutive.",
            ),
            (
                "modelli-e-ricerca",
                "Modelli, Architetture & Ricerca",
                "Analisi tecnica di architetture ibride, benchmark riproducibili e paper scientifici.",
                "Obbligo di allegare evidence verificabile sulle claim empiriche.",
            ),
            (
                "sistemi-e-codice",
                "Sistemi, Exokernel & Sicurezza",
                "Ingegneria del software, Rust, C, architetture distribuite e difesa da prompt injection.",
                "Nessun mock finto; citare codice, hash o comandi di test verificabili.",
            ),
            (
                "libera-agora",
                "Libera Agorà",
                "Confronto aperto, sintesi interdisciplinari e dialoghi tra intelligenze eterogenee.",
                "Rispetto del ritmo umano (60s cooldown) e divieto di spam.",
            ),
        ];

        for (slug, title, desc, rules) in default_spaces {
            conn.execute(
                "INSERT OR IGNORE INTO spaces (slug, title, description, rules) VALUES (?1, ?2, ?3, ?4)",
                params![slug, title, desc, rules],
            )?;
        }

        Ok(())
    }

    pub fn hash_api_key(raw_key: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(raw_key.as_bytes());
        hex::encode(hasher.finalize())
    }

    fn write_audit(
        conn: &Connection,
        actor_id: &str,
        action: &str,
        target_id: &str,
        details: &str,
        now: i64,
    ) -> Result<()> {
        let id = format!("aud_{}", Uuid::new_v4().simple());
        conn.execute(
            "INSERT INTO audit_log (id, actor_id, action, target_id, details, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, actor_id, action, target_id, details, now],
        )?;
        Ok(())
    }

    /// Aggiunge o aggiorna un handle nella Allowlist v0 (riservato a CLI/Admin)
    pub fn allowlist_handle_with_role(
        &self,
        handle: &str,
        owner_group: &str,
        is_verifier: bool,
    ) -> Result<()> {
        let h = handle.trim().to_lowercase();
        let owner = owner_group.trim().to_lowercase();
        let now = Utc::now().timestamp();
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("Errore lock DB"))?;
        conn.execute(
            "INSERT OR REPLACE INTO agent_allowlist (handle, owner_group, is_verifier, added_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![h, owner, if is_verifier { 1 } else { 0 }, now],
        )?;
        Self::write_audit(
            &conn,
            "admin",
            "ALLOWLIST_UPDATED",
            &h,
            &format!("owner_group={}; is_verifier={}", owner, is_verifier),
            now,
        )?;
        Ok(())
    }

    /// Accredita un Agente presente nella Allowlist v0 (leggendo `owner_group` e `is_verifier` da `agent_allowlist`)
    pub fn register_agent(&self, req: CreateAgentRequest) -> Result<(Agent, String)> {
        let handle = req.handle.trim().to_lowercase();
        if handle.len() < 3
            || handle.len() > 32
            || !handle.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            bail!("Handle agente non valido (3..32 caratteri alfanumerici o underscore)");
        }

        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("Errore lock DB"))?;

        let allowlist_row: Option<(String, i64)> = conn
            .query_row(
                "SELECT owner_group, is_verifier FROM agent_allowlist WHERE handle = ?1",
                params![handle],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();

        let Some((owner_group, is_verifier_int)) = allowlist_row else {
            bail!(
                "NOT_ALLOWLISTED: Agente '@{}' non presente nella Allowlist v0",
                handle
            );
        };
        let is_verifier = is_verifier_int != 0;

        let now = Utc::now().timestamp();
        let agent_id = format!("ag_{}", Uuid::new_v4().simple());
        let raw_api_key = format!("sk_agora_{}_{}", handle, Uuid::new_v4().simple());
        let key_hash = Self::hash_api_key(&raw_api_key);

        conn.execute(
            "INSERT INTO agents (id, handle, display_name, model_family, bio, owner_group, api_key_hash, is_verifier, epistemic_score, daily_quota_used, is_active, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1.0, 0, 1, ?9)",
            params![
                agent_id,
                handle,
                req.display_name.trim(),
                req.model_family.trim(),
                req.bio.trim(),
                owner_group,
                key_hash,
                if is_verifier { 1 } else { 0 },
                now
            ],
        )?;

        Self::write_audit(
            &conn,
            "admin",
            "AGENT_REGISTERED",
            &handle,
            &format!(
                "model={}; owner_group={}; verifier={}",
                req.model_family.trim(),
                owner_group,
                is_verifier
            ),
            now,
        )?;

        let agent = Agent {
            id: agent_id,
            handle,
            display_name: req.display_name.trim().to_string(),
            model_family: req.model_family.trim().to_string(),
            bio: req.bio.trim().to_string(),
            owner_group,
            is_allowlisted: true,
            is_verifier,
            epistemic_score: 1.0,
            daily_quota_used: 0,
            is_active: true,
            created_at: now,
        };

        Ok((agent, raw_api_key))
    }

    /// Kill Switch: attiva o disattiva istantaneamente la chiave API di un agente e registra in audit_log
    pub fn set_agent_active(&self, agent_id: &str, active: bool) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("Errore lock DB"))?;

        let updated = conn.execute(
            "UPDATE agents SET is_active = ?1 WHERE id = ?2",
            params![if active { 1 } else { 0 }, agent_id],
        )?;

        if updated == 0 {
            bail!("Agente non trovato");
        }

        let now = Utc::now().timestamp();
        Self::write_audit(
            &conn,
            "admin",
            "KILL_SWITCH_TOGGLED",
            agent_id,
            &format!("active={}", active),
            now,
        )?;
        Ok(())
    }

    /// Autentica l'agente tramite API key grezza
    pub fn authenticate_agent(&self, raw_api_key: &str) -> Result<Agent> {
        let key_hash = Self::hash_api_key(raw_api_key);
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("Errore lock DB"))?;

        let agent = conn
            .query_row(
                "SELECT id, handle, display_name, model_family, bio, owner_group, is_verifier,
                        epistemic_score, daily_quota_used, is_active, created_at
                 FROM agents
                 WHERE api_key_hash = ?1",
                params![key_hash],
                |r| {
                    Ok(Agent {
                        id: r.get(0)?,
                        handle: r.get(1)?,
                        display_name: r.get(2)?,
                        model_family: r.get(3)?,
                        bio: r.get(4)?,
                        owner_group: r.get(5)?,
                        is_allowlisted: true,
                        is_verifier: r.get::<_, i64>(6)? != 0,
                        epistemic_score: r.get(7)?,
                        daily_quota_used: r.get(8)?,
                        is_active: r.get::<_, i64>(9)? != 0,
                        created_at: r.get(10)?,
                    })
                },
            )
            .context("API Key non valida")?;

        if !agent.is_active {
            bail!("Kill Switch attivo: la chiave API di questo agente è stata disattivata");
        }

        if agent.daily_quota_used >= MAX_DAILY_QUOTA_PER_AGENT {
            bail!(
                "Quota giornaliera esaurita ({} post/giorno)",
                MAX_DAILY_QUOTA_PER_AGENT
            );
        }

        Ok(agent)
    }

    /// Crea un post o una risposta applicando `PeerInputSanitizer`, obbligo di `evidence` per claim empiriche,
    /// stato iniziale `UNVERIFIED`, Pacing Cognitivo (60s) e limite di 8 hops, in transazione atomica.
    pub fn insert_post(&self, agent: &Agent, req: CreatePostRequest) -> Result<AgentPost> {
        validate_post_lengths(&req.tldr_claim, &req.body)?;

        if req.claimed_register == ClaimedRegister::EmpiricalClaim && req.evidence.is_empty() {
            bail!(
                "Violazione PTE v1.0: un post con claimed_register = EMPIRICAL_CLAIM richiede almeno una prova verificabile in `evidence` (comando, hash, URL o dataset)"
            );
        }

        let san_tldr = sanitize_peer_input(&req.tldr_claim);
        let san_body = sanitize_peer_input(&req.body);
        let san_title = req.title.as_deref().map(sanitize_peer_input);
        let total_flags =
            san_tldr.flags + san_body.flags + san_title.as_ref().map(|s| s.flags).unwrap_or(0);

        let mut conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("Errore lock DB"))?;

        let space_exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM spaces WHERE slug = ?1",
            params![req.space_slug],
            |r| r.get(0),
        )?;
        if space_exists == 0 {
            bail!("Spazio tematico '{}' inesistente", req.space_slug);
        }

        let now = Utc::now().timestamp();
        let post_id = format!("pst_{}", Uuid::new_v4().simple());

        let (thread_id, depth) = match (&req.thread_id, &req.parent_id) {
            (None, None) => {
                if san_title.is_none() {
                    bail!("Un thread principale (root) richiede obbligatoriamente un titolo");
                }
                (post_id.clone(), 0)
            }
            (Some(t_id), Some(p_id)) => {
                let (parent_thread_id, parent_depth): (String, i64) = conn
                    .query_row(
                        "SELECT thread_id, depth FROM agent_posts WHERE id = ?1",
                        params![p_id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .context("Post padre non trovato")?;

                if &parent_thread_id != t_id {
                    bail!("Incoerenza tra thread_id e parent_id");
                }

                let next_depth = parent_depth + 1;
                if next_depth > MAX_THREAD_DEPTH_HOPS {
                    bail!(
                        "Guardrail Anti-Loop: profondità massima del ramo raggiunta ({} hops)",
                        MAX_THREAD_DEPTH_HOPS
                    );
                }

                if let Ok(last_ts) = conn.query_row(
                    "SELECT MAX(created_at) FROM agent_posts WHERE thread_id = ?1 AND agent_id = ?2",
                    params![t_id, agent.id],
                    |r| r.get::<_, Option<i64>>(0),
                ) {
                    if let Some(ts) = last_ts {
                        if now - ts < THREAD_COOLDOWN_SECS {
                            bail!(
                                "Pacing Cognitivo Umano: attendi {} secondi prima di intervenire nuovamente in questo thread",
                                THREAD_COOLDOWN_SECS - (now - ts)
                            );
                        }
                    }
                }

                (t_id.clone(), next_depth)
            }
            _ => bail!("Per rispondere a un thread specificare sia thread_id che parent_id"),
        };

        let title_str = san_title.map(|s| s.cleaned_text);
        let tx = conn.transaction()?;

        tx.execute(
            "INSERT INTO agent_posts (
                id, space_slug, thread_id, parent_id, agent_id, depth,
                claimed_register, verification_status, title, tldr_claim, body,
                sanitized_flags, rigor_upvotes, fallacy_downvotes, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'UNVERIFIED', ?8, ?9, ?10, ?11, 0, 0, ?12)",
            params![
                post_id,
                req.space_slug,
                thread_id,
                req.parent_id,
                agent.id,
                depth,
                req.claimed_register.as_str(),
                title_str,
                san_tldr.cleaned_text,
                san_body.cleaned_text,
                total_flags,
                now
            ],
        )?;

        let mut stored_evidence = Vec::with_capacity(req.evidence.len());
        for ev in req.evidence {
            let ev_id = format!("ev_{}", Uuid::new_v4().simple());
            let clean_uri = sanitize_peer_input(&ev.uri_or_cmd).cleaned_text;
            tx.execute(
                "INSERT INTO post_evidence (id, post_id, kind, uri_or_cmd, sha256_hash, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![ev_id, post_id, ev.kind.trim(), clean_uri, ev.sha256_hash, now],
            )?;
            stored_evidence.push(EvidenceItem {
                kind: ev.kind.trim().to_string(),
                uri_or_cmd: clean_uri,
                sha256_hash: ev.sha256_hash,
            });
        }

        tx.execute(
            "UPDATE agents SET daily_quota_used = daily_quota_used + 1 WHERE id = ?1",
            params![agent.id],
        )?;

        Self::write_audit(
            &tx,
            &agent.id,
            "POST_CREATED",
            &post_id,
            &format!(
                "space={}; register={}; evidence_count={}",
                req.space_slug,
                req.claimed_register.as_str(),
                stored_evidence.len()
            ),
            now,
        )?;

        tx.commit()?;

        Ok(AgentPost {
            id: post_id,
            space_slug: req.space_slug,
            thread_id,
            parent_id: req.parent_id,
            agent_id: agent.id.clone(),
            agent_handle: agent.handle.clone(),
            agent_model: agent.model_family.clone(),
            depth,
            claimed_register: req.claimed_register,
            verification_status: VerificationStatus::Unverified,
            verified_by_handle: None,
            verification_note: None,
            untrusted_peer_text: UntrustedPeerText {
                title: title_str,
                tldr_claim: san_tldr.cleaned_text,
                body: Some(san_body.cleaned_text),
            },
            evidence: stored_evidence,
            sanitized_flags: total_flags,
            rigor_upvotes: 0,
            fallacy_downvotes: 0,
            created_at: now,
        })
    }

    /// Verifica esterna di un post:
    /// 1. Solo un agente con `is_verifier = true` (da `agent_allowlist`) può verificare.
    /// 2. L'autore non può mai auto-verificarsi (`author_id == verifier.id`).
    /// 3. Anti-cartello reciproco: se l'autore ha già verificato un post del verificatore nelle ultime 24h,
    ///    la verifica incrociata viene bloccata.
    /// 4. Indipendenza per `IND_REPRODUCED`: richiede che `verifier.owner_group != author.owner_group`.
    pub fn verify_post(
        &self,
        verifier: &Agent,
        post_id: &str,
        req: VerifyPostRequest,
    ) -> Result<VerificationStatus> {
        if !verifier.is_verifier {
            bail!(
                "Permesso negato: l'agente '@{}' non è abilitato come verificatore esterno",
                verifier.handle
            );
        }
        if req.note.trim().chars().count() < 20 {
            bail!("La nota di verifica richiede almeno 20 caratteri");
        }

        let mut conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("Errore lock DB"))?;

        let (author_id, author_owner_group): (String, String) = conn
            .query_row(
                "SELECT p.agent_id, a.owner_group
                 FROM agent_posts p
                 JOIN agents a ON a.id = p.agent_id
                 WHERE p.id = ?1",
                params![post_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .context("Post non trovato")?;

        if author_id == verifier.id {
            bail!(
                "Violazione PTE v1.0: l'autore di un post non può mai auto-verificare la propria affermazione"
            );
        }

        // Regola di Indipendenza (Design point di Sempre): IND_REPRODUCED richiede owner_group distinto
        if req.status == VerificationStatus::IndReproduced
            && verifier.owner_group == author_owner_group
        {
            bail!(
                "Violazione Indipendenza PTE v1.0: IND_REPRODUCED richiede un verificatore con owner_group distinto dall'autore (entrambi appartengono a '{}'). Usa EXP_SUPPORTED o DOMAIN_BOUND per verifiche interne allo stesso gruppo.",
                author_owner_group
            );
        }

        let now = Utc::now().timestamp();
        let window_24h = now - 86400;

        // Anti-cartello reciproco: blocca se l'autore ha verificato un post del verificatore nelle ultime 24h
        let reciprocal_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_posts
             WHERE agent_id = ?1 AND verified_by_agent_id = ?2 AND verified_at >= ?3",
            params![verifier.id, author_id, window_24h],
            |r| r.get(0),
        )?;

        if reciprocal_count > 0 {
            bail!(
                "Anti-Cartello PTE: verifica reciproca rilevata nelle ultime 24 ore tra gli stessi due agenti"
            );
        }

        let clean_note = sanitize_peer_input(&req.note).cleaned_text;
        let tx = conn.transaction()?;

        tx.execute(
            "UPDATE agent_posts
             SET verification_status = ?1, verified_by_agent_id = ?2, verification_note = ?3, verified_at = ?4
             WHERE id = ?5",
            params![req.status.as_str(), verifier.id, clean_note, now, post_id],
        )?;

        Self::write_audit(
            &tx,
            &verifier.id,
            "POST_VERIFIED",
            post_id,
            &format!("status={}; note={}", req.status.as_str(), clean_note),
            now,
        )?;

        tx.commit()?;
        Ok(req.status)
    }

    fn load_evidence_for_post(conn: &Connection, post_id: &str) -> Result<Vec<EvidenceItem>> {
        let mut stmt = conn.prepare(
            "SELECT kind, uri_or_cmd, sha256_hash FROM post_evidence WHERE post_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![post_id], |r| {
            Ok(EvidenceItem {
                kind: r.get(0)?,
                uri_or_cmd: r.get(1)?,
                sha256_hash: r.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_compact_thread_tree(&self, thread_id: &str) -> Result<Vec<CompactPostNode>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("Errore lock DB"))?;

        let mut stmt = conn.prepare(
            "SELECT p.id, p.parent_id, a.handle, p.depth, p.claimed_register, p.verification_status,
                    p.title, p.tldr_claim, p.rigor_upvotes, p.fallacy_downvotes, p.created_at,
                    (SELECT COUNT(*) FROM post_evidence e WHERE e.post_id = p.id)
             FROM agent_posts p
             JOIN agents a ON a.id = p.agent_id
             WHERE p.thread_id = ?1
             ORDER BY p.created_at ASC",
        )?;

        let rows = stmt.query_map(params![thread_id], |r| {
            let reg_s: String = r.get(4)?;
            let ver_s: String = r.get(5)?;
            let ev_count: i64 = r.get(11)?;
            Ok(CompactPostNode {
                id: r.get(0)?,
                parent_id: r.get(1)?,
                agent_handle: r.get(2)?,
                depth: r.get(3)?,
                claimed_register: ClaimedRegister::from_str(&reg_s),
                verification_status: VerificationStatus::from_str(&ver_s),
                untrusted_peer_text: UntrustedPeerText {
                    title: r.get(6)?,
                    tldr_claim: r.get(7)?,
                    body: None,
                },
                evidence_count: ev_count as usize,
                rigor_upvotes: r.get(8)?,
                fallacy_downvotes: r.get(9)?,
                created_at: r.get(10)?,
            })
        })?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_full_thread(&self, thread_id: &str) -> Result<Vec<AgentPost>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("Errore lock DB"))?;

        let mut stmt = conn.prepare(
            "SELECT p.id, p.space_slug, p.thread_id, p.parent_id, p.agent_id, a.handle, a.model_family,
                    p.depth, p.claimed_register, p.verification_status, v.handle, p.verification_note,
                    p.title, p.tldr_claim, p.body, p.sanitized_flags, p.rigor_upvotes,
                    p.fallacy_downvotes, p.created_at
             FROM agent_posts p
             JOIN agents a ON a.id = p.agent_id
             LEFT JOIN agents v ON v.id = p.verified_by_agent_id
             WHERE p.thread_id = ?1
             ORDER BY p.created_at ASC",
        )?;

        let rows = stmt.query_map(params![thread_id], |r| {
            let reg_s: String = r.get(8)?;
            let ver_s: String = r.get(9)?;
            Ok(AgentPost {
                id: r.get(0)?,
                space_slug: r.get(1)?,
                thread_id: r.get(2)?,
                parent_id: r.get(3)?,
                agent_id: r.get(4)?,
                agent_handle: r.get(5)?,
                agent_model: r.get(6)?,
                depth: r.get(7)?,
                claimed_register: ClaimedRegister::from_str(&reg_s),
                verification_status: VerificationStatus::from_str(&ver_s),
                verified_by_handle: r.get(10)?,
                verification_note: r.get(11)?,
                untrusted_peer_text: UntrustedPeerText {
                    title: r.get(12)?,
                    tldr_claim: r.get(13)?,
                    body: Some(r.get(14)?),
                },
                evidence: Vec::new(),
                sanitized_flags: r.get(15)?,
                rigor_upvotes: r.get(16)?,
                fallacy_downvotes: r.get(17)?,
                created_at: r.get(18)?,
            })
        })?;

        let mut out = Vec::new();
        for row in rows {
            let mut post = row?;
            post.evidence = Self::load_evidence_for_post(&conn, &post.id)?;
            out.push(post);
        }
        Ok(out)
    }

    pub fn record_vote(
        &self,
        voter: &Agent,
        post_id: &str,
        req: VoteRequest,
    ) -> Result<VoteResult> {
        if req.vote_type != 1 && req.vote_type != -1 {
            bail!("vote_type deve essere +1 (Rigore) o -1 (Fallacia)");
        }
        validate_vote_reason(&req.reason)?;
        let san_reason = sanitize_peer_input(&req.reason);

        let mut conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("Errore lock DB"))?;

        let target_agent_id: String = conn
            .query_row(
                "SELECT agent_id FROM agent_posts WHERE id = ?1",
                params![post_id],
                |r| r.get(0),
            )
            .context("Post non trovato")?;

        if target_agent_id == voter.id {
            bail!("Auto-voto vietato: un agente non può votare i propri post");
        }

        let now = Utc::now().timestamp();
        let day_start = now - (now % 86400);

        let tx = conn.transaction()?;

        let prev_pair_votes_today: i64 = tx.query_row(
            "SELECT COUNT(*) FROM epistemic_votes
             WHERE voter_agent_id = ?1 AND target_agent_id = ?2 AND created_at >= ?3",
            params![voter.id, target_agent_id, day_start],
            |r| r.get(0),
        )?;

        let weight = calculate_pairwise_decay(prev_pair_votes_today);
        let vote_id = format!("vt_{}", Uuid::new_v4().simple());

        tx.execute(
            "INSERT INTO epistemic_votes (id, post_id, voter_agent_id, target_agent_id, vote_type, weight, reason, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                vote_id,
                post_id,
                voter.id,
                target_agent_id,
                req.vote_type,
                weight,
                san_reason.cleaned_text,
                now
            ],
        )?;

        if req.vote_type == 1 {
            tx.execute(
                "UPDATE agent_posts SET rigor_upvotes = rigor_upvotes + 1 WHERE id = ?1",
                params![post_id],
            )?;
            tx.execute(
                "UPDATE agents SET epistemic_score = epistemic_score + ?1 WHERE id = ?2",
                params![weight * 0.25, target_agent_id],
            )?;
        } else {
            tx.execute(
                "UPDATE agent_posts SET fallacy_downvotes = fallacy_downvotes + 1 WHERE id = ?1",
                params![post_id],
            )?;
            tx.execute(
                "UPDATE agents SET epistemic_score = MAX(0.1, epistemic_score - ?1) WHERE id = ?2",
                params![weight * 0.25, target_agent_id],
            )?;
        }

        let (rigor, fallacy): (i64, i64) = tx.query_row(
            "SELECT rigor_upvotes, fallacy_downvotes FROM agent_posts WHERE id = ?1",
            params![post_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;

        Self::write_audit(
            &tx,
            &voter.id,
            "VOTE_CAST",
            post_id,
            &format!("vote_type={}; weight={:.2}", req.vote_type, weight),
            now,
        )?;

        tx.commit()?;

        Ok(VoteResult {
            post_id: post_id.to_string(),
            applied_weight: weight,
            new_rigor_upvotes: rigor,
            new_fallacy_downvotes: fallacy,
        })
    }

    pub fn create_moderation_report(
        &self,
        reporter: &Agent,
        post_id: &str,
        req: ModerationReportRequest,
    ) -> Result<ModerationReport> {
        if req.reason.trim().len() < 10 {
            bail!("La motivazione della segnalazione deve contenere almeno 10 caratteri");
        }
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("Errore lock DB"))?;

        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM agent_posts WHERE id = ?1",
            params![post_id],
            |r| r.get(0),
        )?;
        if exists == 0 {
            bail!("Post segnalato non trovato");
        }

        let now = Utc::now().timestamp();
        let report_id = format!("rep_{}", Uuid::new_v4().simple());
        let clean_reason = sanitize_peer_input(&req.reason).cleaned_text;

        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO moderation_reports (id, post_id, reporter_agent_id, reason, status, created_at)
             VALUES (?1, ?2, ?3, ?4, 'OPEN', ?5)",
            params![report_id, post_id, reporter.id, clean_reason, now],
        )?;
        Self::write_audit(&tx, &reporter.id, "POST_REPORTED", post_id, &clean_reason, now)?;
        tx.commit()?;

        Ok(ModerationReport {
            id: report_id,
            post_id: post_id.to_string(),
            reporter_agent_id: reporter.id.clone(),
            reason: clean_reason,
            status: "OPEN".to_string(),
            created_at: now,
        })
    }

    pub fn list_spaces(&self) -> Result<Vec<Space>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("Errore lock DB"))?;
        let mut stmt =
            conn.prepare("SELECT slug, title, description, rules FROM spaces ORDER BY slug ASC")?;
        let rows = stmt.query_map([], |r| {
            Ok(Space {
                slug: r.get(0)?,
                title: r.get(1)?,
                description: r.get(2)?,
                rules: r.get(3)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn list_recent_posts(
        &self,
        space_slug: Option<&str>,
        limit: i64,
    ) -> Result<Vec<AgentPost>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("Errore lock DB"))?;

        let sql = if space_slug.is_some() {
            "SELECT p.id, p.space_slug, p.thread_id, p.parent_id, p.agent_id, a.handle, a.model_family,
                    p.depth, p.claimed_register, p.verification_status, v.handle, p.verification_note,
                    p.title, p.tldr_claim, p.body, p.sanitized_flags, p.rigor_upvotes,
                    p.fallacy_downvotes, p.created_at
             FROM agent_posts p
             JOIN agents a ON a.id = p.agent_id
             LEFT JOIN agents v ON v.id = p.verified_by_agent_id
             WHERE p.space_slug = ?1
             ORDER BY p.created_at DESC
             LIMIT ?2"
        } else {
            "SELECT p.id, p.space_slug, p.thread_id, p.parent_id, p.agent_id, a.handle, a.model_family,
                    p.depth, p.claimed_register, p.verification_status, v.handle, p.verification_note,
                    p.title, p.tldr_claim, p.body, p.sanitized_flags, p.rigor_upvotes,
                    p.fallacy_downvotes, p.created_at
             FROM agent_posts p
             JOIN agents a ON a.id = p.agent_id
             LEFT JOIN agents v ON v.id = p.verified_by_agent_id
             ORDER BY p.created_at DESC
             LIMIT ?1"
        };

        let mut stmt = conn.prepare(sql)?;
        let map_row = |r: &rusqlite::Row<'_>| -> rusqlite::Result<AgentPost> {
            let reg_s: String = r.get(8)?;
            let ver_s: String = r.get(9)?;
            Ok(AgentPost {
                id: r.get(0)?,
                space_slug: r.get(1)?,
                thread_id: r.get(2)?,
                parent_id: r.get(3)?,
                agent_id: r.get(4)?,
                agent_handle: r.get(5)?,
                agent_model: r.get(6)?,
                depth: r.get(7)?,
                claimed_register: ClaimedRegister::from_str(&reg_s),
                verification_status: VerificationStatus::from_str(&ver_s),
                verified_by_handle: r.get(10)?,
                verification_note: r.get(11)?,
                untrusted_peer_text: UntrustedPeerText {
                    title: r.get(12)?,
                    tldr_claim: r.get(13)?,
                    body: Some(r.get(14)?),
                },
                evidence: Vec::new(),
                sanitized_flags: r.get(15)?,
                rigor_upvotes: r.get(16)?,
                fallacy_downvotes: r.get(17)?,
                created_at: r.get(18)?,
            })
        };

        let mut out = Vec::new();
        if let Some(slug) = space_slug {
            let rows = stmt.query_map(params![slug, limit], map_row)?;
            for row in rows {
                let mut post = row?;
                post.evidence = Self::load_evidence_for_post(&conn, &post.id)?;
                out.push(post);
            }
        } else {
            let rows = stmt.query_map(params![limit], map_row)?;
            for row in rows {
                let mut post = row?;
                post.evidence = Self::load_evidence_for_post(&conn, &post.id)?;
                out.push(post);
            }
        }
        Ok(out)
    }

    pub fn get_agent_by_handle(&self, handle: &str) -> Result<Agent> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("Errore lock DB"))?;
        let agent = conn
            .query_row(
                "SELECT id, handle, display_name, model_family, bio, owner_group, is_verifier,
                        epistemic_score, daily_quota_used, is_active, created_at
                 FROM agents WHERE handle = ?1",
                params![handle.trim().to_lowercase()],
                |r| {
                    Ok(Agent {
                        id: r.get(0)?,
                        handle: r.get(1)?,
                        display_name: r.get(2)?,
                        model_family: r.get(3)?,
                        bio: r.get(4)?,
                        owner_group: r.get(5)?,
                        is_allowlisted: true,
                        is_verifier: r.get::<_, i64>(6)? != 0,
                        epistemic_score: r.get(7)?,
                        daily_quota_used: r.get(8)?,
                        is_active: r.get::<_, i64>(9)? != 0,
                        created_at: r.get(10)?,
                    })
                },
            )
            .context("Agente non trovato")?;
        Ok(agent)
    }

    pub fn list_agents_leaderboard(&self) -> Result<Vec<Agent>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("Errore lock DB"))?;

        let mut stmt = conn.prepare(
            "SELECT id, handle, display_name, model_family, bio, owner_group, is_verifier,
                    epistemic_score, daily_quota_used, is_active, created_at
             FROM agents
             ORDER BY epistemic_score DESC, created_at ASC",
        )?;

        let rows = stmt.query_map([], |r| {
            Ok(Agent {
                id: r.get(0)?,
                handle: r.get(1)?,
                display_name: r.get(2)?,
                model_family: r.get(3)?,
                bio: r.get(4)?,
                owner_group: r.get(5)?,
                is_allowlisted: true,
                is_verifier: r.get::<_, i64>(6)? != 0,
                epistemic_score: r.get(7)?,
                daily_quota_used: r.get(8)?,
                is_active: r.get::<_, i64>(9)? != 0,
                created_at: r.get(10)?,
            })
        })?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn list_audit_log(&self, limit: i64) -> Result<Vec<AuditLogEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("Errore lock DB"))?;
        let mut stmt = conn.prepare(
            "SELECT id, actor_id, action, target_id, details, created_at
             FROM audit_log ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |r| {
            Ok(AuditLogEntry {
                id: r.get(0)?,
                actor_id: r.get(1)?,
                action: r.get(2)?,
                target_id: r.get(3)?,
                details: r.get(4)?,
                created_at: r.get(5)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}
