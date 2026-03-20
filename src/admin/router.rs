//! Admin API 路由配置

use axum::{
    Router, middleware,
    routing::{delete, get, post, put},
};

use super::{
    api_keys::{
        create_api_key, delete_api_key, get_all_usage, get_key_usage, get_rpm, get_server_info,
        list_api_keys, reset_key_usage, update_api_key,
    },
    handlers::{
        add_credential, delete_credential, get_all_credentials, get_credential_balance,
        get_load_balancing_mode, get_cache_simulation_ratio, get_cache_creation_ratio,
        reset_failure_count,
        set_credential_disabled, set_credential_priority, set_load_balancing_mode,
        set_cache_simulation_ratio, set_cache_creation_ratio, update_credential,
    },
    middleware::{AdminState, admin_auth_middleware},
    proxy_pool::{
        add_proxy, check_proxy, delete_proxy, list_proxies, set_proxy_enabled, update_proxy,
    },
};

/// 创建 Admin API 路由
pub fn create_admin_router(state: AdminState) -> Router {
    Router::new()
        // 凭据管理
        .route(
            "/credentials",
            get(get_all_credentials).post(add_credential),
        )
        .route("/credentials/{id}", delete(delete_credential).put(update_credential))
        .route("/credentials/{id}/disabled", post(set_credential_disabled))
        .route("/credentials/{id}/priority", post(set_credential_priority))
        .route("/credentials/{id}/reset", post(reset_failure_count))
        .route("/credentials/{id}/balance", get(get_credential_balance))
        .route(
            "/config/load-balancing",
            get(get_load_balancing_mode).put(set_load_balancing_mode),
        )
        .route(
            "/config/cache-simulation-ratio",
            get(get_cache_simulation_ratio).put(set_cache_simulation_ratio),
        )
        .route(
            "/config/cache-creation-ratio",
            get(get_cache_creation_ratio).put(set_cache_creation_ratio),
        )
        // API Key 管理
        .route("/server-info", get(get_server_info))
        .route("/api-keys", get(list_api_keys).post(create_api_key))
        .route("/api-keys/usage", get(get_all_usage))
        .route("/api-keys/{id}", put(update_api_key).delete(delete_api_key))
        .route("/api-keys/{id}/usage", get(get_key_usage).delete(reset_key_usage))
        // RPM 监控
        .route("/rpm", get(get_rpm))
        // 代理池管理
        .route("/proxy-pool", get(list_proxies).post(add_proxy))
        .route("/proxy-pool/{id}", put(update_proxy).delete(delete_proxy))
        .route("/proxy-pool/{id}/enabled", post(set_proxy_enabled))
        .route("/proxy-pool/{id}/check", post(check_proxy))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            admin_auth_middleware,
        ))
        .with_state(state)
}
