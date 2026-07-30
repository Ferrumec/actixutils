# Tutorials & Examples

These build on the `Product` entity from **Getting Started**. Each tutorial
overrides one specific piece of the default stack.

## 1. Adding validation with `Service` hooks

`Service` gives you `before_create`/`after_create`,
`before_update`/`after_update`, `before_delete`/`after_delete`, and
`before_list`/`after_list` — all no-ops by default, all overridable
independently. This is the layer to add checks that don't belong in SQL.

```rust
use viewset::{Service, Repository, DefaultRepo, ApiError, ApiResult};
use async_trait::async_trait;

pub struct ProductService {
    repo: DefaultRepo<Product>,
}

impl From<sqlx::PgPool> for ProductService {
    fn from(db: sqlx::PgPool) -> Self {
        Self { repo: db.into() }
    }
}

#[async_trait]
impl Service for ProductService {
    type Repository = DefaultRepo<Product>;

    fn repository(&self) -> &Self::Repository {
        &self.repo
    }

    async fn before_create(&self, dto: ProductCreate) -> ApiResult<ProductCreate> {
        if dto.price_cents <= 0 {
            return Err(ApiError::Validation("price_cents must be positive".into()));
        }
        Ok(dto)
    }
}
```

Everything else (`list`, `retrieve`, `update`, `delete`) keeps using the
default implementation, which still calls your `before_create` hook because
hooks and default methods live on the same trait.

## 2. Authorization checks against `RequestContext`

`RequestContext<U>` carries `permissions: Arc<Vec<String>>` and a
`has_permission` helper. A common pattern is to hold a `RequestContext` on
your service (built by an actix-web extractor in your application) and
check it inside a hook:

```rust
async fn before_delete(&self, _id: &Uuid) -> ApiResult<()> {
    if !self.ctx.has_permission("products:delete") {
        return Err(ApiError::Forbidden);
    }
    Ok(())
}
```

Since `RequestContext` is generic over the user type `U`, your application
defines its own claims/user struct and implements `FromRequest` for
`RequestContext<YourUser>` — `viewset` doesn't assume any particular auth
mechanism.

## 3. Soft delete in practice

Setting `SOFT_DELETE_COLUMN` on `Entity` changes behavior at three points
in the default `Repository`, with no extra code needed:

- `delete()` issues `UPDATE products SET deleted_at = now() WHERE id = $1`
  instead of a `DELETE`.
- `list()` and `retrieve()` both add `WHERE deleted_at IS NULL` (or
  `AND deleted_at IS NULL` if another filter already opened the clause).

If you need an admin endpoint that can see soft-deleted rows too (e.g. a
"trash" view), override `list` directly rather than trying to bypass the
clause through query params:

```rust
async fn list_including_deleted(&self, query: &QueryParams) -> ApiResult<(Vec<Product>, i64)> {
    // Build your own QueryBuilder here, omitting the deleted_at filter —
    // the default list() always applies it when SOFT_DELETE_COLUMN is set.
    todo!()
}
```

## 4. Overriding a `Repository` method for a join

The default `Repository` methods cover single-table CRUD. For anything
involving a join, aggregate, or window function, override the specific
method — everything else keeps using the generated SQL:

```rust
#[async_trait]
impl Repository for ProductRepo {
    type Entity = Product;

    fn database(&self) -> &sqlx::PgPool {
        &self.db
    }

    // Override just retrieve() to also pull category name via a join,
    // rather than trying to express that through Entity::COLUMNS.
    async fn retrieve(&self, id: &Uuid) -> ApiResult<Product> {
        sqlx::query_as::<_, Product>(
            "SELECT p.* FROM products p
             JOIN categories c ON c.id = p.category_id
             WHERE p.id = $1 AND p.deleted_at IS NULL"
        )
        .bind(id)
        .fetch_optional(self.database())
        .await?
        .ok_or(ApiError::NotFound)
    }
}
```

`list`, `create`, `update`, `delete`, `exists`, and `count` all stay on the
trait's default implementation unless you override them too — you only pay
for the queries you actually customize.

## 5. Multi-tenant scoping

`RequestContext` carries an optional `tenant_id: Option<Uuid>`, but scoping
by tenant is deliberately not baked into the default `Repository` (it has no
opinion on your tenancy model — row-level, schema-per-tenant, etc.). For
row-level tenancy, the usual approach is a `Repository` wrapper that adds
the tenant filter to every query:

```rust
pub struct TenantScopedRepo {
    inner: DefaultRepo<Product>,
    tenant_id: Uuid,
}

#[async_trait]
impl Repository for TenantScopedRepo {
    type Entity = Product;

    fn database(&self) -> &sqlx::PgPool {
        self.inner.database()
    }

    async fn list(&self, query: &QueryParams) -> ApiResult<(Vec<Product>, i64)> {
        // Add tenant_id to query.filters before delegating, or write a
        // dedicated query here if FILTERABLE isn't expressive enough.
        self.inner.list(query).await
    }
}
```

For anything beyond simple equality scoping, it's usually more
straightforward to skip the generated `QueryBuilder` entirely for that
method and write the tenant-scoped SQL directly, as in the join example
above.

## 6. Working with list query parameters

`QueryParams` is what `handle_list` deserializes `?...` into, and what the
default `Repository::list` reads:

| Param | Effect |
|---|---|
| `?page=2` | 1-indexed page number (default `1`) |
| `?page_size=50` | Rows per page, clamped to `1..=200` (default `25`) |
| `?sort=-created_at,name` | Sort by `created_at` descending then `name` ascending — only columns in `Entity::SORTABLE` are honored, others are silently dropped |
| `?search=widget` | `ILIKE '%widget%'` across every column in `Entity::SEARCHABLE` |
| `?in_stock=true` | Equality filter — only honored if `in_stock` is in `Entity::FILTERABLE` |
| `?fields=name,sku` | Reserved for sparse-fieldset responses (application-defined) |
| `?expand=category` | Reserved for eager-loading relations (application-defined) |

Because `SORTABLE`/`SEARCHABLE`/`FILTERABLE` are compile-time allow-lists on
`Entity`, a client can never sort, search, or filter on a column you didn't
explicitly opt in — unknown filter keys are silently ignored rather than
erroring, which keeps a stray query param from breaking a request.

> **Implementation note:** the trait-default `search` handling in
> `Repository::list` builds the `ILIKE` clause per `SEARCHABLE` column but,
> as shipped, doesn't bind the `%pattern%` value per field — that binding
> is expected to come from the `#[derive(Entity)]`-generated implementation,
> which has exact per-field placeholder handling. If you're implementing
> `Repository` by hand (not via `DefaultRepo` + derive), double-check your
> `search` behavior with a query that should and shouldn't match before
> relying on it in production.

## 7. Custom error responses

`ApiError` already maps common cases (`NotFound` → 404, `Validation` → 422,
`Forbidden` → 403, `Unauthorized` → 401, `Conflict`/`StaleVersion` → 409,
`Database`/`Internal` → 500) to actix-web's `ResponseError`, returning
`{"error": "..."}`. Return `ApiError` from any hook or overridden method and
the `ViewSet` layer handles the HTTP translation automatically:

```rust
async fn before_update(&self, id: &Uuid, dto: ProductUpdate) -> ApiResult<ProductUpdate> {
    if !self.repository().exists(id).await? {
        return Err(ApiError::NotFound);
    }
    Ok(dto)
}
```

## 8. Full custom `ViewSet` handler

If a resource needs a response shape or extraction logic the default
handlers don't support (e.g. returning extra headers, or accepting
`multipart/form-data` on create), override just that handler — routing via
`configure` still wires up the rest normally:

```rust
impl ViewSet for ProductViewSetImpl {
    type Service = ProductService;

    fn service(&self) -> &Self::Service {
        &self.service
    }

    fn handle_create(
        self: std::sync::Arc<Self>,
        body: web::Json<ProductCreate>,
    ) -> impl std::future::Future<Output = actix_web::Result<HttpResponse>> {
        async move {
            let entity = self.service().create(body.into_inner()).await?;
            Ok(HttpResponse::Created()
                .insert_header(("X-Resource", "product"))
                .json(ProductResponse::from(entity)))
        }
    }
}
```

## Where to go from here

- Combine hooks (tutorial 1–2) with a soft-delete entity (tutorial 3) for a
  typical admin resource: validate on create, check permissions on delete,
  never physically lose a row.
- For resources spanning multiple tables, prefer overriding individual
  `Repository` methods (tutorial 4) over trying to force a join through
  `Entity::COLUMNS`.
- Keep tenant scoping (tutorial 5) at the `Repository` layer so it's
  impossible to forget on a new endpoint that reuses the same entity.
