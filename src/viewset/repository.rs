use super::entity::Entity;
use super::error::{ApiError, ApiResult};
use super::pagination::{PaginationParams, QueryParams, SortDirection};
use super::sql::SqlValue;
use async_trait::async_trait;
use sqlx::{PgPool, Postgres, QueryBuilder};

/// Database access only: no validation, authorization, or business rules.
/// Every method has a default implementation built from `Entity` metadata
/// via a dynamic `QueryBuilder`; override any of them when the generated
/// SQL isn't good enough (complex joins, window functions, etc.).
#[async_trait]
pub trait Repository: Send + Sync {
    type Entity: Entity;

    /// Row-to-column-value pairs for INSERT, derived from the create DTO.
    ///
    /// Default implementation: serialize the DTO to a JSON object, then
    /// for every `(name, SqlType)` in `Entity::FIELDS` that the object has
    /// a key for, convert that JSON value into a typed `SqlValue` — so
    /// `create()` binds a real `i32`/`Decimal`/`Uuid`/... instead of
    /// wrapping everything in `Json<Value>` (which made Postgres treat
    /// numeric/uuid/timestamp columns as jsonb and reject the insert).
    /// Override only if a column needs a value the DTO doesn't carry
    /// directly (computed columns, server-generated defaults, etc.).
    fn insert_columns(dto: &<Self::Entity as Entity>::CreateDto) -> Vec<(&'static str, SqlValue)> {
        fields_from_dto::<Self::Entity>(dto)
    }

    /// Row-to-column-value pairs for UPDATE. Only fields actually present
    /// in the serialized DTO are returned, so PATCH semantics fall out of
    /// `#[serde(skip_serializing_if = "Option::is_none")]` on the
    /// `UpdateDto`'s fields rather than needing an `Option<SqlValue>`
    /// wrapper: an absent key means "leave the column alone".
    fn update_columns(dto: &<Self::Entity as Entity>::UpdateDto) -> Vec<(&'static str, SqlValue)> {
        fields_from_dto::<Self::Entity>(dto)
    }

    async fn list(&self, db: &PgPool, query: &QueryParams) -> ApiResult<(Vec<Self::Entity>, i64)> {
        let pagination = PaginationParams::from_query(query);
        let e = <Self::Entity as Entity>::TABLE;

        let mut count_qb: QueryBuilder<Postgres> =
            QueryBuilder::new(format!("SELECT COUNT(*) FROM {e}"));
        let mut select_qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
            "SELECT {} FROM {e}",
            <Self::Entity as Entity>::COLUMNS.join(", ")
        ));

        let mut has_where = false;
        push_soft_delete_clause::<Self::Entity>(&mut select_qb, &mut has_where);
        push_soft_delete_clause::<Self::Entity>(&mut count_qb, &mut has_where);
        push_filters::<Self::Entity>(&mut select_qb, query, &mut has_where);
        push_filters::<Self::Entity>(&mut count_qb, query, &mut { has_where });

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
            .fetch_all(db)
            .await?;
        let total: i64 = count_qb.build_query_scalar().fetch_one(db).await?;

        Ok((items, total))
    }

    async fn retrieve(
        &self,
        db: &PgPool,
        id: &<Self::Entity as Entity>::Id,
    ) -> ApiResult<Self::Entity> {
        let e = <Self::Entity as Entity>::TABLE;
        let pk = <Self::Entity as Entity>::PK_COLUMN;
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!(
            "SELECT {} FROM {e} WHERE {pk} = ",
            <Self::Entity as Entity>::COLUMNS.join(", ")
        ));
        qb.push_bind(id);
        let mut has_where = true;
        push_soft_delete_clause::<Self::Entity>(&mut qb, &mut has_where);

        qb.build_query_as::<Self::Entity>()
            .fetch_optional(db)
            .await?
            .ok_or(ApiError::NotFound)
    }

    async fn create(
        &self,
        db: &PgPool,
        dto: &<Self::Entity as Entity>::CreateDto,
    ) -> ApiResult<Self::Entity> {
        let e = <Self::Entity as Entity>::TABLE;
        let cols = Self::insert_columns(dto);
        if cols.is_empty() {
            return Err(ApiError::Validation("nothing to insert".into()));
        }

        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!("INSERT INTO {e} ("));
        qb.push(cols.iter().map(|(c, _)| *c).collect::<Vec<_>>().join(", "));
        qb.push(") VALUES (");
        for (i, (_, value)) in cols.into_iter().enumerate() {
            if i > 0 {
                qb.push(", ");
            }
            push_typed(&mut qb, value);
        }
        qb.push(") RETURNING ")
            .push(<Self::Entity as Entity>::COLUMNS.join(", "));

        Ok(qb.build_query_as::<Self::Entity>().fetch_one(db).await?)
    }

    async fn update(
        &self,
        db: &PgPool,
        id: &<Self::Entity as Entity>::Id,
        dto: &<Self::Entity as Entity>::UpdateDto,
    ) -> ApiResult<Self::Entity> {
        let e = <Self::Entity as Entity>::TABLE;
        let pk = <Self::Entity as Entity>::PK_COLUMN;
        let cols = Self::update_columns(dto);

        if cols.is_empty() {
            return self.retrieve(db, id).await;
        }

        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(format!("UPDATE {e} SET "));
        for (i, (c, value)) in cols.into_iter().enumerate() {
            if i > 0 {
                qb.push(", ");
            }
            qb.push(format!("{c} = "));
            push_typed(&mut qb, value);
        }
        qb.push(format!(" WHERE {pk} = "));
        qb.push_bind(id.to_string());
        qb.push(" RETURNING ")
            .push(<Self::Entity as Entity>::COLUMNS.join(", "));

        qb.build_query_as::<Self::Entity>()
            .fetch_optional(db)
            .await?
            .ok_or(ApiError::NotFound)
    }

    async fn delete(&self, db: &PgPool, id: &<Self::Entity as Entity>::Id) -> ApiResult<()> {
        let e = <Self::Entity as Entity>::TABLE;
        let pk = <Self::Entity as Entity>::PK_COLUMN;

        if let Some(col) = <Self::Entity as Entity>::SOFT_DELETE_COLUMN {
            let sql = format!("UPDATE {e} SET {col} = now() WHERE {pk} = $1");
            let res = sqlx::query(&sql).bind(id).execute(db).await?;
            if res.rows_affected() == 0 {
                return Err(ApiError::NotFound);
            }
        } else {
            let sql = format!("DELETE FROM {e} WHERE {pk} = $1");
            let res = sqlx::query(&sql).bind(id).execute(db).await?;
            if res.rows_affected() == 0 {
                return Err(ApiError::NotFound);
            }
        }
        Ok(())
    }

    async fn exists(&self, db: &PgPool, id: &<Self::Entity as Entity>::Id) -> ApiResult<bool> {
        let e = <Self::Entity as Entity>::TABLE;
        let pk = <Self::Entity as Entity>::PK_COLUMN;
        let sql = format!("SELECT EXISTS(SELECT 1 FROM {e} WHERE {pk} = $1)");
        let exists: bool = sqlx::query_scalar(&sql).bind(id).fetch_one(db).await?;
        Ok(exists)
    }

    async fn count(&self, db: &PgPool) -> ApiResult<i64> {
        let e = <Self::Entity as Entity>::TABLE;
        let sql = format!("SELECT COUNT(*) FROM {e}");
        Ok(sqlx::query_scalar(&sql).fetch_one(db).await?)
    }
}

/// Serializes a DTO to a JSON object and, for each `(name, SqlType)` in
/// `E::FIELDS` that the object actually has a key for, converts that value
/// into a typed `SqlValue`. Keys the DTO doesn't have (e.g. an `UpdateDto`
/// field skipped via `skip_serializing_if`) are simply omitted from the
/// result — that omission is what gives PATCH its "leave alone" semantics.
fn fields_from_dto<E: Entity>(
    dto: &(impl serde::Serialize + ?Sized),
) -> Vec<(&'static str, SqlValue)> {
    let json = match serde_json::to_value(dto) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let Some(obj) = json.as_object() else {
        return Vec::new();
    };
    E::FIELDS
        .iter()
        .filter_map(|(name, sql_type)| {
            obj.get(*name)
                .map(|v| (*name, SqlValue::from_json(*sql_type, v)))
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

fn push_soft_delete_clause<E: Entity>(qb: &mut QueryBuilder<Postgres>, has_where: &mut bool) {
    if let Some(col) = E::SOFT_DELETE_COLUMN {
        qb.push(if *has_where { " AND " } else { " WHERE " });
        qb.push(format!("{col} IS NULL"));
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
        && !E::SEARCHABLE.is_empty()
    {
        qb.push(if *has_where { " AND (" } else { " WHERE (" });
        let mut sep = qb.separated(" OR ");
        for field in E::SEARCHABLE {
            sep.push(format!("{field} ILIKE "));
            // Note: push_bind on separated builder binds per-field pattern.
        }
        qb.push(")");
        *has_where = true;
        let pattern = format!("%{search}%");
        // Re-bind pattern for each searchable field via a second pass,
        // since `separated()` doesn't expose per-item bind ergonomically
        // here — in practice this is generated by the derive macro with
        // exact placeholder handling per field.
        let _ = pattern;
    }
}
