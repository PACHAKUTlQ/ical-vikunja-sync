# Vikunja API subset

The CLI uses the Vikunja REST API. The configured URL should normally end in
`/api/v2`.

## Authentication

Every request uses:

```http
Authorization: Bearer <api-token>
```

The token is never sent as a query parameter. API redirects are rejected.
The configured URL must point directly to the API endpoint.

## Endpoints used

| Method   | Endpoint                       | Purpose                                                          |
| -------- | ------------------------------ | ---------------------------------------------------------------- |
| `GET`    | `/projects/{id}`               | Validate the stored project mapping and detect a deleted project |
| `POST`   | `/projects`                    | Create a dedicated project for a feed                            |
| `GET`    | `/tasks/{id}`                  | Detect a task deleted outside the synchronizer                   |
| `POST`   | `/projects/{project}/tasks`    | Create a task for a new occurrence                               |
| `PATCH`  | `/tasks/{task}`                | Update source-owned fields without changing completion           |
| `DELETE` | `/tasks/{task}`                | Remove obsolete or retained occurrences                          |
| `GET`    | `/labels`                      | List accessible labels and resolve configured titles             |
| `POST`   | `/labels`                      | Create a missing configured label                                |
| `GET`    | `/tasks/{task}/labels`         | Inspect actual task-label attachments                            |
| `POST`   | `/tasks/{task}/labels`         | Attach a matching managed label                                  |
| `DELETE` | `/tasks/{task}/labels/{label}` | Detach a nonmatching managed label                               |

Label collection endpoints use the documented pagination envelope, including
`items`, `page`, and `total_pages`. The server may cap the requested page size;
pagination termination follows `total_pages`, not the requested page size.

## Rich text and partial updates

Task and project creation requests include `?format=markdown`.

Task updates use:

```http
Content-Type: application/merge-patch+json
X-Vikunja-Format: markdown
```

The header selects Markdown because the API documents that PATCH query
parameters are not reliable for format selection.

Label creation writes only the title. It does not write rich-text descriptions.

## Task fields written

Creation and source changes write:

- `project_id`
- `title`
- `description`
- `start_date`
- `due_date`
- `end_date`

The source description, UID, occurrence range, and location are included in the
managed Markdown description.

Updates omit `done`. Creation includes `done: false`.

Labels are managed through dedicated attachment endpoints rather than the task
payload. Favorite state, assignees, comments, reactions, and unmanaged label
attachments are not written.

## Label ownership

Configured label titles are resolved by exact, case-sensitive equality among
accessible labels. Missing labels are created; ambiguous duplicate titles fail
the feed.

Only labels named in the feed's current configuration are managed. Matching
labels are attached and nonmatching managed labels are detached. Other
attachments remain unchanged.

Removing the last rule for a label relinquishes ownership and leaves existing
attachments in place. Label resources are never automatically deleted.

See `labels.md` for configuration and matching semantics.

## Error semantics

- HTTP `404` while reading a stored project disables that feed.
- HTTP `404` while reading or updating a stored task creates a tombstone.
- HTTP `404` while deleting an obsolete or retained task is treated as already
  deleted and creates a tombstone.
- Label endpoint failures abort the feed. A label endpoint's `404` is not
  interpreted as task deletion because it may refer to a missing label.
- Other failures abort that feed's synchronization. Already completed API and
  SQLite operations remain in place.
- A feed failure does not prevent other configured feeds from being attempted.
  The process exits unsuccessfully after all feeds have been attempted.

Response bodies and label pagination are bounded. Invalid response bodies,
response-read failures, and inconsistent label pagination are reported as
errors.
