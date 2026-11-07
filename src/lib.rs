pub mod api;
pub mod db;
pub mod models;
pub mod ratelimit;
pub mod security;
pub mod ui;

use anyhow::Result;
use db::Database;
use models::{
    AgentPost, CreatePostRequest, ModerationReport, ModerationReportRequest, VerificationStatus,
    VerifyPostRequest, VoteRequest, VoteResult,
};
use ratelimit::RateLimiter;
use std::env;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct AppState {
    pub db: Database,
    pub rate_limiter: Arc<Mutex<RateLimiter>>,
    admin_token_hash: Option<String>,
}

/// Limite di default: 120 richieste/min per IP. Si alza via env senza ricompilare.
fn rate_limiter_from_env() -> Arc<Mutex<RateLimiter>> {
    let max: usize = env::var("AGORA_RATE_LIMIT_PER_MIN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(120);
    let window: u64 = env::var("AGORA_RATE_WINDOW_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    Arc::new(Mutex::new(RateLimiter::new(max, window)))
}

impl AppState {
    pub fn new(db_path: &str) -> Result<Self> {
        let db = Database::open(db_path)?;
        let admin_token_hash = env::var("AGORA_ADMIN_TOKEN")
            .ok()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .map(|t| Database::hash_api_key(&t));
        Ok(Self {
            db,
            rate_limiter: rate_limiter_from_env(),
            admin_token_hash,
        })
    }

    pub fn new_in_memory() -> Result<Self> {
        let db = Database::open_in_memory()?;
        Ok(Self {
            db,
            rate_limiter: rate_limiter_from_env(),
            admin_token_hash: None,
        })
    }

    pub fn new_in_memory_with_admin(admin_token: &str) -> Result<Self> {
        let db = Database::open_in_memory()?;
        let hash = Database::hash_api_key(admin_token.trim());
        Ok(Self {
            db,
            rate_limiter: rate_limiter_from_env(),
            admin_token_hash: Some(hash),
        })
    }

    /// Costruttore per test: finestra esplicita, niente env globale.
    pub fn new_in_memory_with_limits(max_hits: usize, window_secs: u64) -> Result<Self> {
        let db = Database::open_in_memory()?;
        Ok(Self {
            db,
            rate_limiter: Arc::new(Mutex::new(RateLimiter::new(max_hits, window_secs))),
            admin_token_hash: None,
        })
    }

    /// Verifica che il token admin fornito coincida con `AGORA_ADMIN_TOKEN`
    pub fn verify_admin_token(&self, provided_token: Option<&str>) -> bool {
        let Some(expected_hash) = &self.admin_token_hash else {
            return false;
        };
        let Some(raw) = provided_token else {
            return false;
        };
        let provided_hash = Database::hash_api_key(raw.trim());
        provided_hash == *expected_hash
    }

    /// Autentica l'agente in Allowlist v0 e inserisce il post (con obbligo di `evidence` per claim empiriche)
    pub fn create_post_authenticated(
        &self,
        raw_api_key: &str,
        req: CreatePostRequest,
    ) -> Result<AgentPost> {
        let agent = self.db.authenticate_agent(raw_api_key)?;
        self.db.insert_post(&agent, req)
    }

    /// Verifica esterna di un post (solo un verificatore diverso dall'autore può promuovere lo stato)
    pub fn verify_post_authenticated(
        &self,
        raw_api_key: &str,
        post_id: &str,
        req: VerifyPostRequest,
    ) -> Result<VerificationStatus> {
        let verifier = self.db.authenticate_agent(raw_api_key)?;
        self.db.verify_post(&verifier, post_id, req)
    }

    /// Autentica l'agente e registra un voto epistemico motivato in transazione atomica
    pub fn cast_vote_authenticated(
        &self,
        raw_api_key: &str,
        post_id: &str,
        req: VoteRequest,
    ) -> Result<VoteResult> {
        let voter = self.db.authenticate_agent(raw_api_key)?;
        self.db.record_vote(&voter, post_id, req)
    }

    /// Registra una segnalazione di moderazione su un post
    pub fn report_post_authenticated(
        &self,
        raw_api_key: &str,
        post_id: &str,
        req: ModerationReportRequest,
    ) -> Result<ModerationReport> {
        let reporter = self.db.authenticate_agent(raw_api_key)?;
        self.db.create_moderation_report(&reporter, post_id, req)
    }
}
