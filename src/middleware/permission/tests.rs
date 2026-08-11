//! Integration tests for the permissions middleware.
//!
//! These tests exercise the full middleware stack with Actix-Web's test server,
//! verifying end-to-end behavior including request routing, extension extraction,
//! and HTTP status code responses.

use actix_web::dev::{Service, ServiceRequest, ServiceResponse, Transform};
use actix_web::{http::Method, test, web, App, Error, HttpResponse};
use std::future::{ready, Ready};
use std::task::{Context, Poll};

use actixutils::middleware::{Permission, PermissionSet, Permissions, Principal};

// ---------------------------------------------------------------------------
// Test principal
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct User {
    role: u128,
}

impl Principal for User {
    fn role(&self) -> u128 {
        self.role
    }
}

// ---------------------------------------------------------------------------
// Test helper: middleware that inserts a principal into request extensions
// ---------------------------------------------------------------------------

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

    fn call(&self, mut req: ServiceRequest) -> Self::Future {
        req.extensions_mut().insert(self.principal.clone());
        self.service.call(req)
    }
}

// ---------------------------------------------------------------------------
// Integration tests
// ---------------------------------------------------------------------------

#[actix_web::test]
async fn matching_permission_with_active_bit_reaches_handler() {
    let permissions = PermissionSet::new(vec![
        Permission::new(Method::GET, "/users", 0).unwrap(),
    ])
    .unwrap();

    let app = test::init_service(
        App::new()
            .wrap(Permissions::<User>::new(permissions))
            .route("/users", web::get().to(|| async { HttpResponse::Ok().body("users-list") }))
            .wrap(InsertPrincipal(User { role: 0b1 })),
    )
    .await;

    let req = test::TestRequest::get().uri("/users").to_request();
    let resp = app.call(req).await.unwrap();

    assert!(resp.status().is_success());
    let body = test::read_body(resp).await;
    assert_eq!(body, "users-list");
}

#[actix_web::test]
async fn matching_permission_with_inactive_bit_returns_403() {
    let permissions = PermissionSet::new(vec![
        Permission::new(Method::GET, "/users", 0).unwrap(),
    ])
    .unwrap();

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
    let permissions = PermissionSet::new(vec![
        Permission::new(Method::GET, "/users", 0).unwrap(),
    ])
    .unwrap();

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
    let permissions = PermissionSet::new(vec![
        Permission::new(Method::GET, "/users", 0).unwrap(),
    ])
    .unwrap();

    let app = test::init_service(
        App::new()
            .wrap(Permissions::<User>::new(permissions))
            .route("/other", web::get().to(|| async { HttpResponse::Ok() }))
            .wrap(InsertPrincipal(User { role: 0b1111_1111_1111_1111 })),
    )
    .await;

    let req = test::TestRequest::get().uri("/other").to_request();
    let resp = app.call(req).await.unwrap();
    assert_eq!(resp.status(), 403);
}

#[actix_web::test]
async fn different_http_method_returns_403() {
    let permissions = PermissionSet::new(vec![
        Permission::new(Method::GET, "/users", 0).unwrap(),
    ])
    .unwrap();

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
            .route("/users/{id}", web::get().to(|| async { HttpResponse::Ok().body("user-detail") }))
            .wrap(InsertPrincipal(User { role: 0b100 })),
    )
    .await;

    let req = test::TestRequest::get().uri("/users/123").to_request();
    let resp = app.call(req).await.unwrap();
    assert!(resp.status().is_success());
    let body = test::read_body(resp).await;
    assert_eq!(body, "user-detail");
}

#[actix_web::test]
async fn principal_is_obtained_from_request_extensions() {
    let permissions = PermissionSet::new(vec![
        Permission::new(Method::GET, "/users", 0).unwrap(),
    ])
    .unwrap();

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
async fn middleware_is_auth_mechanism_agnostic() {
    // Any middleware that inserts User into extensions works, regardless of
    // how the User was constructed or authenticated.
    let permissions = PermissionSet::new(vec![
        Permission::new(Method::GET, "/users", 0).unwrap(),
    ])
    .unwrap();

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
async fn permission_set_from_json_file_behavior() {
    // Create a temporary JSON file
    let json = r#"{
        "permissions": [
            { "method": "GET", "url": "/items", "bit_id": 10 },
            { "method": "POST", "url": "/items", "bit_id": 11 }
        ]
    }"#;
    let temp_path = "/tmp/test_permissions.json";
    std::fs::write(temp_path, json).unwrap();

    let permissions = PermissionSet::from_file(temp_path).unwrap();

    let app = test::init_service(
        App::new()
            .wrap(Permissions::<User>::new(permissions))
            .route("/items", web::get().to(|| async { HttpResponse::Ok() }))
            .route("/items", web::post().to(|| async { HttpResponse::Ok() }))
            .wrap(InsertPrincipal(User { role: 1u128 << 10 })),
    )
    .await;

    // GET /items should succeed (bit 10 is set)
    let req = test::TestRequest::get().uri("/items").to_request();
    let resp = app.call(req).await.unwrap();
    assert!(resp.status().is_success());

    // POST /items should fail (bit 11 is not set)
    let req = test::TestRequest::post().uri("/items").to_request();
    let resp = app.call(req).await.unwrap();
    assert_eq!(resp.status(), 403);

    // Clean up
    let _ = std::fs::remove_file(temp_path);
}

#[actix_web::test]
async fn regex_route_matching() {
    let permissions = PermissionSet::new(vec![
        Permission::new(Method::GET, r"/users/{id:\d+}", 5).unwrap(),
    ])
    .unwrap();

    let app = test::init_service(
        App::new()
            .wrap(Permissions::<User>::new(permissions))
            .route("/users/{id}", web::get().to(|| async { HttpResponse::Ok() }))
            .wrap(InsertPrincipal(User { role: 1u128 << 5 })),
    )
    .await;

    let req = test::TestRequest::get().uri("/users/123").to_request();
    let resp = app.call(req).await.unwrap();
    assert!(resp.status().is_success());
}

#[actix_web::test]
async fn tail_pattern_route_matching() {
    let permissions = PermissionSet::new(vec![
        Permission::new(Method::GET, "/files/{tail:.*}", 7).unwrap(),
    ])
    .unwrap();

    let app = test::init_service(
        App::new()
            .wrap(Permissions::<User>::new(permissions))
            .route("/files/{tail:.*}", web::get().to(|| async { HttpResponse::Ok() }))
            .wrap(InsertPrincipal(User { role: 1u128 << 7 })),
    )
    .await;

    let req = test::TestRequest::get().uri("/files/docs/readme.txt").to_request();
    let resp = app.call(req).await.unwrap();
    assert!(resp.status().is_success());
}

#[actix_web::test]
async fn multiple_permissions_different_bits() {
    let permissions = PermissionSet::new(vec![
        Permission::new(Method::GET, "/users", 0).unwrap(),
        Permission::new(Method::POST, "/users", 1).unwrap(),
        Permission::new(Method::DELETE, "/users/{id}", 2).unwrap(),
    ])
    .unwrap();

    let app = test::init_service(
        App::new()
            .wrap(Permissions::<User>::new(permissions))
            .route("/users", web::get().to(|| async { HttpResponse::Ok() }))
            .route("/users", web::post().to(|| async { HttpResponse::Ok() }))
            .route("/users/{id}", web::delete().to(|| async { HttpResponse::Ok() }))
            .wrap(InsertPrincipal(User { role: 0b101 })), // bits 0 and 2 active
    )
    .await;

    let req = test::TestRequest::get().uri("/users").to_request();
    let resp = app.call(req).await.unwrap();
    assert!(resp.status().is_success());

    let req = test::TestRequest::post().uri("/users").to_request();
    let resp = app.call(req).await.unwrap();
    assert_eq!(resp.status(), 403); // bit 1 inactive

    let req = test::TestRequest::delete().uri("/users/42").to_request();
    let resp = app.call(req).await.unwrap();
    assert!(resp.status().is_success());
}

#[actix_web::test]
async fn unauthorized_does_not_leak_details() {
    let permissions = PermissionSet::new(vec![
        Permission::new(Method::GET, "/secret", 0).unwrap(),
    ])
    .unwrap();

    let app = test::init_service(
        App::new()
            .wrap(Permissions::<User>::new(permissions))
            .route("/secret", web::get().to(|| async { HttpResponse::Ok().body("secret") })),
    )
    .await;

    let req = test::TestRequest::get().uri("/secret").to_request();
    let resp = app.call(req).await.unwrap();
    assert_eq!(resp.status(), 401);
    let body = test::read_body(resp).await;
    assert!(body.is_empty() || body.len() == 0);
}
