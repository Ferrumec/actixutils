## [unreleased]

### 📚 Documentation

- Rewrote README.md, docs/index.md, docs/middleware/index.md, EXAMPLES.md, and
  TUTORIALS.md against the current `src/` tree: removed the stale `viewset`
  section (moved to its own crate back in `0.6.0-a`), moved `Session<T>` docs
  from `middleware` to `extractors`, replaced the removed `locals::SessionStore`
  trait with the current `locals::Store<Uuid, T>`-backed `SessionMiddleware<T>`
  in every example, fixed `RateLimiter::new` call sites to include the now-required
  `store` argument, and documented previously-undocumented middleware
  (`Cache`, `Singleflight`, `TimeoutMiddleware`, `ClientIpMiddleware`, `PathParams`,
  `Permissions`) and the `ClientIp` extractor

### ⚙️ Miscellaneous Tasks

- *(version)* Bumped to v0.6.5
## [0.6.5-a] - 2026-09-03

### 🚀 Features

- Added clientip middleware
- Added ClientIp extractor and middleware

### ⚙️ Miscellaneous Tasks

- *(version)* Bumped v0.6.5-a
## [0.6.4] - 2026-08-25

### 🚀 Features

- Added Send + Sync on trait Store
- Added more From<HashMap<_,_>> for Filters

### 🐛 Bug Fixes

- Filters now implements Deserialize and is a default feature. closes #1.
- Moved Session from jwt feature to default feature

### ⚙️ Miscellaneous Tasks

- *(version)* Bumped to v0.6.2
- *(version)* Bumbed to v0.6.3
- *(version)* Bumbed to v0.6.4
## [0.6.1-a] - 2026-08-15

### ⚙️ Miscellaneous Tasks

- *(docs)* Updated docs
- *(docs)* Updated docs
## [0.6.0-c] - 2026-08-15

### 🐛 Bug Fixes

- Breaking: removed MemoryStore

### ⚙️ Miscellaneous Tasks

- *(test)* Updated tests
## [0.6.0-d] - 2026-08-15

### ⚙️ Miscellaneous Tasks

- *(test)* Updated tests
## [0.6.0-b] - 2026-08-15

### 🐛 Bug Fixes

- Fixed memory store to use trait Store

### ⚙️ Miscellaneous Tasks

- *(tests)* Updated tests
- Formatting
## [0.6.0-a] - 2026-08-15

### 🚀 Features

- Added trait Store
- Added Filters and Session extractor exports

### 🐛 Bug Fixes

- Removed unnecessary Phantom markers
- Removed dashmap from rate limiting store
- Session middleware now uses trait Store for storage
- Breaking: removed SessionStore
- :breaking: Cache middleware now uses trait Store

### 🚜 Refactor

- Breaking: moved viewset to a separate crate. viewset is now a separate crate
## [0.5.1-d] - 2026-08-11

### ⚙️ Miscellaneous Tasks

- Clippy fixes
## [0.5.1-b] - 2026-08-11

### 🚀 Features

- Added PathParams middleware

### 🐛 Bug Fixes

- Made Filters to try extracting from request extenstions to capture PathParams middleware additions
- Integrated Filters into ViewSet
- Integrated Filters into ViewSet

### ⚙️ Miscellaneous Tasks

- *(version)* Bumped to v0.5.1
## [0.5.1] - 2026-08-09

### 🚀 Features

- Added coalesce middleware
- Added timeout middleware
- Breaking: added caching to repository

### 🐛 Bug Fixes

- Integrated cache into repository
- Integrated cache into repository

### 🚜 Refactor

- Refactored session store trait
- Moved Session<T> to extractors

### 🧪 Testing

- Fixed tests for coalesce and session middlewares

### ⚙️ Miscellaneous Tasks

- *(version)* Bumped to v0.4.3
- *(version)* Bumped to v0.5
## [0.4.3] - 2026-08-05

### 🚀 Features

- Added caching middleware

### ⚙️ Miscellaneous Tasks

- *(version)* Bumped to v0.4.2
## [0.4.2] - 2026-08-05

### 🚀 Features

- Added permission middleware
- Added permission middleware

### 🐛 Bug Fixes

- *(viewset)* [**breaking**] Repair broken filtering, search, soft-delete, and PK binding
- Made SessionMiddleware<T> to insert T into request extentions
- Replaced actix-router with actix-web::dev
- Reexported permission

### 📚 Documentation

- Added docs for viewset
- Linked viewset with index
- Linked viewset with index
- Added middlewares to docs
- Added middlewares to docs
- Added middlewares to docs

### ⚙️ Miscellaneous Tasks

- *(docs)* Added viewset docs index
- *(docs)* Added viewset docs index
## [0.3.1-r] - 2026-07-28

### ⚙️ Miscellaneous Tasks

- *(fmt)* Cargo formatting
- Cargo clippy cleanup
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
