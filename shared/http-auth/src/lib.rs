//! Shared bearer-auth middleware for the HTTP bins.
//!
//! Every box that can move real SOL (hunter `live`/`lab`, forge `live`) puts its
//! mutating API surface behind the SAME fail-closed gate. Keeping ONE copy here
//! (rather than a per-bin clone) means the auth decision — the security-critical
//! bit — lives in exactly one place and cannot silently drift between products.
//!
//! Wiring in a composition root:
//! ```ignore
//! use actix_web::{middleware::from_fn, web, App};
//! let api_auth = http_auth::ApiAuth { token: Some(secret) };
//! App::new()
//!     .wrap(from_fn(http_auth::require_bearer_auth))
//!     .app_data(web::Data::new(api_auth.clone()))
//!     // ...routes
//! # ;
//! ```

use actix_web::body::{EitherBody, MessageBody};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::middleware::Next;
use actix_web::{web, HttpResponse};

/// Bearer token gating mutating API requests, shared via `app_data`.
///
/// `token` is `Option` only so the middleware can express the "no token
/// configured" state — which it treats as **deny** (see [`require_bearer_auth`]),
/// never as "auth off". A bin that reaches HTTP setup is expected to carry
/// `Some(secret)` here (its config load should require `API_AUTH_TOKEN`).
#[derive(Clone)]
pub struct ApiAuth {
    pub token: Option<String>,
}

/// Middleware: require `Authorization: Bearer <token>` on mutating requests.
///
/// Preflight (OPTIONS) and safe reads (GET/HEAD) always pass. The check is
/// **fail-closed**: a mutating request is rejected when no token is configured,
/// so a forgotten `API_AUTH_TOKEN` blocks real-SOL routes rather than exposing
/// them. The token stays server-side (injected by the reverse proxy from its own
/// env) and is never sent to the browser.
pub async fn require_bearer_auth(
    req: ServiceRequest,
    next: Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<EitherBody<impl MessageBody>>, actix_web::Error> {
    use actix_web::http::{header::AUTHORIZATION, Method};

    let mutating = matches!(
        *req.method(),
        Method::POST | Method::PUT | Method::DELETE | Method::PATCH
    );
    let configured = req
        .app_data::<web::Data<ApiAuth>>()
        .and_then(|a| a.token.clone());

    let authorized = match (mutating, configured) {
        // Safe reads + preflight always pass.
        (false, _) => true,
        // Mutating + token configured → must match exactly.
        (true, Some(expected)) => req
            .headers()
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .map(|t| t == expected)
            .unwrap_or(false),
        // Mutating + NO token configured → deny (fail closed).
        (true, None) => false,
    };

    if authorized {
        Ok(next.call(req).await?.map_into_left_body())
    } else {
        let resp =
            HttpResponse::Unauthorized().json(serde_json::json!({ "error": "unauthorized" }));
        Ok(req.into_response(resp).map_into_right_body())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::middleware::from_fn;
    use actix_web::{test, App, HttpResponse};

    async fn ok() -> HttpResponse {
        HttpResponse::Ok().finish()
    }

    // Build a test app whose mutating + safe routes sit behind the gate. `token`
    // is what `ApiAuth` carries; `None` models a misconfigured box.
    macro_rules! app {
        ($token:expr) => {
            test::init_service(
                App::new()
                    .wrap(from_fn(require_bearer_auth))
                    .app_data(web::Data::new(ApiAuth { token: $token }))
                    .route("/", actix_web::web::get().to(ok))
                    .route("/", actix_web::web::post().to(ok)),
            )
            .await
        };
    }

    #[actix_web::test]
    async fn safe_read_passes_regardless() {
        let app = app!(Some("secret".to_string()));
        let resp = test::call_service(&app, test::TestRequest::get().uri("/").to_request()).await;
        assert!(resp.status().is_success());
    }

    #[actix_web::test]
    async fn mutating_with_correct_token_passes() {
        let app = app!(Some("secret".to_string()));
        let req = test::TestRequest::post()
            .uri("/")
            .insert_header(("Authorization", "Bearer secret"))
            .to_request();
        assert!(test::call_service(&app, req).await.status().is_success());
    }

    #[actix_web::test]
    async fn mutating_with_wrong_token_rejected() {
        let app = app!(Some("secret".to_string()));
        let req = test::TestRequest::post()
            .uri("/")
            .insert_header(("Authorization", "Bearer nope"))
            .to_request();
        assert_eq!(test::call_service(&app, req).await.status(), 401);
    }

    #[actix_web::test]
    async fn mutating_without_header_rejected() {
        let app = app!(Some("secret".to_string()));
        let req = test::TestRequest::post().uri("/").to_request();
        assert_eq!(test::call_service(&app, req).await.status(), 401);
    }

    #[actix_web::test]
    async fn fail_closed_when_no_token_configured() {
        // A box that booted without a token must DENY mutations, not wave them
        // through — even if the caller presents some bearer header.
        let app = app!(None);
        let req = test::TestRequest::post()
            .uri("/")
            .insert_header(("Authorization", "Bearer secret"))
            .to_request();
        assert_eq!(test::call_service(&app, req).await.status(), 401);
    }
}
