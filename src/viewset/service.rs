use super::entity::Entity;
use super::error::ApiResult;
use super::pagination::{Page, QueryParams};
use super::repository::Repository;
use async_trait::async_trait;
use sqlx::{PgPool, Postgres, Transaction};

type E<S> = <<S as Service>::Repository as Repository>::Entity;

/// Business logic layer. Default methods delegate straight to the
/// repository; override a `before_*`/`after_*` hook to add validation,
/// permission checks, events, audit logging, or caching without touching
/// the CRUD wiring itself.
///
/// `create`/`update`/`delete` now run their hooks and the underlying
/// write inside a single transaction: if a hook errors after the write
/// (or the write errors after a hook), everything rolls back together
/// instead of leaving a committed row with a hook that never ran (or ran
/// against data that was then never persisted).
#[async_trait]
pub trait Service: Send + Sync {
    type Repository: Repository;

    fn repository(&self) -> &Self::Repository;

    // ---- hooks (all default no-ops) -------------------------------------

    async fn before_list(&self, _q: &QueryParams) -> ApiResult<()> {
        Ok(())
    }
    async fn after_list(&self, page: Page<E<Self>>) -> ApiResult<Page<E<Self>>> {
        Ok(page)
    }

    async fn before_create(
        &self,
        _tx: &mut Transaction<'_, Postgres>,
        dto: <E<Self> as Entity>::CreateDto,
    ) -> ApiResult<<E<Self> as Entity>::CreateDto> {
        Ok(dto)
    }
    async fn after_create(
        &self,
        _tx: &mut Transaction<'_, Postgres>,
        entity: E<Self>,
    ) -> ApiResult<E<Self>> {
        Ok(entity)
    }

    async fn before_update(
        &self,
        _tx: &mut Transaction<'_, Postgres>,
        _id: &<E<Self> as Entity>::Id,
        dto: <E<Self> as Entity>::UpdateDto,
    ) -> ApiResult<<E<Self> as Entity>::UpdateDto> {
        Ok(dto)
    }
    async fn after_update(
        &self,
        _tx: &mut Transaction<'_, Postgres>,
        entity: E<Self>,
    ) -> ApiResult<E<Self>> {
        Ok(entity)
    }

    async fn before_delete(
        &self,
        _tx: &mut Transaction<'_, Postgres>,
        _id: &<E<Self> as Entity>::Id,
    ) -> ApiResult<()> {
        Ok(())
    }
    async fn after_delete(
        &self,
        _tx: &mut Transaction<'_, Postgres>,
        _id: &<E<Self> as Entity>::Id,
    ) -> ApiResult<()> {
        Ok(())
    }

    // ---- default CRUD, built from the hooks above ------------------------

    async fn list(&self, q: QueryParams) -> ApiResult<Page<E<Self>>> {
        self.before_list(&q).await?;
        let (items, total) = self.repository().list(&q).await?;
        let pagination = super::pagination::PaginationParams::from_query(&q);
        let page = Page::new(items, &pagination, total);
        self.after_list(page).await
    }

    async fn retrieve(&self, id: <E<Self> as Entity>::Id) -> ApiResult<E<Self>> {
        self.repository().retrieve(&id).await
    }

    async fn create(&self, dto: <E<Self> as Entity>::CreateDto) -> ApiResult<E<Self>> {
        let mut tx = self.repository().transaction().await?;
        let dto = self.before_create(&mut tx, dto).await?;
        let entity = self.repository().create_in_tx(&mut tx, &dto).await?;
        let entity = self.after_create(&mut tx, entity).await?;
        tx.commit().await?;
        Ok(entity)
    }

    async fn update(
        &self,
        id: <E<Self> as Entity>::Id,
        dto: <E<Self> as Entity>::UpdateDto,
    ) -> ApiResult<E<Self>> {
        let mut tx = self.repository().transaction().await?;
        let dto = self.before_update(&mut tx, &id, dto).await?;
        let entity = self.repository().update_in_tx(&mut tx, &id, &dto).await?;
        let entity = self.after_update(&mut tx, entity).await?;
        tx.commit().await?;
        Ok(entity)
    }

    async fn delete(&self, id: <E<Self> as Entity>::Id) -> ApiResult<()> {
        let mut tx = self.repository().transaction().await?;
        self.before_delete(&mut tx, &id).await?;
        self.repository().delete_in_tx(&mut tx, &id).await?;
        self.after_delete(&mut tx, &id).await?;
        tx.commit().await?;
        Ok(())
    }
}

pub struct DefaultService<E: Repository> {
    repo: E,
}

impl<E: Repository + From<PgPool>> From<PgPool> for DefaultService<E> {
    fn from(db: PgPool) -> DefaultService<E> {
        let repo = db.into();
        Self { repo }
    }
}

impl<E: Repository> Service for DefaultService<E> {
    type Repository = E;
    fn repository(&self) -> &E {
        &self.repo
    }
}
