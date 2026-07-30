# Getting Started

This guide wires up a single entity, `Product`, end to end using the
fully-default stack (`DefaultRepo` → `DefaultService` → `DefaultViewSet`).
By the end you'll have working `GET/POST /products` and
`GET/PUT/PATCH/DELETE /products/{id}` routes.

## 1. Dependencies

`viewset` builds on these crates; add whichever your `Cargo.toml` doesn't
already have:

```toml
[dependencies]
actix-web = "4"
sqlx = { version = "0.8", features = ["postgres", "runtime-tokio", "uuid", "chrono", "rust_decimal"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
rust_decimal = { version = "1", features = ["serde"] }
async-trait = "0.1"
thiserror = "1"
```

`viewset` itself is exposed from `actixutils::viewset` (or as its own crate,
depending on how your workspace is laid out), together with the
`viewset-macros` crate for `#[derive(Entity)]`.

## 2. Set up a connection pool

Nothing in `viewset` creates the pool for you — build a standard
`sqlx::PgPool` the way you already would:

```rust
use sqlx::postgres::PgPoolOptions;

let db = PgPoolOptions::new()
    .max_connections(10)
    .connect(&std::env::var("DATABASE_URL")?)
    .await?;
```

## 3. Define your table

```sql
CREATE TABLE products (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL,
    sku         TEXT NOT NULL UNIQUE,
    price_cents INTEGER NOT NULL,
    in_stock    BOOLEAN NOT NULL DEFAULT true,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at  TIMESTAMPTZ
);
```

## 4. Define the entity struct and its DTOs

You need four related shapes: the row itself, the `POST` payload, the
`PATCH`/`PUT` payload, and what's actually returned to clients.

```rust
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Product {
    pub id: Uuid,
    pub name: String,
    pub sku: String,
    pub price_cents: i32,
    pub in_stock: bool,
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ProductCreate {
    pub name: String,
    pub sku: String,
    pub price_cents: i32,
    #[serde(default = "default_true")]
    pub in_stock: bool,
}
fn default_true() -> bool { true }

#[derive(Debug, Deserialize, Serialize)]
pub struct ProductUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_cents: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_stock: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ProductResponse {
    pub id: Uuid,
    pub name: String,
    pub sku: String,
    pub price_cents: i32,
    pub in_stock: bool,
}

impl From<Product> for ProductResponse {
    fn from(p: Product) -> Self {
        Self { id: p.id, name: p.name, sku: p.sku, price_cents: p.price_cents, in_stock: p.in_stock }
    }
}
```

> The `#[serde(skip_serializing_if = "Option::is_none")]` on every
> `ProductUpdate` field is what makes `PATCH` a true partial update — a
> field the client didn't send stays absent from the serialized JSON, and
> the default `Repository::update_columns` treats an absent key as
> "leave this column alone" rather than "set it to `NULL`".

## 5. Implement `Entity`

```rust
use viewset::{Entity, SqlType};

impl Entity for Product {
    type Id = Uuid;
    type CreateDto = ProductCreate;
    type UpdateDto = ProductUpdate;
    type ResponseDto = ProductResponse;

    const TABLE: &'static str = "products";
    const COLUMNS: &'static [&'static str] =
        &["id", "name", "sku", "price_cents", "in_stock", "created_at", "deleted_at"];
    const FIELDS: &'static [(&'static str, SqlType)] = &[
        ("name", SqlType::Text),
        ("sku", SqlType::Text),
        ("price_cents", SqlType::Int4),
        ("in_stock", SqlType::Bool),
    ];
    const SEARCHABLE: &'static [&'static str] = &["name", "sku"];
    const SORTABLE: &'static [&'static str] = &["name", "price_cents", "created_at"];
    const FILTERABLE: &'static [&'static str] = &["in_stock"];
    const SOFT_DELETE_COLUMN: Option<&'static str> = Some("deleted_at");

    fn id(&self) -> Self::Id {
        self.id
    }
}
```

`COLUMNS` order must match what `FromRow` expects when the query builder
selects `SELECT {COLUMNS} FROM products`. `FIELDS` only needs to list the
columns that are actually writable through `CreateDto`/`UpdateDto` — leave
out server-generated columns like `id` and `created_at`.

This impl is normally generated for you by `#[derive(Entity)]` from
attributes on the struct rather than written by hand; write it manually
(as above) any time you want full control, or while the derive macro isn't
available in your build.

## 6. Wire up the default stack

```rust
use viewset::{DefaultRepo, DefaultService, DefaultViewSet, ViewSet};
use actix_web::{App, HttpServer, web};
use std::sync::Arc;

type ProductViewSet = DefaultViewSet<DefaultService<DefaultRepo<Product>>>;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let db = /* PgPool from step 2 */;

    HttpServer::new(move || {
        let products: Arc<ProductViewSet> = Arc::new(db.clone().into());

        App::new().configure(|cfg| {
            products.clone().configure(cfg, "/products");
        })
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await
}
```

That's it — `/products` now supports:

- `GET /products` — paginated list (`?page=`, `?page_size=`, `?sort=`,
  `?search=`, `?field=value` filters)
- `POST /products` — create from `ProductCreate`
- `GET /products/{id}` — retrieve one
- `PUT` / `PATCH /products/{id}` — full or partial update from
  `ProductUpdate`
- `DELETE /products/{id}` — soft-deletes (sets `deleted_at`) because
  `SOFT_DELETE_COLUMN` is set

## 7. Sanity-check it

```bash
curl -X POST localhost:8080/products \
  -H 'content-type: application/json' \
  -d '{"name":"Widget","sku":"WID-1","price_cents":1999}'

curl 'localhost:8080/products?search=widget&sort=-price_cents&page_size=10'

curl -X PATCH localhost:8080/products/<id> \
  -H 'content-type: application/json' \
  -d '{"in_stock":false}'
```

## Next steps

Once the default stack is working, see **Tutorials & Examples** for:

- Adding validation and permission checks with `Service` hooks
- Multi-tenant scoping via `RequestContext`
- Overriding a `Repository` method for a join or aggregate query
- Working with `?search=`, `?sort=`, and `?field=value` filtering in detail
