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
