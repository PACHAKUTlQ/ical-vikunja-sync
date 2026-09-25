use crate::{
    config::FeedConfig,
    db::Database,
    ical::{Occurrence, metadata},
    vikunja::{Vikunja, is_not_found},
};
use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Duration, Months, Utc};
use serde_json::json;
use std::{collections::HashSet, fs, path::Path};
use url::Url;

pub async fn run_feed(
    db: &Database,
    api: &Vikunja,
    feed: &FeedConfig,
    data_dir: &Path,
) -> Result<()> {
    if !feed.enabled {
        println!("feed {} is disabled", feed.id);
        return Ok(());
    }
    let now = Utc::now();
    let lower = shift_months(now, -feed.window_past_months);
    let upper = shift_months(now, feed.window_future_months);
    let text = obtain(&feed.id, &feed.source, data_dir).await?;
    let occurrences = crate::ical::parse(&text, lower, upper)
        .with_context(|| format!("cannot parse feed {}", feed.id))?;
    db.ensure_feed(&feed.id)?;
    let project = ensure_project(db, api, feed).await?;
    let desired: HashSet<String> = occurrences.iter().map(|event| event.key.clone()).collect();

    for event in &occurrences {
        sync_occurrence(db, api, feed, project, event).await?;
    }
    for row in db.rows(&feed.id)? {
        if row.start >= lower && row.start <= upper && !desired.contains(&row.key) && !row.tombstone
        {
            if let Some(task_id) = row.task_id {
                match api.delete_task(task_id).await {
                    Ok(()) => db.save(
                        &feed.id, &row.key, None, true, row.start, row.end, &row.hash,
                    )?,
                    Err(error) if is_not_found(&error) => db.save(
                        &feed.id, &row.key, None, true, row.start, row.end, &row.hash,
                    )?,
                    Err(error) => {
                        return Err(anyhow!("cannot remove obsolete task {task_id}: {error}"));
                    }
                }
            } else {
                db.save(
                    &feed.id, &row.key, None, true, row.start, row.end, &row.hash,
                )?;
            }
        }
    }
    apply_retention(db, api, feed, now).await
}

async fn ensure_project(db: &Database, api: &Vikunja, feed: &FeedConfig) -> Result<i64> {
    let (project_id, disabled) = db.feed_state(&feed.id)?.unwrap_or((None, false));
    if disabled {
        return Err(anyhow!(
            "feed {} is disabled because its project was deleted",
            feed.id
        ));
    }
    if let Some(project_id) = project_id {
        match api.project(project_id).await {
            Ok(_) => return Ok(project_id),
            Err(error) if is_not_found(&error) => {
                db.disable(&feed.id)?;
                return Err(anyhow!(
                    "project {project_id} for feed {} no longer exists; import disabled",
                    feed.id
                ));
            }
            Err(error) => return Err(anyhow!("cannot read project {project_id}: {error}")),
        }
    }
    let project_id = api
        .create_project(&feed.project)
        .await
        .map_err(|error| anyhow!("cannot create project for {}: {error}", feed.id))?;
    db.project(&feed.id, project_id)?;
    println!("created Vikunja project {} ({})", project_id, feed.project);
    Ok(project_id)
}

async fn sync_occurrence(
    db: &Database,
    api: &Vikunja,
    feed: &FeedConfig,
    project: i64,
    event: &Occurrence,
) -> Result<()> {
    let existing = db.get(&feed.id, &event.key)?;
    if existing.as_ref().is_some_and(|row| row.tombstone) {
        return Ok(());
    }
    let Some(row) = existing else {
        let task_id = api
            .create_task(project, creation_payload(project, event))
            .await
            .map_err(|error| anyhow!("cannot create task for feed {}: {error}", feed.id))?;
        db.save(
            &feed.id,
            &event.key,
            Some(task_id),
            false,
            event.start,
            event.end,
            &event.hash,
        )?;
        return Ok(());
    };
    let Some(task_id) = row.task_id else {
        let task_id = api
            .create_task(project, creation_payload(project, event))
            .await
            .map_err(|error| anyhow!("cannot recreate task for feed {}: {error}", feed.id))?;
        db.save(
            &feed.id,
            &event.key,
            Some(task_id),
            false,
            event.start,
            event.end,
            &event.hash,
        )?;
        return Ok(());
    };
    match api.task(task_id).await {
        Ok(_) => {
            if row.hash != event.hash {
                api.update_task(task_id, task_payload(project, event))
                    .await
                    .map_err(|error| anyhow!("cannot update task {task_id}: {error}"))?;
                db.save(
                    &feed.id,
                    &event.key,
                    Some(task_id),
                    false,
                    event.start,
                    event.end,
                    &event.hash,
                )?;
            }
        }
        Err(error) if is_not_found(&error) => db.save(
            &feed.id,
            &event.key,
            None,
            true,
            event.start,
            event.end,
            &event.hash,
        )?,
        Err(error) => return Err(anyhow!("cannot inspect task {task_id}: {error}")),
    }
    Ok(())
}

fn creation_payload(project: i64, event: &Occurrence) -> serde_json::Value {
    let mut payload = task_payload(project, event);
    payload["done"] = json!(false);
    payload
}

fn task_payload(project: i64, event: &Occurrence) -> serde_json::Value {
    let start = event.start.to_rfc3339();
    let end = event.end.map(|value| value.to_rfc3339());
    json!({
        "project_id": project,
        "title": event.title,
        "description": metadata(event),
        "start_date": start,
        "due_date": start,
        "end_date": end
    })
}

async fn apply_retention(
    db: &Database,
    api: &Vikunja,
    feed: &FeedConfig,
    now: DateTime<Utc>,
) -> Result<()> {
    let Some(days) = feed.retention_days else {
        return Ok(());
    };
    let cutoff = now - Duration::days(days);
    for row in db.rows(&feed.id)? {
        if row.end.or(Some(row.start)).is_some_and(|end| end < cutoff) && !row.tombstone {
            if let Some(task_id) = row.task_id {
                match api.delete_task(task_id).await {
                    Ok(())
                    | Err(crate::vikunja::ApiError {
                        status: reqwest::StatusCode::NOT_FOUND,
                        ..
                    }) => {}
                    Err(error) => {
                        return Err(anyhow!(
                            "retention deletion of task {task_id} failed: {error}"
                        ));
                    }
                }
            }
            db.save(
                &feed.id, &row.key, None, true, row.start, row.end, &row.hash,
            )?;
        }
    }
    Ok(())
}

async fn obtain(feed_id: &str, source: &str, data_dir: &Path) -> Result<String> {
    if let Ok(url) = Url::parse(source) {
        if matches!(url.scheme(), "http" | "https") {
            let response = reqwest::get(url.clone())
                .await
                .with_context(|| format!("remote fetch failed for feed {feed_id}"))?;
            let response = response
                .error_for_status()
                .with_context(|| format!("remote feed returned an error for {feed_id}"))?;
            let text = response
                .text()
                .await
                .with_context(|| format!("cannot read remote feed {feed_id}"))?;
            if text.trim().is_empty() {
                return Err(anyhow!("remote feed {feed_id} is empty"));
            }
            let cache = data_dir.join("cache").join(format!("{feed_id}.ics"));
            if let Some(parent) = cache.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(cache, &text)?;
            return Ok(text);
        }
    }
    Ok(crate::ical::read_local(Path::new(source))?)
}

fn shift_months(value: DateTime<Utc>, months: i64) -> DateTime<Utc> {
    if months >= 0 {
        value
            .checked_add_months(Months::new(months as u32))
            .unwrap_or(value)
    } else {
        value
            .checked_sub_months(Months::new((-months) as u32))
            .unwrap_or(value)
    }
}
