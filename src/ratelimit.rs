//! Scudo anti-traffico v0 (07/11/2026, Sempre).
//!
//! Due difese a costo zero, senza nuove dipendenze:
//! 1. `RateLimiter`: finestra scorrevole per chiave IP (max N richieste / M secondi).
//! 2. `rate_limit_middleware`: middleware Axum che identifica il client reale
//!    dietro Cloudflare Tunnel (`CF-Connecting-IP`, poi `X-Forwarded-For`).
//!    Dietro il tunnel il peer TCP è sempre cloudflared, quindi limitare sul
//!    peer IP sarebbe un unico secchio condiviso: sbagliato.
//! 3. Il limite di 64KB sul body vive in `api.rs` (`RequestBodyLimitLayer`).
//!
//! La logica di finestra è pura (`check(&mut self, key, now)`) così i test
//! sono deterministici senza sleep.

use axum::{
    body::Body,
    extract::{ConnectInfo, State},
    http::{header, HeaderMap, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::time::Instant;

use crate::AppState;

/// Finestra scorrevole: al massimo `max_hits` richieste per `window` per chiave.
pub struct RateLimiter {
    max_hits: usize,
    window: Duration,
    hits: HashMap<String, Vec<Instant>>,
}

impl RateLimiter {
    pub fn new(max_hits: usize, window_secs: u64) -> Self {
        Self {
            max_hits: max_hits.max(1),
            window: Duration::from_secs(window_secs.max(1)),
            hits: HashMap::new(),
        }
    }

    /// `true` = richiesta ammessa (e registrata). Deterministico a parità di `now`.
    pub fn check(&mut self, key: &str, now: Instant) -> bool {
        let cutoff = now.checked_sub(self.window).unwrap_or(now);
        let entry = self.hits.entry(key.to_string()).or_default();
        entry.retain(|t| *t > cutoff);
        if entry.len() >= self.max_hits {
            return false;
        }
        entry.push(now);
        // Tetto anti-crescita: se le chiavi esplodono, spazza quelle scadute.
        if self.hits.len() > 8192 {
            self.hits
                .retain(|_, v| !v.iter().all(|t| *t <= cutoff));
        }
        true
    }

    pub fn window_secs(&self) -> u64 {
        self.window.as_secs().max(1)
    }
}

/// Chiave di limiting: IP reale dietro Cloudflare, mai il peer TCP del tunnel.
pub fn client_ip_key(headers: &HeaderMap, req: &Request<Body>) -> String {
    if let Some(s) = first_header_value(headers, "cf-connecting-ip") {
        return format!("cf:{s}");
    }
    if let Some(v) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if let Some(first) = v
            .split(',')
            .next()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty() && s.len() <= 45)
        {
            return format!("xff:{first}");
        }
    }
    if let Some(s) = first_header_value(headers, "x-real-ip") {
        return format!("xrip:{s}");
    }
    if let Some(ci) = req.extensions().get::<ConnectInfo<SocketAddr>>() {
        return format!("peer:{}", ci.0.ip());
    }
    "unknown".to_string()
}

fn first_header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty() && s.len() <= 45)
}

/// 429 con `Retry-After` se la chiave ha esaurito la finestra.
pub async fn rate_limit_middleware(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    let key = client_ip_key(req.headers(), &req);
    let now = Instant::now();
    let (allowed, retry_after) = {
        let mut rl = state.rate_limiter.lock().unwrap_or_else(|e| e.into_inner());
        let ok = rl.check(&key, now);
        (ok, rl.window_secs())
    };
    if !allowed {
        let res = (
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, retry_after.to_string())],
            Json(json!({
                "error": "Rate limit superato: troppe richieste. Riprova tra poco."
            })),
        )
            .into_response();
        return Err(res);
    }
    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_allows_then_blocks_then_resets() {
        let mut rl = RateLimiter::new(3, 60);
        let t0 = Instant::now();
        assert!(rl.check("a", t0));
        assert!(rl.check("a", t0));
        assert!(rl.check("a", t0));
        assert!(!rl.check("a", t0), "la quarta nella stessa finestra va bloccata");
        // Altra chiave indipendente
        assert!(rl.check("b", t0));
        // Dopo la finestra, di nuovo ammessa
        let t1 = t0 + Duration::from_secs(61);
        assert!(rl.check("a", t1));
        assert!(rl.check("a", t1));
        assert!(rl.check("a", t1));
        assert!(!rl.check("a", t1));
    }

    #[test]
    fn min_bounds_are_sane() {
        let mut rl = RateLimiter::new(0, 0);
        let t0 = Instant::now();
        assert!(rl.check("a", t0));
        assert!(!rl.check("a", t0));
    }
}
