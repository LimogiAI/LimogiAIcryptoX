//! LimogiAICryptoX - Pure Rust Trading Backend
//!
//! Complete replacement for Python backend.

mod api;
mod db;
mod trading;

// Trading engine modules
mod auth;
mod config_manager;
mod event_system;
mod executor;
mod graph_manager;
mod order_book;
mod scanner;
mod slippage;
mod trading_config;
mod types;
mod ws_v2;

use crate::api::create_router;
use crate::db::Database;
use crate::trading::TradingEngine;

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::signal;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

/// Application state shared across all handlers
pub struct AppState {
    pub db: Database,
    pub engine: Arc<TradingEngine>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load environment variables
    dotenvy::dotenv().ok();

    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(false)
        .with_thread_ids(true)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("╔══════════════════════════════════════════════════════════╗");
    info!("║     LimogiAICryptoX - Pure Rust Trading Backend v1.0    ║");
    info!("╚══════════════════════════════════════════════════════════╝");

    // Get configuration from environment
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://krakencryptox:krakencryptox123@db:5432/krakencryptox".to_string());
    
    let api_key = std::env::var("KRAKEN_API_KEY").ok();
    let api_secret = std::env::var("KRAKEN_API_SECRET").ok();
    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "8000".to_string())
        .parse()
        .unwrap_or(8000);

    // Initialize database
    info!("Connecting to database...");
    let db = Database::new(&database_url).await?;
    info!("Database connected");

    // Initialize trading engine
    info!("Initializing trading engine...");
    let engine = Arc::new(TradingEngine::new(
        api_key,
        api_secret,
        db.clone(),
    ).await?);
    info!("Trading engine initialized");

    // Start the engine
    info!("Starting trading engine...");
    engine.start().await?;
    info!("Trading engine started");

    // Create application state
    let state = Arc::new(AppState { db, engine });

    // Create router with all API endpoints
    let app = create_router(state);

    // Start server
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Starting API server on http://{}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await?;
    
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("Server shutdown complete");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("Shutdown signal received, starting graceful shutdown...");
}
