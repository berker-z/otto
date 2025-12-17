use anyhow::Result;
use clap::Parser;
use otto::app;
use otto::cli::Cli;

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file if it exists
    dotenvy::dotenv().ok();
    init_tracing();

    let cli = Cli::parse();
    app::run(cli).await
}

fn init_tracing() {
    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .finish();

    let _ = tracing::subscriber::set_global_default(subscriber);
}
