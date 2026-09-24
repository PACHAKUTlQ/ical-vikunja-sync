mod config;
mod db;
mod ical;
mod sync;
mod vikunja;

use anyhow::Result;
use clap::Parser;
use config::Config;
use db::Database;
use std::{collections::HashSet, path::PathBuf};
use vikunja::Vikunja;

#[derive(Debug, Parser)]
#[command(
    name = "ical-vikunja-sync",
    version,
    about = "Synchronize iCalendar VEVENT occurrences into Vikunja tasks"
)]
struct Args {
    #[arg(long, default_value = "config.toml", env = "ICAL_VIKUNJA_CONFIG")]
    config: PathBuf,
    #[arg(
        long,
        default_value = "ical-vikunja.sqlite3",
        env = "ICAL_VIKUNJA_DATABASE"
    )]
    database: PathBuf,
    #[arg(long, default_value = "data", env = "ICAL_VIKUNJA_DATA_DIR")]
    data_dir: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config = Config::load(&args.config)?;
    let database = Database::open(&args.database)?;
    let api = Vikunja::new(
        &config.vikunja.url,
        &config.vikunja.token,
        config.vikunja.timeout_seconds,
    )?;

    let configured: HashSet<&str> = config.feeds.iter().map(|feed| feed.id.as_str()).collect();
    for old_id in database.configured_feed_ids()? {
        if !configured.contains(old_id.as_str()) {
            println!(
                "warning: feed {old_id} was removed from configuration; its Vikunja project and tasks were left untouched"
            );
        }
    }

    let mut failures = Vec::new();
    for feed in &config.feeds {
        match sync::run_feed(&database, &api, feed, &args.data_dir).await {
            Ok(()) => println!("synchronized feed {}", feed.id),
            Err(error) => {
                eprintln!("error: feed {}: {error:#}", feed.id);
                failures.push(feed.id.clone());
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("{} feed(s) failed: {}", failures.len(), failures.join(", "))
    }
}
