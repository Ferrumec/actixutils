use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, Deserialize)]
pub enum SortDirection {
    #[serde(rename = "asc")]
    Asc,
    #[serde(rename = "desc")]
    Desc,
}

/// Raw query-string params, parsed by the ViewSet, validated by the
/// Repository against `Entity::SORTABLE` / `SEARCHABLE` / `FILTERABLE`.
#[derive(Debug, Clone, Deserialize)]
pub struct QueryParams {
    #[serde(default)]
    pub page: Option<u32>,
    #[serde(default)]
    pub page_size: Option<u32>,
    /// e.g. "?sort=-created_at,name" -> [("created_at", Desc), ("name", Asc)]
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub search: Option<String>,
    /// Arbitrary `field=value` pairs not otherwise consumed above.
    #[serde(flatten)]
    pub filters: HashMap<String, String>,
    /// Comma separated field allow-list for sparse responses.
    #[serde(default)]
    pub fields: Option<String>,
    /// Comma separated relations to eager-load.
    #[serde(default)]
    pub expand: Option<String>,
}

pub struct PaginationParams {
    pub limit: u32,
    pub offset: u32,
    pub page: u32,
}

impl PaginationParams {
    pub const DEFAULT_PAGE_SIZE: u32 = 25;
    pub const MAX_PAGE_SIZE: u32 = 200;

    pub fn from_query(q: &QueryParams) -> Self {
        let page = q.page.unwrap_or(1).max(1);
        let limit = q
            .page_size
            .unwrap_or(Self::DEFAULT_PAGE_SIZE)
            .min(Self::MAX_PAGE_SIZE)
            .max(1);
        Self {
            limit,
            offset: (page - 1) * limit,
            page,
        }
    }

    pub fn parse_sort(raw: &str) -> Vec<(String, SortDirection)> {
        raw.split(',')
            .filter(|s| !s.is_empty())
            .map(|s| {
                if let Some(field) = s.strip_prefix('-') {
                    (field.to_string(), SortDirection::Desc)
                } else {
                    (s.to_string(), SortDirection::Asc)
                }
            })
            .collect()
    }
}

#[derive(Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub page: u32,
    pub page_size: u32,
    pub total: i64,
    pub total_pages: u32,
}

impl<T> Page<T> {
    pub fn new(items: Vec<T>, pagination: &PaginationParams, total: i64) -> Self {
        let total_pages = ((total as f64) / (pagination.limit as f64)).ceil().max(1.0) as u32;
        Self {
            items,
            page: pagination.page,
            page_size: pagination.limit,
            total,
            total_pages,
        }
    }
}
