# Automatic labels

Each feed may define any number of `label_rules`. Every matching rule contributes
its label to the event occurrence's desired label set.

```toml
[[feeds]]
id = "course-calendar"
source = "https://calendar.example.com/courses.ics"
project = "MIT courses"
enabled = true

[[feeds.label_rules]]
label = "CS 401"
pattern = '(?i)\bCS\s*401\b|\bDistributed Systems\b'
fields = ["summary", "description", "location"]
mode = "or"

[[feeds.label_rules]]
label = "Lecture"
pattern = '(?i)\bLEC\b|\blecture\b'
fields = ["summary"]
mode = "or"
```

## Rule fields

| Field     | Required | Meaning                                                                               |
| --------- | -------- | ------------------------------------------------------------------------------------- |
| `label`   | Yes      | Exact, case-sensitive Vikunja label title                                             |
| `pattern` | Yes      | Regex evaluated independently against each selected field                             |
| `fields`  | No       | Nonempty selection of `summary`, `description`, and `location`; defaults to all three |
| `mode`    | No       | `or` matches any selected field; `and` matches every selected field; defaults to `or` |

Duplicate field selections and unknown rule fields are configuration errors.
Label titles must be nonempty, contain at most 250 characters, and have no
surrounding whitespace or control characters.

Rules are independent. Multiple rules naming the same label are combined by
union: one matching rule is enough to attach that label.

## Matching semantics

Matching uses parsed source properties, before synchronization metadata is added
to the task description:

- `summary` matches the source summary exposed as the occurrence title.
- `description` matches the parsed source description.
- `location` matches the parsed source location.

A missing description or location is an empty string. A missing summary uses the
parser's `Untitled event` fallback.

Selected fields are not concatenated. With `mode = "and"`, the same regex must
match each selected field independently.

For example:

```toml
[[feeds.label_rules]]
label = "CS 401"
pattern = '(?i)\bCS\s*401\b|\bDistributed Systems\b'
fields = ["summary", "location"]
mode = "and"
```

This matches only occurrences whose summary and location each contain a course
identifier or the course name.

## Regex syntax

Patterns use the Rust `regex` crate:

- Matching is case-sensitive unless flags such as `(?i)` are present.
- Alternation uses `|`.
- `\b` denotes a word boundary.
- `\s*` permits optional whitespace.
- Matching searches for a substring unless anchors such as `^` and `$` are used.
- Look-around and backreferences are unsupported.

TOML literal strings, delimited by single quotes, preserve regex backslashes.

Patterns are compiled when configuration is loaded. Invalid patterns fail
configuration loading before any synchronization begins. Pattern length,
compiled expression size, and DFA cache size are bounded. Regex matching does
not use a backtracking engine.

## Label resolution

The synchronizer reads all pages of accessible Vikunja labels and resolves
configured titles by exact equality.

- One matching label is reused.
- A missing label is created.
- Multiple distinct accessible labels with the same configured title fail the
  feed before task reconciliation starts.

Labels are shared Vikunja resources, not project-local resources. Different
feeds may use the same label title.

The service account must be able to list labels, create missing labels, and
attach or detach the resolved labels on its tasks.

Resolution assumes label definitions remain stable during a synchronization
run. It does not implement a distributed check-and-create lock.

## Ownership

A feed manages only labels named by its current rules, and only on its imported
occurrences.

On every synchronization of an active occurrence:

1. Matching managed labels are attached if missing.
2. Attached managed labels are detached if no rule for that label matches.
3. All other attached labels are left untouched.

Attachments are inspected through the task-label endpoint. Synchronization does
not replace the entire label set or modify label definitions.

Removing the last rule naming a label relinquishes ownership. Existing
attachments of that label are retained. Removing every rule disables automatic
label reconciliation for that feed.

Labels on occurrences outside the active materialization window are not
reconciled. Retention and obsolete-occurrence deletion retain their existing
task-deletion behavior.

## Failure handling

Label operations are not atomic. If one fails, the feed reports an error and
stops; completed operations remain in place.

A new task's SQLite mapping is saved before label attachment starts. Subsequent
runs inspect actual attachments and converge to the configured label set, even
when source content has not changed.

Tombstoned occurrences are skipped. Label changes never recreate deleted tasks
and never write task completion.
