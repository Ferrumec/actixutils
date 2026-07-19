use actix_web::Error; // <-- this one
use std::sync::Arc;
use super::session::{Session,SessionStore,SessionMiddleware};
    use actix_web::{test, web, App, HttpResponse, Responder};
    use std::collections::HashMap;
    use tokio::sync::RwLock;
use serde::{Deserialize,Serialize};

    #[derive(Clone, Default, Debug, PartialEq, Serialize,Deserialize)]
    struct TestSession {
        user_id: Option<i64>,
        counter: i32,
    }

    struct MockStore {
        inner: Arc<RwLock<HashMap<String, TestSession>>>,
    }

    impl MockStore {
        fn new() -> Self {
            Self { inner: Arc::new(RwLock::new(HashMap::new())) }
        }
    }

    #[async_trait::async_trait]
    impl SessionStore for MockStore {
        type Session = TestSession;

        async fn load(&self, session_id: &str) -> Result<Option<Self::Session>, Error> {
            let map = self.inner.read().await;
            Ok(map.get(session_id).cloned())
        }

        async fn save(&self, session_id: &str, session: &Self::Session) -> Result<(), Error> {
            let mut map = self.inner.write().await;
            map.insert(session_id.to_string(), session.clone());
            Ok(())
        }

        async fn delete(&self, session_id: &str) -> Result<(), Error> {
            let mut map = self.inner.write().await;
            map.remove(session_id);
            Ok(())
        }
    }

    // Handler that uses the extractor
    async fn get_session(session: Session<TestSession>) -> impl Responder {
        let s = session.0.read().await;
        HttpResponse::Ok().json(&*s)
    }

    async fn inc_counter(session: Session<TestSession>) -> impl Responder {
        let mut s = session.0.write().await;
        s.counter += 1;
        HttpResponse::Ok().json(&*s)
    }

    #[actix_web::test]
    async fn test_session_load_from_cookie() {
        let store = Arc::new(MockStore::new());
        // pre-populate store
        store.save("abc123", &TestSession { user_id: Some(42), counter: 5 }).await.unwrap();

        let app = test::init_service(
            App::new()
                .wrap(SessionMiddleware::new(store.clone()))
                .route("/me", web::get().to(get_session)),
        )
        .await;

        let req = test::TestRequest::get()
            .uri("/me")
            .cookie(actix_web::cookie::Cookie::new("session", "abc123"))
            .to_request();

        let resp: TestSession = test::call_and_read_body_json(&app, req).await;
        
        assert_eq!(resp.user_id, Some(42));
        assert_eq!(resp.counter, 5);
    }

    #[actix_web::test]
    async fn test_session_save_on_response() {
        let store = Arc::new(MockStore::new());
        store.save("xyz", &TestSession { user_id: None, counter: 0 }).await.unwrap();

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

        // Verify it was saved back to store
        let saved = store.load("xyz").await.unwrap().unwrap();
        assert_eq!(saved.counter, 1);
    }

    #[actix_web::test]
    async fn test_unauthorized_without_session_in_extensions() {
        let app = test::init_service(
            App::new()
                .route("/me", web::get().to(get_session)), // no middleware
        )
        .await;

        let req = test::TestRequest::get().uri("/me").to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401); // FromRequest returns ErrorUnauthorized
    }
