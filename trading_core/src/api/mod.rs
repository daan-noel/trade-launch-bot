pub mod handlers;
pub mod table_query;

use actix_web::web;

/// Register the mode-agnostic **core** routes shared by every backend bin:
/// SSE stream, settings/price, profiles/wallets/tags, token reads + creation
/// stats + batch. Deploy and local bins compose this with their own route configs
/// (e.g. `App::new().configure(configure_core_routes).configure(configure_deploy_routes)`).
///
/// Routes are registered with full `/api/...` paths (not a nested scope) so several
/// configure fns compose without scope-prefix juggling. Static token paths
/// (`/tokens/creation-stats`, `/tokens/batch`) are registered before `/tokens/{mint}`
/// so they aren't captured as a mint.
pub fn configure_core_routes(cfg: &mut web::ServiceConfig) {
    cfg
        // Real-time SSE stream
        .route("/api/stream", web::get().to(handlers::system::stream_events))
        // SOL price + persisted settings
        .route("/api/system/price", web::get().to(handlers::system::get_sol_price))
        .route("/api/system/settings", web::get().to(handlers::system::get_settings))
        .route("/api/system/settings", web::put().to(handlers::system::update_settings))
        // Creation-time bias aggregates (static paths before `/tokens/{mint}`)
        .route(
            "/api/tokens/creation-stats",
            web::get().to(handlers::tokens::get_creation_stats),
        )
        .route(
            "/api/tokens/creation-stats/grouped",
            web::get().to(handlers::tokens::get_grouped_creation_stats),
        )
        .route("/api/tokens/batch", web::post().to(handlers::tokens::post_tokens_batch))
        // Token reads
        .route("/api/tokens/{mint}", web::get().to(handlers::tokens::get_token))
        .route("/api/tokens/{mint}/trades", web::get().to(handlers::tokens::get_trades))
        // NOTE: creator reads (`/api/creators*`) take `LocalState` (analysis.rs) and
        // are registered by the local bin, not here.
        // Profiles
        .route("/api/profiles", web::get().to(handlers::system::list_profiles))
        .route("/api/profiles", web::post().to(handlers::system::create_profile))
        .route("/api/profiles/{id}", web::get().to(handlers::system::get_profile))
        .route("/api/profiles/{id}", web::put().to(handlers::system::update_profile))
        .route("/api/profiles/{id}", web::delete().to(handlers::system::delete_profile))
        .route("/api/profiles/{id}/tags", web::put().to(handlers::system::update_profile_tags))
        // Wallets
        .route("/api/profiles/{id}/wallets", web::post().to(handlers::system::create_wallet))
        .route("/api/wallets/{id}", web::get().to(handlers::system::get_wallet))
        .route("/api/wallets/{id}", web::put().to(handlers::system::update_wallet))
        .route("/api/wallets/{id}", web::delete().to(handlers::system::delete_wallet))
        // Tags
        .route("/api/tags", web::get().to(handlers::system::list_tags))
        .route("/api/tags", web::post().to(handlers::system::create_tag))
        .route("/api/tags/{id}", web::put().to(handlers::system::update_tag))
        .route("/api/tags/{id}", web::delete().to(handlers::system::delete_tag));
}
