use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, TimeZone, Utc};
use chrono_tz::Tz;
use icalendar::{Calendar, CalendarDateTime, Component, DatePerhapsTime, EventLike};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

const RECURRENCE_LIMIT: u16 = 50_000;

#[derive(Debug, Clone)]
pub struct Occurrence {
    pub key: String,
    pub uid: String,
    pub title: String,
    pub description: String,
    pub location: String,
    pub start: DateTime<Utc>,
    pub end: Option<DateTime<Utc>>,
    pub all_day: bool,
    pub hash: String,
}

pub fn read_local(path: &Path) -> Result<String> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("cannot read local calendar {}", path.display()))?;
    if text.trim().is_empty() {
        bail!("local calendar {} is empty", path.display());
    }
    Ok(text)
}

pub fn parse(text: &str, lower: DateTime<Utc>, upper: DateTime<Utc>) -> Result<Vec<Occurrence>> {
    let calendar: Calendar = text
        .parse()
        .map_err(|error| anyhow!("iCalendar parse error: {error}"))?;
    let mut result = Vec::new();

    for event in calendar.events() {
        let uid = event.get_uid().unwrap_or("(missing-uid)").to_owned();
        let start_value = event
            .get_start()
            .ok_or_else(|| anyhow!("VEVENT {uid} has no DTSTART"))?;
        let all_day = matches!(start_value, DatePerhapsTime::Date(_));
        let first_start = to_utc(&start_value)?;
        let event_end = event.get_end().map(|value| to_utc(&value)).transpose()?;
        let duration = event_end.map(|end| end - first_start);

        let has_recurrence_properties = ["RRULE", "RDATE", "EXDATE"].iter().any(|name| {
            event.properties().contains_key(*name) || event.multi_properties().contains_key(*name)
        });

        let starts = match event.get_recurrence() {
            Ok(recurrence) => {
                let dates = recurrence.all(RECURRENCE_LIMIT).dates;
                if dates.len() >= usize::from(RECURRENCE_LIMIT) {
                    bail!(
                        "VEVENT {uid} reached the {RECURRENCE_LIMIT}-occurrence expansion limit; \
                         refusing to reconcile an incomplete feed"
                    );
                }
                dates
                    .into_iter()
                    .map(|date| date.with_timezone(&Utc))
                    .collect::<Vec<_>>()
            }
            Err(error) if has_recurrence_properties => {
                bail!("VEVENT {uid} has an invalid recurrence: {error:?}");
            }
            Err(_) => vec![first_start],
        };

        for start in starts {
            if start < lower || start > upper {
                continue;
            }
            let end = duration.map(|value| start + value);
            let summary = event.get_summary().unwrap_or("Untitled event").to_owned();
            let description = event.get_description().unwrap_or_default().to_owned();
            let location = event.get_location().unwrap_or_default().to_owned();
            let key = format!("{}|{}", uid, start.to_rfc3339());
            let hash = fingerprint(&uid, &summary, &description, &location, start, end);
            result.push(Occurrence {
                key,
                uid: uid.clone(),
                title: summary,
                description,
                location,
                start,
                end,
                all_day,
                hash,
            });
        }
    }
    Ok(result)
}

fn to_utc(value: &DatePerhapsTime) -> Result<DateTime<Utc>> {
    match value {
        DatePerhapsTime::Date(date) => {
            Ok(Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0).unwrap()))
        }
        DatePerhapsTime::DateTime(CalendarDateTime::Utc(value)) => Ok(*value),
        DatePerhapsTime::DateTime(CalendarDateTime::Floating(value)) => {
            Ok(Utc.from_utc_datetime(value))
        }
        DatePerhapsTime::DateTime(CalendarDateTime::WithTimezone { date_time, tzid }) => {
            let timezone: Tz = tzid
                .parse()
                .with_context(|| format!("unknown TZID {tzid}"))?;
            timezone
                .from_local_datetime(date_time)
                .single()
                .map(|value| value.with_timezone(&Utc))
                .ok_or_else(|| anyhow!("ambiguous or invalid local time {date_time} in {tzid}"))
        }
    }
}

fn fingerprint(
    uid: &str,
    title: &str,
    description: &str,
    location: &str,
    start: DateTime<Utc>,
    end: Option<DateTime<Utc>>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(uid.as_bytes());
    hasher.update([0]);
    hasher.update(title.as_bytes());
    hasher.update([0]);
    hasher.update(description.as_bytes());
    hasher.update([0]);
    hasher.update(location.as_bytes());
    hasher.update([0]);
    hasher.update(start.to_rfc3339().as_bytes());
    hasher.update(
        end.map(|value| value.to_rfc3339())
            .unwrap_or_default()
            .as_bytes(),
    );
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for &byte in digest.iter() {
        hex.push(HEX[(byte >> 4) as usize] as char);
        hex.push(HEX[(byte & 0x0f) as usize] as char);
    }
    hex
}

pub fn metadata(event: &Occurrence) -> String {
    let start = if event.all_day {
        event.start.format("%Y-%m-%d").to_string()
    } else {
        event.start.to_rfc3339()
    };
    let end = event
        .end
        .map(|value| {
            if event.all_day {
                value.format("%Y-%m-%d").to_string()
            } else {
                value.to_rfc3339()
            }
        })
        .unwrap_or_else(|| "none".to_owned());
    format!(
        "Managed by ical-vikunja-sync.\n\nSource UID: `{}`\nOccurrence start: `{}`\nOccurrence end: `{}`\n\n{}{}",
        event.uid,
        start,
        end,
        event.description,
        if event.location.is_empty() {
            String::new()
        } else {
            format!("\n\nLocation: {}", event.location)
        }
    )
}
