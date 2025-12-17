use clap::Parser;

/// Command-line options for Otto.
#[derive(Parser, Debug)]
#[command(author, version, about)]
pub struct Cli {
    /// Add a new account via OAuth onboarding
    #[arg(long)]
    pub add_account: bool,

    /// Disable sync for this run (serve from cache only).
    #[arg(long)]
    pub no_sync: bool,

    /// Force full sync, bypassing MODSEQ optimization.
    #[arg(long)]
    pub force: bool,

    /// Force safe mode (disable mutations) even if account-level safe_mode is false.
    #[arg(long)]
    pub safe_mode: bool,
}
