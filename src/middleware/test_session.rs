use super::session::{Session, SessionMiddleware, SessionStore};
use actix_web::Error; // <-- this one
use actix_web::{App, HttpResponse, Responder, test, web};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
struct TestSession {
    user_id: Option<i64>,
    counter: i32,
}

struct MockStore {
    inner: Arc<RwLock<HashMap<Uuid, TestSession>>>,
}

impl MockStore {
    fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl SessionStore for MockStore {
    type Session = TestSession;

    async fn load(&self, session_id: &Uuid) -> Result<Option<Self::Session>, Error> {
        let map = self.inner.read().await;
        Ok(map.get(session_id).cloned())
    }

    async fn save(&self, session_id: &Uuid, session: &Self::Session) -> Result<(), Error> {
        let mut map = self.inner.write().await;
        map.insert(*session_id, session.clone());
        Ok(())
    }

    async fn delete(&self, session_id: &Uuid) -> Result<(), Error> {
        let mut map = self.inner.write().await;
        map.remove(session_id);
        Ok(())
    }
}

// Handler that uses the extractor
async fn get_session(session: Session<TestSession>) -> impl Responder {
    let s = session.read().await;
    HttpResponse::Ok().json(&*s)
}

async fn inc_counter(session: Session<TestSession>) -> impl Responder {
    let mut s = session.write().await;
    s.counter += 1;
    HttpResponse::Ok().json(&*s)
}

#[actix_web::test]
async fn test_session_load_from_cookie() {
    let store = Arc::new(MockStore::new());
    // pre-populate store
    let sess_id = Uuid::new_v4();
    store
        .save(
            &sess_id,
            &TestSession {
                user_id: Some(42),
                counter: 5,
            },
        )
        .await
        .unwrap();

    let app = test::init_service(
        App::new()
            .wrap(SessionMiddleware::new(store.clone()))
            .route("/me", web::get().to(get_session)),
    )
    .await;

    let req = test::TestRequest::get()
        .uri("/me")
        .cookie(actix_web::cookie::Cookie::new(
            "session",
            sess_id.to_string(),
        ))
        .to_request();

    let resp: TestSession = test::call_and_read_body_json(&app, req).await;

    assert_eq!(resp.user_id, Some(42));
    assert_eq!(resp.counter, 5);
}

#[actix_web::test]
async fn test_session_save_on_response() {
    let store = Arc::new(MockStore::new());
    let sess_id = Uuid::new_v4();
    store
        .save(
            &sess_id,
            &TestSession {
                user_id: None,
                counter: 0,
            },
        )
        .await
        .unwrap();

    let app = test::init_service(
        App::new()
            .wrap(SessionMiddleware::new(store.clone()))
            .route("/inc", web::post().to(inc_counter)),
    )
    .await;

    let req = test::TestRequest::post()
        .uri("/inc")
        .cookie(actix_web::cookie::Cookie::new(
            "session",
            sess_id.to_string(),
        ))
        .to_request();

    let resp: TestSession = test::call_and_read_body_json(&app, req).await;
    assert_eq!(resp.counter, 1); // handler incremented

    // Verify it was saved back to store
    let saved = store.load(&sess_id).await.unwrap().unwrap();
    assert_eq!(saved.counter, 1);
}

#[actix_web::test]
async fn test_default_session() {
    let store = Arc::new(MockStore::new());

    let app = test::init_service(
        App::new()
            .wrap(SessionMiddleware::new(store.clone()))
            .route("/inc", web::post().to(inc_counter)),
    )
    .await;

    let req = test::TestRequest::post()
        .uri("/inc")
        .cookie(actix_web::cookie::Cookie::new("session", "xyz"))
        .to_request();

    let resp: TestSession = test::call_and_read_body_json(&app, req).await;
    assert_eq!(resp.counter, 1); // handler incremented
}

#[actix_web::test]
async fn test_server_error_without_session_in_extensions() {
    let app = test::init_service(
        App::new().route("/me", web::get().to(get_session)), // no middleware
    )
    .await;

    let req = test::TestRequest::get().uri("/me").to_request();
    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 500); // FromRequest returns ErrorUnauthorized
}

#[actix_web::test]
async fn test_unauthorized_without_required_session() {
    let store = Arc::new(MockStore::new());
    let app = test::init_service(
        App::new()
            .wrap(SessionMiddleware::required(store.clone()))
            .route("/me", web::get().to(get_session)),
    )
    .await;

    let req = test::TestRequest::get().uri("/me").to_request();
    let result = test::try_call_service(&app, req).await;
    let err = result.unwrap_err();
    assert_eq!(
        err.as_response_error().status_code(),
        actix_web::http::StatusCode::UNAUTHORIZED
    ); // FromRequest returns ErrorUnauthorized
}
