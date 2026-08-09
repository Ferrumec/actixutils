use super::cache::{Cache, DefaultCache};
use super::entity::Entity;
use super::error::{ApiError, ApiResult};
use super::pagination::{PaginationParams, QueryParams, SortDirection};
use super::sql::{SqlType, SqlValue};
use async_trait::async_trait;
use sqlx::{PgPool, Postgres, QueryBuilder, Transaction};
use std::sync::Arc;
/// Database access only: no validation, authorization, or business rules.
/// Every method has a default implementation built from `Entity` metadata
/// via a dynamic `QueryBuilder`; override any of them when the generated
/// SQL isn't good enough (complex joins, window functions, etc.).
#[async_trait]
pub trait Repository: Send + Sync {
    type Entity: Entity;
    fn database(&self) -> &PgPool;

    fn cache(&self) -> Arc<dyn Cache<<<Self as Repository>::Entity as Entity>::Id, Self::Entity>>;

    /// Begin a transaction against this repository's pool. Used by the
    /// default `Service::create`/`update`/`delete` implementations so a
    /// mutation and its `before_*`/`after_*` hooks run atomically — if a
    /// hook (or the write itself) fails, everything rolls back together.
    async fn transaction(&self) -> ApiResult<Transaction<'_, Postgres>> {
        Ok(self.database().begin().await?)
    }

    /// Row-to-column-value pairs for INSERT, derived from the create DTO.
    ///
    /// Default implementation: serialize the DTO to a JSON object, then
    /// for every `(name, SqlType)` in `Entity::FIELDS` that the object has
    /// a key for, convert that JSON value into a typed `SqlValue` — so
    /// `create()` binds a real `i32`/`Decimal`/`Uuid`/... instead of
    /// wrapping everything in `Json<Value>` (which made Postgres treat
    /// numeric/uuid/timestamp columns as jsonb and reject the insert).
    /// Returns an error (rather than silently defaulting) if a value
    /// doesn't fit its column's declared type. Override only if a column
    /// needs a value the DTO doesn't carry directly (computed columns,
    /// server-generated defaults, etc.).
    fn insert_columns(
        dto: &<Self::Entity as Entity>::CreateDto,
    ) -> ApiResult<Vec<(&'static str, SqlValue)>> {
        fields_from_dto::<Self::Entity>(dto)
    }

    /// Row-to-column-value pairs for UPDATE. Only fields actually present
    /// in the serialized DTO are returned, so PATCH semantics fall out of
    /// `#[serde(skip_serializing_if = "Option::is_none")]` on the
    /// `UpdateDto`'s fields rather than needing an `Option<SqlValue>`
    /// wrapper: an absent key means "leave the column alone".
    fn update_columns(
        dto: &<Self::Entity as Entity>::UpdateDto,
    ) -> ApiResult<Vec<(&'static str, SqlValue)>> {
        fields_from_dto::<Self::Entity>(dto)
    }

    async fn list(&self, query: &QueryParams) -> ApiResult<(Vec<Self::Entity>, i64)> {
        let pagination = PaginationParams::from_query(query);
        let e = <Self::Entity as Entity>::TABLE;

        let mut count_qb: QueryBuilder<Postgres> =
            QueryBuilder::new(format!("SELECT COUNT(*) FROM {e}"));
        let mut select_qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
            "SELECT {} FROM {e}",
            <Self::Entity as Entity>::COLUMNS.join(", ")
        ));

        // `select_qb` and `count_qb` are two independent statements and
        // must track their own WHERE state separately — sharing one flag
        // between them caused a bare `AND` with no preceding `WHERE` on
        // `count_qb` whenever a soft-delete column or any filter/search
        // param was involved, which Postgres rejects as a syntax error.
        let mut select_has_where = false;
        let mut count_has_where = false;
        push_soft_delete_clause::<Self::Entity>(&mut select_qb, &mut select_has_where);
        push_soft_delete_clause::<Self::Entity>(&mut count_qb, &mut count_has_where);
        push_filters::<Self::Entity>(&mut select_qb, query, &mut select_has_where);
        push_filters::<Self::Entity>(&mut count_qb, query, &mut count_has_where);

        if let Some(sort) = &query.sort {
            let clauses: Vec<String> = PaginationParams::parse_sort(sort)
                .into_iter()
                .filter(|(field, _)| <Self::Entity as Entity>::SORTABLE.contains(&field.as_str()))
                .map(|(field, dir)| {
                    format!(
                        "{field} {}",
                        match dir {
                            SortDirection::Asc => "ASC",
                            SortDirection::Desc => "DESC",
                        }
                    )
                })
                .collect();
            if !clauses.is_empty() {
                select_qb.push(" ORDER BY ").push(clauses.join(", "));
            }
        }

        select_qb
            .push(" LIMIT ")
            .push_bind(pagination.limit as i64)
            .push(" OFFSET ")
            .push_bind(pagination.offset as i64);

        let items = select_qb
            .build_query_as::<Self::Entity>()
            .fetch_all(self.database())
            .await?;
        let total: i64 = count_qb
            .build_query_scalar()
            .fetch_one(self.database())
            .await?;

        Ok((items, total))
    }

    /// Cache-aside read: a hit returns straight from `cache()` without
    /// touching the database; a miss falls through to the row fetch and
    /// populates the cache before returning. Safe to populate
    /// unconditionally here — unlike the `_in_tx` write paths below,
    /// there's no open transaction that could still roll back and turn
    /// this into a phantom entry.
    async fn retrieve(&self, id: &<Self::Entity as Entity>::Id) -> ApiResult<Self::Entity> {
        if let Some(hit) = self.cache().get(id) {
            return Ok(hit);
        }
        let entity = retrieve_row::<_, Self::Entity>(self.database(), id).await?;
        self.cache().set(entity.id(), entity.clone());
        Ok(entity)
    }

    /// Runs as its own auto-committed statement (no explicit `BEGIN`), so
    /// by the time this returns the row is durably written and it's safe
    /// to populate the cache with it directly (write-through).
    async fn create(&self, dto: &<Self::Entity as Entity>::CreateDto) -> ApiResult<Self::Entity> {
        let cols = Self::insert_columns(dto)?;
        let entity = insert_row::<_, Self::Entity>(self.database(), cols).await?;
        self.cache().set(entity.id(), entity.clone());
        Ok(entity)
    }

    /// Same as `create`, but runs against an already-open transaction so
    /// the insert can be committed or rolled back together with whatever
    /// a `Service`'s `before_create`/`after_create` hooks do in that same
    /// transaction.
    ///
    /// Deliberately does **not** touch the cache. The row isn't committed
    /// yet at this point — if a later hook in the same transaction fails
    /// and the caller rolls back, a cache entry set here would describe an
    /// id that never actually existed. Once the transaction commits, a
    /// subsequent `retrieve()` will populate the cache normally via the
    /// cache-aside path above.
    async fn create_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        dto: &<Self::Entity as Entity>::CreateDto,
    ) -> ApiResult<Self::Entity> {
        let cols = Self::insert_columns(dto)?;
        insert_row::<_, Self::Entity>(&mut **tx, cols).await
    }

    /// Auto-committed like `create`, so the write-through cache update
    /// below is ordered safely after the durable write.
    async fn update(
        &self,
        id: &<Self::Entity as Entity>::Id,
        dto: &<Self::Entity as Entity>::UpdateDto,
    ) -> ApiResult<Self::Entity> {
        let cols = Self::update_columns(dto)?;
        let entity = update_row::<_, Self::Entity>(self.database(), id, cols).await?;
        self.cache().set(entity.id(), entity.clone());
        Ok(entity)
    }

    /// Transactional counterpart to `update`, see `create_in_tx`.
    ///
    /// Invalidates (rather than repopulates) the cache entry: unlike
    /// `create_in_tx`, there's a previously-cached value to worry about
    /// here, and holding onto a stale one is worse than dropping it.
    /// Overwriting it with the *new* row would be worse still — same
    /// rollback risk as `create_in_tx` — so this drops the entry and lets
    /// the next `retrieve()` repopulate it post-commit.
    ///
    /// Known race: a concurrent `retrieve()` between this invalidation and
    /// the caller's `tx.commit()` will still observe the pre-update row
    /// (Postgres read-committed semantics) and re-cache *that*, leaving a
    /// stale entry after commit. Closing that window fully needs a
    /// post-commit invalidation from the `Service` layer, which owns the
    /// commit point — worth revisiting if strict read-after-write
    /// consistency through the cache becomes a requirement.
    async fn update_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        id: &<Self::Entity as Entity>::Id,
        dto: &<Self::Entity as Entity>::UpdateDto,
    ) -> ApiResult<Self::Entity> {
        let cols = Self::update_columns(dto)?;
        let entity = update_row::<_, Self::Entity>(&mut **tx, id, cols).await?;
        self.cache().delete(id);
        Ok(entity)
    }

    /// Auto-committed like `create`/`update`, so invalidating the cache
    /// entry here is correctly ordered after the durable delete.
    async fn delete(&self, id: &<Self::Entity as Entity>::Id) -> ApiResult<()> {
        delete_row::<_, Self::Entity>(self.database(), id).await?;
        self.cache().delete(id);
        Ok(())
    }

    /// Transactional counterpart to `delete`, see `create_in_tx` and
    /// `update_in_tx` for why this invalidates rather than caching
    /// anything, and for the same pre-commit race `update_in_tx` has.
    async fn delete_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        id: &<Self::Entity as Entity>::Id,
    ) -> ApiResult<()> {
        delete_row::<_, Self::Entity>(&mut **tx, id).await?;
        self.cache().delete(id);
        Ok(())
    }

    async fn exists(&self, id: &<Self::Entity as Entity>::Id) -> ApiResult<bool> {
        let e = <Self::Entity as Entity>::TABLE;
        let pk = <Self::Entity as Entity>::PK_COLUMN;
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new(format!("SELECT EXISTS(SELECT 1 FROM {e} WHERE {pk} = "));
        qb.push_bind(id);
        qb.push(")");

        let exists: bool = qb.build_query_scalar().fetch_one(self.database()).await?;
        Ok(exists)
    }

    async fn count(&self) -> ApiResult<i64> {
        let e = <Self::Entity as Entity>::TABLE;
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!("SELECT COUNT(*) FROM {e}"));
        Ok(qb.build_query_scalar().fetch_one(self.database()).await?)
    }
}

/// Serializes a DTO to a JSON object and, for each `(name, SqlType)` in
/// `E::FIELDS` that the object actually has a key for, converts that value
/// into a typed `SqlValue`. Keys the DTO doesn't have (e.g. an `UpdateDto`
/// field skipped via `skip_serializing_if`) are simply omitted from the
/// result — that omission is what gives PATCH its "leave alone" semantics.
/// A value that doesn't match its column's declared type is rejected with
/// `ApiError::Validation` rather than silently coerced to a default.
fn fields_from_dto<E: Entity>(
    dto: &(impl serde::Serialize + ?Sized),
) -> ApiResult<Vec<(&'static str, SqlValue)>> {
    let json = serde_json::to_value(dto)
        .map_err(|e| ApiError::Internal(format!("failed to serialize DTO: {e}")))?;
    let Some(obj) = json.as_object() else {
        return Ok(Vec::new());
    };
    E::FIELDS
        .iter()
        .filter_map(|(name, sql_type)| {
            obj.get(*name)
                .map(|v| SqlValue::from_json(*sql_type, name, v).map(|sv| (*name, sv)))
        })
        .collect()
}

/// Binds one `SqlValue` into the query builder with its native type, so
/// Postgres sees an `i32`/`Decimal`/`Uuid`/... parameter instead of jsonb.
fn push_typed(qb: &mut QueryBuilder<Postgres>, value: SqlValue) {
    match value {
        SqlValue::Text(v) => {
            qb.push_bind(v);
        }
        SqlValue::Int4(v) => {
            qb.push_bind(v);
        }
        SqlValue::Int8(v) => {
            qb.push_bind(v);
        }
        SqlValue::Float4(v) => {
            qb.push_bind(v);
        }
        SqlValue::Float8(v) => {
            qb.push_bind(v);
        }
        SqlValue::Bool(v) => {
            qb.push_bind(v);
        }
        SqlValue::Uuid(v) => {
            qb.push_bind(v);
        }
        SqlValue::Date(v) => {
            qb.push_bind(v);
        }
        SqlValue::Timestamp(v) => {
            qb.push_bind(v);
        }
        SqlValue::Timestamptz(v) => {
            qb.push_bind(v);
        }
        SqlValue::Numeric(v) => {
            qb.push_bind(v);
        }
        SqlValue::Json(v) => {
            qb.push_bind(sqlx::types::Json(v));
        }
        // Untyped NULL literal — safe for any column, and correctly
        // distinct from "column omitted" (which never reaches this
        // function at all, see `fields_from_dto`).
        SqlValue::Null => {
            qb.push("NULL");
        }
    }
}

/// Whether `E`'s soft-delete column (if any) is declared as `Bool` in
/// `Entity::FIELDS`. Anything else (nullable timestamp being the common
/// case) is treated as timestamp-flavored soft delete.
fn soft_delete_is_bool<E: Entity>(col: &str) -> bool {
    E::FIELDS
        .iter()
        .any(|(name, ty)| *name == col && *ty == SqlType::Bool)
}

fn push_soft_delete_clause<E: Entity>(qb: &mut QueryBuilder<Postgres>, has_where: &mut bool) {
    if let Some(col) = E::SOFT_DELETE_COLUMN {
        qb.push(if *has_where { " AND " } else { " WHERE " });
        if soft_delete_is_bool::<E>(col) {
            // Boolean flag: NULL and false both mean "not deleted".
            qb.push(format!("{col} IS NOT TRUE"));
        } else {
            qb.push(format!("{col} IS NULL"));
        }
        *has_where = true;
    }
}

fn push_filters<E: Entity>(
    qb: &mut QueryBuilder<Postgres>,
    query: &QueryParams,
    has_where: &mut bool,
) {
    for (field, value) in &query.filters {
        if !E::FILTERABLE.contains(&field.as_str()) {
            continue; // silently ignore unknown/forbidden filter keys
        }
        qb.push(if *has_where { " AND " } else { " WHERE " });
        qb.push(format!("{field} = "));
        qb.push_bind(value.clone());
        *has_where = true;
    }
    if let Some(search) = &query.search
        && !search.is_empty()
        && !E::SEARCHABLE.is_empty()
    {
        qb.push(if *has_where { " AND (" } else { " WHERE (" });
        let pattern = format!("%{search}%");
        for (i, field) in E::SEARCHABLE.iter().enumerate() {
            if i > 0 {
                qb.push(" OR ");
            }
            qb.push(format!("{field} ILIKE "));
            qb.push_bind(pattern.clone());
        }
        qb.push(")");
        *has_where = true;
    }
}

/// Generic single-row fetch by primary key, usable against either a pool
/// or an open transaction.
async fn retrieve_row<'e, E, Ent>(exec: E, id: &Ent::Id) -> ApiResult<Ent>
where
    E: sqlx::postgres::PgExecutor<'e>,
    Ent: Entity,
{
    let table = Ent::TABLE;
    let pk = Ent::PK_COLUMN;
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
        "SELECT {} FROM {table} WHERE {pk} = ",
        Ent::COLUMNS.join(", ")
    ));
    qb.push_bind(id);
    let mut has_where = true;
    push_soft_delete_clause::<Ent>(&mut qb, &mut has_where);

    qb.build_query_as::<Ent>()
        .fetch_optional(exec)
        .await?
        .ok_or(ApiError::NotFound)
}

/// Generic INSERT, usable against either a pool or an open transaction.
async fn insert_row<'e, E, Ent>(exec: E, cols: Vec<(&'static str, SqlValue)>) -> ApiResult<Ent>
where
    E: sqlx::postgres::PgExecutor<'e>,
    Ent: Entity,
{
    let table = Ent::TABLE;
    if cols.is_empty() {
        return Err(ApiError::Validation("nothing to insert".into()));
    }

    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!("INSERT INTO {table} ("));
    qb.push(cols.iter().map(|(c, _)| *c).collect::<Vec<_>>().join(", "));
    qb.push(") VALUES (");
    for (i, (_, value)) in cols.into_iter().enumerate() {
        if i > 0 {
            qb.push(", ");
        }
        push_typed(&mut qb, value);
    }
    qb.push(") RETURNING ").push(Ent::COLUMNS.join(", "));

    Ok(qb.build_query_as::<Ent>().fetch_one(exec).await?)
}

/// Generic UPDATE by primary key, usable against either a pool or an open
/// transaction. The id is bound with its native sqlx type (not
/// `.to_string()`'d into a text parameter) so it matches the column's
/// actual type — binding a `Uuid`/int PK as text made Postgres reject the
/// comparison with an "operator does not exist" error.
async fn update_row<'e, E, Ent>(
    exec: E,
    id: &Ent::Id,
    cols: Vec<(&'static str, SqlValue)>,
) -> ApiResult<Ent>
where
    E: sqlx::postgres::PgExecutor<'e>,
    Ent: Entity,
{
    if cols.is_empty() {
        return retrieve_row::<_, Ent>(exec, id).await;
    }

    let table = Ent::TABLE;
    let pk = Ent::PK_COLUMN;
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!("UPDATE {table} SET "));
    for (i, (c, value)) in cols.into_iter().enumerate() {
        if i > 0 {
            qb.push(", ");
        }
        qb.push(format!("{c} = "));
        push_typed(&mut qb, value);
    }
    qb.push(format!(" WHERE {pk} = "));
    qb.push_bind(id);
    qb.push(" RETURNING ").push(Ent::COLUMNS.join(", "));

    qb.build_query_as::<Ent>()
        .fetch_optional(exec)
        .await?
        .ok_or(ApiError::NotFound)
}

/// Generic DELETE (or soft delete) by primary key, usable against either a
/// pool or an open transaction. Soft delete now respects the column's
/// actual type: `SET col = true` for a boolean flag, `SET col = now()`
/// for the more common nullable-timestamp column — previously this
/// always wrote `now()`, which fails against a boolean column.
async fn delete_row<'e, E, Ent>(exec: E, id: &Ent::Id) -> ApiResult<()>
where
    E: sqlx::postgres::PgExecutor<'e>,
    Ent: Entity,
{
    let table = Ent::TABLE;
    let pk = Ent::PK_COLUMN;

    let mut qb: QueryBuilder<Postgres> = if let Some(col) = Ent::SOFT_DELETE_COLUMN {
        if soft_delete_is_bool::<Ent>(col) {
            QueryBuilder::new(format!("UPDATE {table} SET {col} = true WHERE {pk} = "))
        } else {
            QueryBuilder::new(format!("UPDATE {table} SET {col} = now() WHERE {pk} = "))
        }
    } else {
        QueryBuilder::new(format!("DELETE FROM {table} WHERE {pk} = "))
    };
    qb.push_bind(id);

    let res = qb.build().execute(exec).await?;
    if res.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(())
}

use std::marker::PhantomData;

pub struct DefaultRepo<E: Entity> {
    db: PgPool,
    cache: Arc<DefaultCache<E::Id, E>>,
    _marker: PhantomData<E>,
}

impl<E: Entity> From<PgPool> for DefaultRepo<E> {
    fn from(db: PgPool) -> DefaultRepo<E> {
        Self {
            db,
            _marker: PhantomData,
            cache: Arc::new(DefaultCache::new(1000)),
        }
    }
}

impl<E: Entity> Repository for DefaultRepo<E> {
    type Entity = E;
    fn cache(&self) -> Arc<dyn super::cache::Cache<<E as Entity>::Id, E> + 'static> {
        self.cache.clone()
    }
    fn database(&self) -> &PgPool {
        &self.db
    }
}
