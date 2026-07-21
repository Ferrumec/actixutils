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
