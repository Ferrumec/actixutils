## [0.4] - 2026-07-31
### Fixed

- **`list()` produced invalid SQL whenever soft-delete or any filter/search
  param was used.** `select_qb` and `count_qb` shared a single `has_where`
  flag, so `count_qb` frequently got `... AND ...` with no preceding
  `WHERE`. Each query builder now tracks its own `has_where` independently.
  (`repository.rs`)

- **`?search=` never worked.** The generated `ILIKE` clauses were pushed
  without a corresponding `push_bind`, so any request using `search` on an
  entity with `SEARCHABLE` fields hit a SQL syntax error. Search patterns
  are now bound per field. (`repository.rs`)

- **`update()` bound the primary key as text instead of its native type**
  (`id.to_string()`), which Postgres rejects for non-text PKs (`uuid = text`
  has no operator). The id is now bound with its actual sqlx type, matching
  `retrieve`/`delete`/`exists`. (`repository.rs`)

- **`#[derive(Entity)]` hardcoded `type Id = uuid::Uuid`** regardless of the
  actual primary-key field's type, breaking any entity with a non-UUID PK
  (`i32`/`i64`/etc.). `Id` is now derived from the `#[entity(pk)]` field's
  real type (falling back to a field named `id`). The generated `id()`
  method now `.clone()`s the field instead of moving it, since the PK type
  is no longer guaranteed `Copy`. (`viewset-macros/lib.rs`)

- **`SqlValue::from_json` silently defaulted on bad input** — a malformed
  UUID became `Uuid::nil()`, a bad number became `0`, a bad date became
  the Unix epoch, an out-of-range `i64` silently truncated into `i32`, etc.
  It now returns `ApiResult<SqlValue>` and rejects mismatched values with
  `ApiError::Validation` instead of writing a plausible-looking wrong
  value. `insert_columns`/`update_columns`/`fields_from_dto` updated to
  propagate the `Result`. (`sql.rs`, `repository.rs`)

- **Soft delete assumed every `SOFT_DELETE_COLUMN` was a nullable
  timestamp**, unconditionally running `SET col = now()` and filtering
  `col IS NULL`. Boolean soft-delete columns now get `SET col = true` and
  `col IS NOT TRUE`, matching what the `Entity::SOFT_DELETE_COLUMN` docs
  already claimed was supported. (`repository.rs`)

- **5xx error responses leaked raw internal error text** — `ApiError::Database`/
  `Internal` serialized `self.to_string()` straight into the JSON body,
  potentially exposing table/column/constraint names to API clients. 5xx
  responses now log the real error via `tracing::error!` and return a
  generic `"internal server error"` body; 4xx bodies are unchanged.
  (`error.rs`)

- **No transactional boundary around create/update/delete.** A `before_*`
  hook and the actual write were two independent, non-atomic steps — a
  hook could "succeed" against data that was never persisted, or vice
  versa. `Service::create`/`update`/`delete` now open a transaction,
  run `before_*` → write → `after_*` inside it, and commit once at the
  end; any failure rolls the whole thing back. Hooks now receive
  `&mut Transaction<'_, Postgres>` so they can run their own queries
  atomically with the write. Added `Repository::create_in_tx`/
  `update_in_tx`/`delete_in_tx` and `Repository::transaction()` to
  support this. (`repository.rs`, `service.rs`)

### Added

- `Repository::transaction()` — opens a transaction against the
  repository's pool.
- `Repository::create_in_tx` / `update_in_tx` / `delete_in_tx` —
  transaction-scoped counterparts to `create`/`update`/`delete`, sharing
  logic with the pool-based versions via generic executor-parameterized
  free functions (`insert_row`, `update_row`, `delete_row`, `retrieve_row`).
- Doc comments capturing three deliberate scope decisions, so they read as
  intentional rather than incomplete:
  - `RequestContext` (`context.rs`) — why it's not threaded through trait
    signatures, the middleware + `tokio::task_local!` pattern this crate
    expects instead, and why that's sound (`ViewSet`'s default handlers
    never spawn tasks) plus the one case where it isn't (a hook that
    itself calls `tokio::spawn`).
  - `QueryParams::fields` / `::expand` (`pagination.rs`) — parsed but
    unused by any default method; reserved for developers overriding
    list handling to implement sparse fields / eager-loading themselves.
  - `ApiError::StaleVersion` (`error.rs`) — not produced by any default
    method; reserved as a ready-made 409 for an `update` override that
    adds optimistic locking.

### Changed (breaking)

- `Repository::insert_columns` / `update_columns` now return
  `ApiResult<Vec<(&'static str, SqlValue)>>` instead of
  `Vec<(&'static str, SqlValue)>`. Any override of these methods needs a
  signature update.
- `Service`'s `before_create`/`after_create`/`before_update`/`after_update`/
  `before_delete`/`after_delete` hooks now take an additional
  `&mut Transaction<'_, Postgres>` parameter. Any override of these hooks
  needs a signature update.
- `SqlValue::from_json` now takes a `field_name: &str` parameter (for
  error messages) and returns `ApiResult<Self>` instead of `Self`.

### Not changed (deferred by design)

- `RequestContext` remains unwired from `ViewSet`/`Service`/`Repository`
  signatures — intentional, see doc comment in `context.rs`.
- `QueryParams::fields`/`::expand` remain unconsumed by default methods —
  intentional, see doc comment in `pagination.rs`.
- `ApiError::StaleVersion` remains unproduced by default methods —
  intentional, see doc comment in `error.rs`.

## [0.3.1] - 2026-07-27

### 🚀 Features

- Added drfault implementations for ViewSet, Service and Repository
- Implemented From<T:Service> for DefaultViewSet<T>

### 🚜 Refactor

- Breaking: removed Service{ type User }

### ⚙️ Miscellaneous Tasks

- *(doc)* Referenced changelog in readme
- Bumped to v0.3.1
## [0.3] - 2026-07-23

### 🐛 Bug Fixes

- Breaking: trait Repository requires fn database defined instead of reading from request extraction

### 📚 Documentation

- Updated documentations

### ⚙️ Miscellaneous Tasks

- Bumped to v0.2.3
- Bumped to v0.2.3
- *(version)* Bumped to v0.3
## [0.2.2] - 2026-07-22

### 🐛 Bug Fixes

- Viewset exports

### 📚 Documentation

- Updated changelog

### ⚙️ Miscellaneous Tasks

- Bumped to v0.2.1
## [0.2.1] - 2026-07-21

### 🚀 Features

- Added required mode to SessionMiddleware.

### 🐛 Bug Fixes

- Tests for session middleware
- SessionStore::save now saves only if the session was modified.

### 📚 Documentation

- *(toml)* Added changelog reference to Cargo.toml

### ⚙️ Miscellaneous Tasks

- New version pins
## [0.2] - 2026-07-20

### 🚀 Features

- Added AttacbLocal<T> middleware for attaching values to task local variables
- Added session middleware
- Added Session middleware
- Added offset to Pagination

### 🐛 Bug Fixes

- Moved path specification to configure method
- Authority::check bug
- Added default on missing on Session middleware
- Broken-cookie session isn't persisted or re-issued
- Idempotency key never released on handler error
- Identity/Authority timestamps are 1000x too generous

### 🚜 Refactor

- Breaking: removed locals::utils
- Breaking: renamed Auth<T> extractor to Jwt<T>

### 📚 Documentation

- Added changelog

### ⚙️ Miscellaneous Tasks

- Fixed viewset-macro version
- *(release)* Bumped to v0.2
## [0.1.0] - 2026-06-24
