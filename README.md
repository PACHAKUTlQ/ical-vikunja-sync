# ical-vikunja-sync

A one-shot Rust CLI that imports iCalendar `VEVENT` occurrences into [Vikunja](https://github.com/go-vikunja/vikunja) through the Vikunja REST API. Requires Vikunja >= v2.7.0.

## Behavior

- Each configured feed has one dedicated Vikunja project.
- Local paths and HTTP(S) feed URLs are supported.
- A successful feed is parsed before any task reconciliation starts.
- Recurring events are materialized into individual tasks within the configured window.
- The default window is the current instant through four months in the future.
- Timed events use their start as `start_date` and `due_date`; the original end is written to `end_date` and to managed metadata in the description.
- All-day events are represented at midnight UTC, with their source range retained in metadata.
- Task completion is controlled exclusively by Vikunja. Synchronization never sends a changed `done` value during updates.
- A task deleted in Vikunja is recorded as a tombstone and is never recreated.
- A missing project disables that feed in SQLite and reports the condition.
- Removing a feed from TOML only emits a warning. It does not delete its project or tasks.
- `retention_days` deletes materialized tasks after their event end, then records tombstones.
- Source occurrences removed from a valid feed are deleted from the corresponding project when they are inside the active materialization window.

## Build and run

```sh
cargo build --release
install -Dm755 target/release/ical-vikunja-sync ~/.local/bin/ical-vikunja-sync
export VIKUNJA_API_TOKEN='...'
ical-vikunja-sync --config /etc/ical-vikunja/config.toml --database /var/lib/ical-vikunja/state.sqlite3 --data-dir /var/lib/ical-vikunja
```

The database is SQLite with WAL mode. The data directory stores successfully downloaded remote feeds as an audit/cache copy; cached data is not used after a failed fetch.

## systemd

`/etc/systemd/system/ical-vikunja-sync.service`:

```ini
[Unit]
Description=Synchronize iCalendar feeds into Vikunja
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
User=ical-vikunja
Group=ical-vikunja
ExecStart=/usr/local/bin/ical-vikunja-sync --config /etc/ical-vikunja/config.toml --database /var/lib/ical-vikunja/state.sqlite3 --data-dir /var/lib/ical-vikunja
EnvironmentFile=/etc/ical-vikunja/environment
```

`/etc/systemd/system/ical-vikunja-sync.timer`:

```ini
[Unit]
Description=Run iCalendar to Vikunja synchronization

[Timer]
OnBootSec=5min
OnUnitActiveSec=15min
Persistent=true

[Install]
WantedBy=timers.target
```

Enable it with:

```sh
systemctl daemon-reload
systemctl enable --now ical-vikunja-sync.timer
```

## Security notes

Use a dedicated Vikunja API token with only the permissions needed for the target projects. Store the token in an environment file readable only by the service account. Do not place the expanded token in the TOML file or command-line arguments.

The current implementation sends the complete managed description on source changes. User-managed fields such as completion, favorite state, labels, assignees, comments, and reactions are not modified by synchronization.
