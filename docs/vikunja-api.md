# Vikunja API subset

The CLI uses only the REST API described by the supplied OpenAPI document. The configured URL should normally end in `/api/v2`.

## Authentication

Every request uses:

```http
Authorization: Bearer <api-token>
```

The token is never sent as a query parameter.

## Endpoints used

| Method   | Endpoint                    | Purpose                                                          |
| -------- | --------------------------- | ---------------------------------------------------------------- |
| `GET`    | `/projects/{id}`            | Detect a deleted project and validate the stored project mapping |
| `POST`   | `/projects`                 | Create a dedicated project for a feed                            |
| `GET`    | `/tasks/{id}`               | Detect a task deleted outside the synchronizer                   |
| `POST`   | `/projects/{project}/tasks` | Create a task for a new occurrence                               |
| `PATCH`  | `/tasks/{task}`             | Update source-owned fields without changing completion           |
| `DELETE` | `/tasks/{task}`             | Remove obsolete or retained occurrences                          |

Rich-text requests use Markdown. Creation requests include `?format=markdown`; merge-patch updates use `X-Vikunja-Format: markdown` because the API documents that PATCH query parameters are not reliable for this format selection.

## Task fields written

Creation and source changes write:

- `project_id`
- `title`
- `description`
- `start_date`
- `due_date`
- `end_date`

The source description, UID, occurrence range, and location are included in the managed Markdown description. The implementation intentionally omits `done` from update payloads. The creation payload includes `done: false` only because new tasks must start incomplete.

## Error semantics

- HTTP `404` for a stored project disables that feed.
- HTTP `404` for a stored task creates a tombstone and prevents recreation.
- Other HTTP failures abort that feed's synchronization and leave its previous SQLite state intact as far as the completed operations permit.
- A feed failure does not prevent other configured feeds from being attempted; the process exits unsuccessfully after all feeds have been attempted.
