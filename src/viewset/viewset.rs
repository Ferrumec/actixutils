use super::context::RequestContext;
use super::entity::Entity;
use super::error::ApiError;
use super::pagination::QueryParams;
use super::service::Service;
use actix_web::{web, HttpResponse};
use std::str::FromStr;

type E<V> = <<<V as ViewSet>::Service as Service>::Repository as super::repository::Repository>::Entity;

/// HTTP layer: extracts requests, calls the Service, translates results
/// into responses. Nothing here touches the database directly.
///
/// `V::Service::User` must implement `FromRequest` (or be extracted by
/// whatever auth middleware the application uses) to build the
/// `RequestContext` — that wiring is application-specific and lives
/// outside this crate.
pub trait ViewSet: Send + Sync + 'static {
    type Service: Service;

    /// Base path this viewset is mounted at, e.g. "/products".
    const PATH: &'static str;

    fn service(&self) -> &Self::Service;

    fn configure(self: std::sync::Arc<Self>, cfg: &mut web::ServiceConfig)
    where
        Self: Sized,
    {
        let vs_list = self.clone();
        let vs_get = self.clone();
        let vs_post = self.clone();
        let vs_put = self.clone();
        let vs_patch = self.clone();
        let vs_delete = self.clone();

        cfg.service(
            web::resource(Self::PATH)
                .route(web::get().to(move |ctx, q| Self::handle_list(vs_list.clone(), ctx, q)))
                .route(web::post().to(move |ctx, body| Self::handle_create(vs_post.clone(), ctx, body))),
        )
        .service(
            web::resource(format!("{}/{{id}}", Self::PATH))
                .route(web::get().to(move |ctx, id| Self::handle_retrieve(vs_get.clone(), ctx, id)))
                .route(web::put().to(move |ctx, id, body| Self::handle_update(vs_put.clone(), ctx, id, body)))
                .route(web::patch().to(move |ctx, id, body| Self::handle_update(vs_patch.clone(), ctx, id, body)))
                .route(web::delete().to(move |ctx, id| Self::handle_delete(vs_delete.clone(), ctx, id))),
        );
    }

    // ---- default handlers, all overridable ------------------------------

    fn handle_list(
        self: std::sync::Arc<Self>,
        ctx: web::Data<RequestContext<<Self::Service as Service>::User>>,
        q: web::Query<QueryParams>,
    ) -> impl std::future::Future<Output = actix_web::Result<HttpResponse>>
    where
        Self: Sized,
    {
        async move {
            let page = self.service().list(&ctx, q.into_inner()).await?;
            Ok(HttpResponse::Ok().json(page))
        }
    }

    fn handle_retrieve(
        self: std::sync::Arc<Self>,
        ctx: web::Data<RequestContext<<Self::Service as Service>::User>>,
        id: web::Path<String>,
    ) -> impl std::future::Future<Output = actix_web::Result<HttpResponse>>
    where
        Self: Sized,
    {
        async move {
            let id = parse_id::<E<Self>>(&id)?;
            let entity = self.service().retrieve(&ctx, id).await?;
            Ok(HttpResponse::Ok().json(<E<Self> as Entity>::ResponseDto::from(entity)))
        }
    }

    fn handle_create(
        self: std::sync::Arc<Self>,
        ctx: web::Data<RequestContext<<Self::Service as Service>::User>>,
        body: web::Json<<E<Self> as Entity>::CreateDto>,
    ) -> impl std::future::Future<Output = actix_web::Result<HttpResponse>>
    where
        Self: Sized,
    {
        async move {
            let entity = self.service().create(&ctx, body.into_inner()).await?;
            Ok(HttpResponse::Created().json(<E<Self> as Entity>::ResponseDto::from(entity)))
        }
    }

    fn handle_update(
        self: std::sync::Arc<Self>,
        ctx: web::Data<RequestContext<<Self::Service as Service>::User>>,
        id: web::Path<String>,
        body: web::Json<<E<Self> as Entity>::UpdateDto>,
    ) -> impl std::future::Future<Output = actix_web::Result<HttpResponse>>
    where
        Self: Sized,
    {
        async move {
            let id = parse_id::<E<Self>>(&id)?;
            let entity = self.service().update(&ctx, id, body.into_inner()).await?;
            Ok(HttpResponse::Ok().json(<E<Self> as Entity>::ResponseDto::from(entity)))
        }
    }

    fn handle_delete(
        self: std::sync::Arc<Self>,
        ctx: web::Data<RequestContext<<Self::Service as Service>::User>>,
        id: web::Path<String>,
    ) -> impl std::future::Future<Output = actix_web::Result<HttpResponse>>
    where
        Self: Sized,
    {
        async move {
            let id = parse_id::<E<Self>>(&id)?;
            self.service().delete(&ctx, id).await?;
            Ok(HttpResponse::NoContent().finish())
        }
    }
}

fn parse_id<E: Entity>(raw: &str) -> Result<E::Id, ApiError>
where
    E::Id: FromStr,
{
    raw.parse::<E::Id>()
        .map_err(|_| ApiError::Validation(format!("invalid id: {raw}")))
}
