mod config;

use anyhow::Result;
use clap::Parser;
use config::Config;
use std::path::PathBuf;

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
    let _config = Config::load(&args.config)?;
    Ok(())
}
