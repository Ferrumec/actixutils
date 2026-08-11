//! The [`Permissions`] middleware implementation.
//!
//! This module provides the Actix-Web middleware that enforces authorization based on
//! a [`PermissionSet`] and a [`Principal`] extracted from request extensions.
//!
//! # Middleware Flow
//!
//! ```text
//! Incoming Request
//!       │
//!       ▼
//! Find matching permission by (method, route)
//!       │
//!       ├── No permission found ──► 403 Forbidden
//!       │
//!       ▼
//! Extract Principal from request extensions
//!       │
//!       ├── Principal missing ──► 401 Unauthorized
//!       │
//!       ▼
//! Check principal.role() bit for permission.bit_id
//!       │
//!       ├── Bit inactive ──► 403 Forbidden
//!       │
//!       └── Bit active ──► Continue to handler
//! ```
//!
//! # Registration Order
//!
//! The authentication middleware must be registered **before** (i.e., outer to)
//! the permissions middleware so that the principal is already present in
//! request extensions when authorization runs.
//!
//! ```rust,no_run
//! use actix_web::App;
//! use actixutils::middleware::{PermissionSet, Permissions};
//! # use actixutils::middleware::Principal;
//! # #[derive(Clone)] struct User { role: u128 }
//! # impl Principal for User { fn role(&self) -> u128 { self.role } }
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let permissions = PermissionSet::from_file("permissions.json")?;
//!
//! let app = App::new()
//!     // Authentication runs first (outer), inserts User into extensions.
//!     // .wrap(AuthenticationMiddleware::new(...))
//!     // Authorization runs second (inner), checks permissions.
//!     .wrap(Permissions::<User>::new(permissions));
//! # Ok(())
//! # }
//! ```

use std::future::{Ready, ready};
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;

use actix_web::HttpMessage;
use actix_web::body::{BoxBody, MessageBody};
use actix_web::dev::{Service, ServiceRequest, ServiceResponse, Transform, forward_ready};
use actix_web::{Error, HttpResponse};

use super::permission::PermissionSet;
use super::principal::Principal;

/// Authorization middleware for Actix-Web.
///
/// `Permissions` is generic over the principal type `P` that implements [`Principal`].
/// It extracts `P` from request extensions and checks whether the principal's role
/// bitset authorizes access to the requested endpoint.
///
/// The middleware uses a **default-deny** policy: if no permission is configured
/// for the incoming request, access is denied with **403 Forbidden**.
///
/// # Type Parameters
///
/// - `P`: The principal type stored in request extensions by an upstream
///   authentication middleware.
///
/// # Examples
///
/// ```rust,no_run
/// use actix_web::App;
/// use actixutils::middleware::{PermissionSet, Permissions, Principal};
///
/// #[derive(Clone)]
/// struct User { role: u128 }
///
/// impl Principal for User {
///     fn role(&self) -> u128 { self.role }
/// }
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let permissions = PermissionSet::from_file("permissions.json")?;
/// let app = App::new().wrap(Permissions::<User>::new(permissions));
/// # Ok(())
/// # }
/// ```
pub struct Permissions<P> {
    permission_set: Arc<PermissionSet>,
    _phantom: PhantomData<P>,
}

impl<P> Permissions<P> {
    /// Creates a new `Permissions` middleware from a [`PermissionSet`].
    ///
    /// The permission set is wrapped in an [`Arc`](std::sync::Arc) so it can be
    /// shared efficiently across all requests without cloning.
    ///
    /// # Examples
    ///
    /// ```
    /// use actixutils::middleware::{PermissionSet, Permissions};
    /// # use actixutils::middleware::Principal;
    /// # #[derive(Clone)] struct User { role: u128 }
    /// # impl Principal for User { fn role(&self) -> u128 { self.role } }
    ///
    /// let set = PermissionSet::new(vec![]).unwrap();
    /// let mw = Permissions::<User>::new(set);
    /// ```
    pub fn new(permission_set: PermissionSet) -> Self {
        Self {
            permission_set: Arc::new(permission_set),
            _phantom: PhantomData,
        }
    }
}

impl<P, S, B> Transform<S, ServiceRequest> for Permissions<P>
where
    P: Principal,
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type InitError = ();
    type Transform = PermissionsMiddleware<P, S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(PermissionsMiddleware {
            service,
            permission_set: Arc::clone(&self.permission_set),
            _phantom: PhantomData,
        }))
    }
}

/// The service implementation for the [`Permissions`] middleware.
pub struct PermissionsMiddleware<P, S> {
    service: S,
    permission_set: Arc<PermissionSet>,
    _phantom: PhantomData<P>,
}

impl<P, S, B> Service<ServiceRequest> for PermissionsMiddleware<P, S>
where
    P: Principal,
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: MessageBody + 'static,
{
    type Response = ServiceResponse<BoxBody>;
    type Error = Error;
    type Future = Pin<Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>>>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let method = req.method().clone();
        let path = req.path().to_string();

        let permission = self.permission_set.find(&method, &path);

        match permission {
            None => {
                // No permission configured for this endpoint.
                // Default deny.
                Box::pin(async move { Ok(req.into_response(HttpResponse::Forbidden().finish())) })
            }

            Some(perm) => {
                let role = req
                    .extensions()
                    .get::<P>()
                    .map(|principal| principal.role());

                let Some(role) = role else {
                    return Box::pin(async move {
                        Ok(req.into_response(HttpResponse::Unauthorized().finish()))
                    });
                };

                let bit_mask = 1u128 << perm.bit_id;

                if role & bit_mask == 0 {
                    Box::pin(
                        async move { Ok(req.into_response(HttpResponse::Forbidden().finish())) },
                    )
                } else {
                    let fut = self.service.call(req);

                    Box::pin(async move {
                        let res = fut.await?;
                        Ok(res.map_into_boxed_body())
                    })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Principal;
    use super::*;
    use super::{PermissionSet, Permissions};
    use crate::middleware::permission::Permission;
    use actix_web::dev::{Service, Transform};
    use actix_web::{App, HttpResponse, http::Method, test, web};
    use std::task::{Context, Poll};

    #[derive(Clone, Debug)]
    struct User {
        role: u128,
    }

    impl Principal for User {
        fn role(&self) -> u128 {
            self.role
        }
    }

    /// Test helper middleware that inserts a principal into request extensions.
    struct InsertPrincipal<P>(P);

    impl<P, S, B> Transform<S, ServiceRequest> for InsertPrincipal<P>
    where
        P: Clone + 'static,
        S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
        S::Future: 'static,
        B: 'static,
    {
        type Response = ServiceResponse<B>;
        type Error = Error;
        type InitError = ();
        type Transform = InsertPrincipalMiddleware<P, S>;
        type Future = Ready<Result<Self::Transform, Self::InitError>>;

        fn new_transform(&self, service: S) -> Self::Future {
            ready(Ok(InsertPrincipalMiddleware {
                service,
                principal: self.0.clone(),
            }))
        }
    }

    struct InsertPrincipalMiddleware<P, S> {
        service: S,
        principal: P,
    }

    impl<P, S, B> Service<ServiceRequest> for InsertPrincipalMiddleware<P, S>
    where
        P: Clone + 'static,
        S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
        S::Future: 'static,
        B: 'static,
    {
        type Response = ServiceResponse<B>;
        type Error = Error;
        type Future = S::Future;

        fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            self.service.poll_ready(cx)
        }

        fn call(&self, req: ServiceRequest) -> Self::Future {
            req.extensions_mut().insert(self.principal.clone());
            self.service.call(req)
        }
    }

    #[actix_web::test]
    async fn active_bit_reaches_handler() {
        let permissions =
            PermissionSet::new(vec![Permission::new(Method::GET, "/users", 0).unwrap()]).unwrap();

        let app = test::init_service(
            App::new()
                .wrap(Permissions::<User>::new(permissions))
                .route(
                    "/users",
                    web::get().to(|| async { HttpResponse::Ok().body("ok") }),
                )
                .wrap(InsertPrincipal(User { role: 0b1 })),
        )
        .await;

        let req = test::TestRequest::get().uri("/users").to_request();
        let resp = app.call(req).await.unwrap();
        assert!(resp.status().is_success());
    }

    #[actix_web::test]
    async fn inactive_bit_returns_403() {
        let permissions =
            PermissionSet::new(vec![Permission::new(Method::GET, "/users", 0).unwrap()]).unwrap();

        let app = test::init_service(
            App::new()
                .wrap(Permissions::<User>::new(permissions))
                .route("/users", web::get().to(|| async { HttpResponse::Ok() }))
                .wrap(InsertPrincipal(User { role: 0b0 })),
        )
        .await;

        let req = test::TestRequest::get().uri("/users").to_request();
        let resp = app.call(req).await.unwrap();
        assert_eq!(resp.status(), 403);
    }

    #[actix_web::test]
    async fn no_principal_returns_401() {
        let permissions =
            PermissionSet::new(vec![Permission::new(Method::GET, "/users", 0).unwrap()]).unwrap();

        let app = test::init_service(
            App::new()
                .wrap(Permissions::<User>::new(permissions))
                .route("/users", web::get().to(|| async { HttpResponse::Ok() })),
        )
        .await;

        let req = test::TestRequest::get().uri("/users").to_request();
        let resp = app.call(req).await.unwrap();
        assert_eq!(resp.status(), 401);
    }

    #[actix_web::test]
    async fn no_matching_permission_returns_403() {
        let permissions =
            PermissionSet::new(vec![Permission::new(Method::GET, "/users", 0).unwrap()]).unwrap();

        let app = test::init_service(
            App::new()
                .wrap(Permissions::<User>::new(permissions))
                .route("/other", web::get().to(|| async { HttpResponse::Ok() }))
                .wrap(InsertPrincipal(User {
                    role: 0b1111_1111_1111_1111,
                })),
        )
        .await;

        let req = test::TestRequest::get().uri("/other").to_request();
        let resp = app.call(req).await.unwrap();
        assert_eq!(resp.status(), 403);
    }

    #[actix_web::test]
    async fn method_mismatch_returns_403() {
        let permissions =
            PermissionSet::new(vec![Permission::new(Method::GET, "/users", 0).unwrap()]).unwrap();

        let app = test::init_service(
            App::new()
                .wrap(Permissions::<User>::new(permissions))
                .route("/users", web::post().to(|| async { HttpResponse::Ok() }))
                .wrap(InsertPrincipal(User { role: 0b1 })),
        )
        .await;

        let req = test::TestRequest::post().uri("/users").to_request();
        let resp = app.call(req).await.unwrap();
        assert_eq!(resp.status(), 403);
    }

    #[actix_web::test]
    async fn dynamic_route_matching_works() {
        let permissions = PermissionSet::new(vec![
            Permission::new(Method::GET, "/users/{id}", 2).unwrap(),
        ])
        .unwrap();

        let app = test::init_service(
            App::new()
                .wrap(Permissions::<User>::new(permissions))
                .route(
                    "/users/{id}",
                    web::get().to(|| async { HttpResponse::Ok() }),
                )
                .wrap(InsertPrincipal(User { role: 0b100 })),
        )
        .await;

        let req = test::TestRequest::get().uri("/users/123").to_request();
        let resp = app.call(req).await.unwrap();
        assert!(resp.status().is_success());
    }

    #[actix_web::test]
    async fn principal_from_extensions() {
        let permissions =
            PermissionSet::new(vec![Permission::new(Method::GET, "/users", 0).unwrap()]).unwrap();

        let app = test::init_service(
            App::new()
                .wrap(Permissions::<User>::new(permissions))
                .route("/users", web::get().to(|| async { HttpResponse::Ok() }))
                .wrap(InsertPrincipal(User { role: 0b1 })),
        )
        .await;

        let req = test::TestRequest::get().uri("/users").to_request();
        let resp = app.call(req).await.unwrap();
        assert!(resp.status().is_success());
    }

    #[actix_web::test]
    async fn middleware_does_not_care_about_auth_mechanism() {
        // Simulate a different "authentication" mechanism by using a different
        // middleware that inserts the same principal type.
        let permissions =
            PermissionSet::new(vec![Permission::new(Method::GET, "/users", 0).unwrap()]).unwrap();

        let app = test::init_service(
            App::new()
                .wrap(Permissions::<User>::new(permissions))
                .route("/users", web::get().to(|| async { HttpResponse::Ok() }))
                .wrap(InsertPrincipal(User { role: 0b1 })),
        )
        .await;

        let req = test::TestRequest::get().uri("/users").to_request();
        let resp = app.call(req).await.unwrap();
        assert!(resp.status().is_success());
    }

    #[actix_web::test]
    async fn bit_127_works_in_middleware() {
        let permissions =
            PermissionSet::new(vec![Permission::new(Method::GET, "/admin", 127).unwrap()]).unwrap();

        let app = test::init_service(
            App::new()
                .wrap(Permissions::<User>::new(permissions))
                .route("/admin", web::get().to(|| async { HttpResponse::Ok() }))
                .wrap(InsertPrincipal(User { role: 1u128 << 127 })),
        )
        .await;

        let req = test::TestRequest::get().uri("/admin").to_request();
        let resp = app.call(req).await.unwrap();
        assert!(resp.status().is_success());
    }

    #[actix_web::test]
    async fn empty_permission_set_denies_all() {
        let permissions = PermissionSet::new(vec![]).unwrap();

        let app = test::init_service(
            App::new()
                .wrap(Permissions::<User>::new(permissions))
                .route("/users", web::get().to(|| async { HttpResponse::Ok() }))
                .wrap(InsertPrincipal(User { role: 0b1 })),
        )
        .await;

        let req = test::TestRequest::get().uri("/users").to_request();
        let resp = app.call(req).await.unwrap();
        assert_eq!(resp.status(), 403);
    }
}
