# 0006. JSON preferences now; versioned session files later

Status: Accepted for user preferences. Proposed for sessions.

## Context

Three things can be persisted and must not be confused:

- the **application definition**: the dashboard, written by the developer.
  It is compiled into the executable; nothing to store.
- **user preferences**: theme, later recent files and chart preferences.
- **analysis sessions**: which data was loaded, filters, selections,
  parameters, results, software versions.

## Decision (implemented)

User preferences are one JSON file per application:

```text
<per-user config directory>/eario/<application-slug>/preferences.json
{ "version": 1, "theme": "dark" }
```

- The directory comes from the `directories` crate (for example
  `%APPDATA%\eario\config` on Windows, `~/.config/eario` on Linux). The slug
  is derived from the dashboard title.
- Writes go to a temporary file that is then renamed over the target, so an
  interrupted write leaves the previous file intact.
- A missing, unreadable or newer-versioned file yields the defaults and a
  log line. It never prevents start-up.
- `Dashboard::remember_preferences(false)` keeps preferences in memory.

## Direction (not implemented)

A session is a directory, or an archive of it:

```text
session.json        format version, Eario and application versions, timestamps,
                    data sources (path, size, hash, import options),
                    state map, component identifiers it refers to
data/*.parquet      optional embedded copies of datasets or derived results
```

- **JSON** for the manifest: readable, diffable, versioned by a `version`
  field with explicit migrations.
- **Parquet** (or Arrow IPC) for embedded tables: columnar, typed, compact.
- **SQLite** is reserved for what is naturally relational and append-heavy:
  an index of recent sessions, an analysis history. It is not a container for
  numerical datasets.
- Opening a session whose source file is missing or changed (hash mismatch)
  is reported, and the session opens with what it has.
- Saving writes to a temporary location then renames.

## Alternatives

- **SQLite for everything.** One file and transactions, but numerical columns
  stored as rows or blobs are slow and opaque.
- **TOML for preferences.** Friendlier to hand-editing; JSON was chosen
  because the rest of the system already speaks it and no one is expected to
  edit this file.
- **Browser `localStorage`.** Tied to an origin, and the port changes at each
  start.

## Consequences

- State is lost when the application stops. This is the most visible
  limitation after the closed component set.
- Component identifiers become part of a stored format once sessions exist;
  generated identifiers (`chart-1`) will not be stable enough.

## Open questions

- Whether a session embeds data by default or references it.
- Recording enough for reproducibility: the full chain of transformations,
  not only the final state.
- Where sessions live by default, and how they are named.
