#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{App, HttpResponse, Responder, test};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tokio::sync::Barrier;

    async fn test_handler(
        counter: web::Data<Arc<AtomicUsize>>,
        barrier: web::Data<Arc<Barrier>>,
    ) -> impl Responder {
        counter.fetch_add(1, Ordering::SeqCst);
        barrier.wait().await;
        HttpResponse::Ok().body("coalesced response")
    }

    #[actix_web::test]
    async fn test_single_request_normal_execution() {
        let counter = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(1));

        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(counter.clone()))
                .app_data(web::Data::new(barrier.clone()))
                .wrap(Singleflight::new(|req| req.uri().to_string()))
                .route("/", web::get().to(test_handler)),
        )
        .await;

        let req = test::TestRequest::get().uri("/").to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[actix_web::test]
    async fn test_concurrent_requests_same_key_execute_once() {
        let counter = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(3)); // Leader + 2 followers

        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(counter.clone()))
                .app_data(web::Data::new(barrier.clone()))
                .wrap(Singleflight::new(|req| req.uri().to_string()))
                .route("/", web::get().to(test_handler)),
        )
        .await;

        let req1 = test::TestRequest::get().uri("/").to_request();
        let req2 = test::TestRequest::get().uri("/").to_request();
        let req3 = test::TestRequest::get().uri("/").to_request();

        let srv = app.clone();
        let fut1 = tokio::spawn(async move { test::call_service(&srv, req1).await });
        let srv = app.clone();
        let fut2 = tokio::spawn(async move { test::call_service(&srv, req2).await });
        let srv = app.clone();
        let fut3 = tokio::spawn(async move { test::call_service(&srv, req3).await });

        let (res1, res2, res3) = tokio::join!(fut1, fut2, fut3);

        assert!(res1.unwrap().status().is_success());
        assert!(res2.unwrap().status().is_success());
        assert!(res3.unwrap().status().is_success());

        // Exactly one downstream execution should have occurred despite 3 concurrent requests
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[actix_web::test]
    async fn test_different_keys_execute_independently() {
        let counter = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(1));

        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(counter.clone()))
                .app_data(web::Data::new(barrier.clone()))
                .wrap(Singleflight::new(|req| req.uri().to_string()))
                .route("/a", web::get().to(test_handler))
                .route("/b", web::get().to(test_handler)),
        )
        .await;

        let req_a = test::TestRequest::get().uri("/a").to_request();
        let req_b = test::TestRequest::get().uri("/b").to_request();

        let resp_a = test::call_service(&app, req_a).await;
        let resp_b = test::call_service(&app, req_b).await;

        assert!(resp_a.status().is_success());
        assert!(resp_b.status().is_success());
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[actix_web::test]
    async fn test_subsequent_request_after_completion_starts_new_execution() {
        let counter = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(1));

        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(counter.clone()))
                .app_data(web::Data::new(barrier.clone()))
                .wrap(Singleflight::new(|req| req.uri().to_string()))
                .route("/", web::get().to(test_handler)),
        )
        .await;

        // First execution
        let req1 = test::TestRequest::get().uri("/").to_request();
        let _ = test::call_service(&app, req1).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // Second execution after completion (should trigger new run since flight is removed)
        let req2 = test::TestRequest::get().uri("/").to_request();
        let _ = test::call_service(&app, req2).await;
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[actix_web::test]
    async fn test_arbitrary_key_generation() {
        let counter = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(1));

        // Key incorporates header value
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(counter.clone()))
                .app_data(web::Data::new(barrier.clone()))
                .wrap(Singleflight::new(|req| {
                    let tenant = req
                        .headers()
                        .get("X-Tenant")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("default");
                    format!("{}:{}", req.uri(), tenant)
                }))
                .route("/", web::get().to(test_handler)),
        )
        .await;

        let req1 = test::TestRequest::get()
            .uri("/")
            .insert_header(("X-Tenant", "tenant-1"))
            .to_request();
        let req2 = test::TestRequest::get()
            .uri("/")
            .insert_header(("X-Tenant", "tenant-2"))
            .to_request();

        let _ = test::call_service(&app, req1).await;
        let _ = test::call_service(&app, req2).await;

        // Different tenants should result in separate executions
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }
}
