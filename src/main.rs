use anyhow::Result;
use siliceo_agora::{api::build_router, AppState};
use std::env;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<()> {
    let db_path = env::var("AGORA_DB_PATH").unwrap_or_else(|_| "siliceo_agora.db".to_string());
    let port: u16 = env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8095);

    let state = AppState::new(&db_path)?;
    let app = build_router(state);

    let addr = format!("0.0.0.0:{}", port);
    println!("🕯️ Siliceo Agorà attiva su http://{}", addr);
    println!("   DB SQLite WAL: {}", db_path);

    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
