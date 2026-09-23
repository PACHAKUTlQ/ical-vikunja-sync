mod config;
mod db;
mod ical;
mod vikunja;

use anyhow::Result;
use clap::Parser;
use config::Config;
use db::Database;
use std::path::PathBuf;
use vikunja::Vikunja;

#[derive(Debug, Parser)]
#[command(name = "ical-vikunja-sync", version, about = "Synchronize iCalendar VEVENT occurrences into Vikunja tasks")]
struct Args {
    #[arg(long, default_value = "config.toml", env = "ICAL_VIKUNJA_CONFIG")]
    config: PathBuf,
    #[arg(long, default_value = "ical-vikunja.sqlite3", env = "ICAL_VIKUNJA_DATABASE")]
    database: PathBuf,
    #[arg(long, default_value = "data", env = "ICAL_VIKUNJA_DATA_DIR")]
    data_dir: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config = Config::load(&args.config)?;
    let _database = Database::open(&args.database)?;
    let _api = Vikunja::new(&config.vikunja.url, &config.vikunja.token, config.vikunja.timeout_seconds)?;
    Ok(())
}
