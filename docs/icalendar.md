# iCalendar parser notes

The implementation uses `icalendar` with its `parser`, `recurrence`, and `chrono-tz` features.

```rust
let calendar: icalendar::Calendar = text.parse()?;
for event in calendar.events() {
    let start = event.get_start();
    let end = event.get_end();
    let recurrences = event.get_recurrence();
}
```

## Supported input

- `VCALENDAR` containing `VEVENT` components.
- `DTSTART` and optional `DTEND`.
- UTC date-times ending in `Z`.
- `TZID` date-times, resolved using `chrono-tz`.
- Floating date-times, interpreted as UTC because a feed without a timezone reference cannot be resolved safely.
- `VALUE=DATE` all-day events.
- Recurrence rules supported by the `icalendar` recurrence integration, including recurrence dates and exception dates exposed by that integration.
- RFC 5545 folded lines and escaped text through the crate parser.

Each recurrence is represented by a stable key consisting of the source `UID` and the occurrence start. This key is stored in SQLite and is independent of the Vikunja task title.

## Materialization

The CLI asks the recurrence engine for a bounded number of occurrences and then filters the results to:

```text
now - window_past_months <= occurrence_start <= now + window_future_months
```

The bound prevents an unbounded rule from producing an infinite result. The current limit is 50,000 generated dates per VEVENT. Feeds with very old high-frequency recurrences should use a source feed that limits its recurrence or be split into narrower feeds.

## Deliberate limitations

- `VTODO` is not imported because the synchronization model treats every source event occurrence as a Vikunja task.
- A floating date-time has no authoritative timezone and is therefore treated as UTC.
- Ambiguous or nonexistent local times are rejected instead of silently choosing a DST interpretation.
- The source end for all-day events follows iCalendar's exclusive `DTEND` convention and is retained in the task metadata.
