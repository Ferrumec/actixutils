use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

/// Column-type metadata for one field, used only to pick the right
/// `SqlValue` variant (and therefore the right `push_bind::<T>()` call) —
/// never serialized straight to a `jsonb` bind, which was the bug this
/// replaces: binding every value as `Json<Value>` made Postgres treat
/// numeric/uuid/timestamp columns as jsonb and reject them at insert time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlType {
    Text,
    Int4,
    Int8,
    Float4,
    Float8,
    Bool,
    Uuid,
    Date,
    Timestamp,
    Timestamptz,
    Numeric,
    /// Escape hatch for columns that are genuinely `json`/`jsonb`.
    Json,
}

/// `(column name, column type)` — what `Entity::FIELDS` is made of.
pub type Field = (&'static str, SqlType);

/// An actual value ready to be bound with its native Rust/sqlx type,
/// instead of going through `serde_json::Value` + `Json<_>`. Each variant
/// binds with `QueryBuilder::push_bind` using the type Postgres actually
/// expects for that column.
#[derive(Debug, Clone)]
pub enum SqlValue {
    Text(String),
    Int4(i32),
    Int8(i64),
    Float4(f32),
    Float8(f64),
    Bool(bool),
    Uuid(Uuid),
    Date(NaiveDate),
    Timestamp(NaiveDateTime),
    Timestamptz(DateTime<Utc>),
    Numeric(Decimal),
    Json(serde_json::Value),
    /// Untyped SQL `NULL` — safe for any column type since it isn't bound
    /// as a parameter at all, just pushed as a literal.
    Null,
}

impl SqlValue {
    /// Converts one field of a DTO's `serde_json::Value` representation
    /// into a typed `SqlValue`, per the column's declared `SqlType`.
    ///
    /// This is intentionally lenient (falls back to a type's default
    /// rather than panicking) because a mismatched DTO/column type is a
    /// developer error the `Entity`/DTO structs should catch at compile
    /// time, not something this conversion should crash a request over.
    pub fn from_json(sql_type: SqlType, value: &serde_json::Value) -> Self {
        if value.is_null() {
            return SqlValue::Null;
        }
        match sql_type {
            SqlType::Text => SqlValue::Text(
                value
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| value.to_string()),
            ),
            SqlType::Int4 => SqlValue::Int4(value.as_i64().unwrap_or_default() as i32),
            SqlType::Int8 => SqlValue::Int8(value.as_i64().unwrap_or_default()),
            SqlType::Float4 => SqlValue::Float4(value.as_f64().unwrap_or_default() as f32),
            SqlType::Float8 => SqlValue::Float8(value.as_f64().unwrap_or_default()),
            SqlType::Bool => SqlValue::Bool(value.as_bool().unwrap_or_default()),
            SqlType::Uuid => SqlValue::Uuid(
                value
                    .as_str()
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .unwrap_or_default(),
            ),
            SqlType::Date => SqlValue::Date(
                value
                    .as_str()
                    .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
                    .unwrap_or_default(),
            ),
            SqlType::Timestamp => SqlValue::Timestamp(
                value
                    .as_str()
                    .and_then(|s| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").ok())
                    .unwrap_or_default(),
            ),
            SqlType::Timestamptz => SqlValue::Timestamptz(
                value
                    .as_str()
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_default(),
            ),
            // rust_decimal's default serde impl emits a JSON string; accept
            // a JSON number too in case a caller uses the "serde-float" feature.
            SqlType::Numeric => SqlValue::Numeric(
                value
                    .as_str()
                    .and_then(|s| s.parse::<Decimal>().ok())
                    .or_else(|| value.as_f64().and_then(Decimal::from_f64_retain))
                    .unwrap_or_default(),
            ),
            SqlType::Json => SqlValue::Json(value.clone()),
        }
    }
}
