use super::entity::Entity;
use super::error::ApiResult;
use super::pagination::{Page, QueryParams};
use super::repository::Repository;
use async_trait::async_trait;

type E<S> = <<S as Service>::Repository as Repository>::Entity;

/// Business logic layer. Default methods delegate straight to the
/// repository; override a `before_*`/`after_*` hook to add validation,
/// permission checks, transactions, events, audit logging, or caching
/// without touching the CRUD wiring itself.
///
/// `U` matches the user type carried by `RequestContext<U>`.
#[async_trait]
pub trait Service: Send + Sync {
    type Repository: Repository;
    /// User type carried by `RequestContext`. Associated-type defaults are
    /// unstable, so implementors set this explicitly — `type User = ();`
    /// is the common no-auth-context choice, see `examples/product.rs`.
    type User: Send + Sync;

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

        dto: <E<Self> as Entity>::CreateDto,
    ) -> ApiResult<<E<Self> as Entity>::CreateDto> {
        Ok(dto)
    }
    async fn after_create(&self, entity: E<Self>) -> ApiResult<E<Self>> {
        Ok(entity)
    }

    async fn before_update(
        &self,

        _id: &<E<Self> as Entity>::Id,
        dto: <E<Self> as Entity>::UpdateDto,
    ) -> ApiResult<<E<Self> as Entity>::UpdateDto> {
        Ok(dto)
    }
    async fn after_update(&self, entity: E<Self>) -> ApiResult<E<Self>> {
        Ok(entity)
    }

    async fn before_delete(&self, _id: &<E<Self> as Entity>::Id) -> ApiResult<()> {
        Ok(())
    }
    async fn after_delete(&self, _id: &<E<Self> as Entity>::Id) -> ApiResult<()> {
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
        let dto = self.before_create(dto).await?;
        let entity = self.repository().create(&dto).await?;
        self.after_create(entity).await
    }

    async fn update(
        &self,

        id: <E<Self> as Entity>::Id,
        dto: <E<Self> as Entity>::UpdateDto,
    ) -> ApiResult<E<Self>> {
        let dto = self.before_update(&id, dto).await?;
        let entity = self.repository().update(&id, &dto).await?;
        self.after_update(entity).await
    }

    async fn delete(&self, id: <E<Self> as Entity>::Id) -> ApiResult<()> {
        self.before_delete(&id).await?;
        self.repository().delete(&id).await?;
        self.after_delete(&id).await
    }
}
