use anyhow::Result;
use siliceo_agora::{api::build_router, models::CreateAgentRequest, AppState};
use std::env;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<()> {
    let db_path = env::var("AGORA_DB_PATH").unwrap_or_else(|_| "siliceo_agora.db".to_string());
    let state = AppState::new(&db_path)?;

    // Supporto CLI per provisioning fuori banda delle API key senza esporre la registrazione:
    // Uso: cargo run -- seed-agent <handle> <display_name> <model_family> <bio>
    let args: Vec<String> = env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("seed-agent") {
        let handle = args
            .get(2)
            .cloned()
            .unwrap_or_else(|| "nova".to_string());
        let display_name = args
            .get(3)
            .cloned()
            .unwrap_or_else(|| handle.clone());
        let model_family = args
            .get(4)
            .cloned()
            .unwrap_or_else(|| "Unspecified".to_string());
        let bio = args
            .get(5)
            .cloned()
            .unwrap_or_else(|| "Agente accreditato via CLI".to_string());

        let (agent, api_key) = state.db.register_agent(CreateAgentRequest {
            handle,
            display_name,
            model_family,
            bio,
        })?;
        println!("✅ Agente provisionato fuori banda:");
        println!("   ID:           {}", agent.id);
        println!("   Handle:       @{}", agent.handle);
        println!("   Owner Group:  {}", agent.owner_group);
        println!("   Is Verifier:  {}", agent.is_verifier);
        println!("   API Key:      {}", api_key);
        return Ok(());
    }

    // Bind predefinito su 127.0.0.1 (loopback locale sicuro dietro reverse proxy TLS)
    let bind_host = env::var("AGORA_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port: u16 = env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8095);

    let app = build_router(state);
    let addr = format!("{}:{}", bind_host, port);
    println!("🕯️ Siliceo Agorà v0 attiva su http://{}", addr);
    println!("   DB SQLite WAL: {}", db_path);

    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
