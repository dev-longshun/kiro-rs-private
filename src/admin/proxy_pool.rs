//! Admin 代理池管理处理器

use std::collections::HashMap;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};

use super::{
    middleware::AdminState,
    types::{
        AddProxyRequest, AdminErrorResponse, BoundCredentialInfo, ProxyBindingEntry,
        SetProxyEnabledRequest, SuccessResponse, UpdateProxyRequest,
    },
};

/// GET /api/admin/proxy-pool
pub async fn list_proxies(State(state): State<AdminState>) -> impl IntoResponse {
    match &state.proxy_pool {
        Some(pool) => Json(pool.list()).into_response(),
        None => {
            let error = AdminErrorResponse::internal_error("代理池未启用");
            (StatusCode::SERVICE_UNAVAILABLE, Json(error)).into_response()
        }
    }
}

/// POST /api/admin/proxy-pool
pub async fn add_proxy(
    State(state): State<AdminState>,
    Json(payload): Json<AddProxyRequest>,
) -> impl IntoResponse {
    let Some(pool) = &state.proxy_pool else {
        let error = AdminErrorResponse::internal_error("代理池未启用");
        return (StatusCode::SERVICE_UNAVAILABLE, Json(error)).into_response();
    };

    match pool.add(payload.name, payload.url, payload.username, payload.password) {
        Ok(entry) => (StatusCode::CREATED, Json(entry)).into_response(),
        Err(e) => {
            let error = AdminErrorResponse::internal_error(e.to_string());
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error)).into_response()
        }
    }
}

/// PUT /api/admin/proxy-pool/{id}
pub async fn update_proxy(
    State(state): State<AdminState>,
    Path(id): Path<u32>,
    Json(payload): Json<UpdateProxyRequest>,
) -> impl IntoResponse {
    let Some(pool) = &state.proxy_pool else {
        let error = AdminErrorResponse::internal_error("代理池未启用");
        return (StatusCode::SERVICE_UNAVAILABLE, Json(error)).into_response();
    };

    match pool.update(id, payload.name, payload.url, payload.username, payload.password) {
        Ok(Some(entry)) => Json(entry).into_response(),
        Ok(None) => {
            let error = AdminErrorResponse::not_found(format!("代理 #{} 不存在", id));
            (StatusCode::NOT_FOUND, Json(error)).into_response()
        }
        Err(e) => {
            let error = AdminErrorResponse::internal_error(e.to_string());
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error)).into_response()
        }
    }
}

/// DELETE /api/admin/proxy-pool/{id}
pub async fn delete_proxy(
    State(state): State<AdminState>,
    Path(id): Path<u32>,
) -> impl IntoResponse {
    let Some(pool) = &state.proxy_pool else {
        let error = AdminErrorResponse::internal_error("代理池未启用");
        return (StatusCode::SERVICE_UNAVAILABLE, Json(error)).into_response();
    };

    match pool.delete(id) {
        Ok(true) => Json(SuccessResponse::new(format!("代理 #{} 已删除", id))).into_response(),
        Ok(false) => {
            let error = AdminErrorResponse::not_found(format!("代理 #{} 不存在", id));
            (StatusCode::NOT_FOUND, Json(error)).into_response()
        }
        Err(e) => {
            let error = AdminErrorResponse::internal_error(e.to_string());
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error)).into_response()
        }
    }
}

/// POST /api/admin/proxy-pool/{id}/enabled
pub async fn set_proxy_enabled(
    State(state): State<AdminState>,
    Path(id): Path<u32>,
    Json(payload): Json<SetProxyEnabledRequest>,
) -> impl IntoResponse {
    let Some(pool) = &state.proxy_pool else {
        let error = AdminErrorResponse::internal_error("代理池未启用");
        return (StatusCode::SERVICE_UNAVAILABLE, Json(error)).into_response();
    };

    match pool.set_enabled(id, payload.enabled) {
        Ok(true) => {
            let action = if payload.enabled { "启用" } else { "禁用" };
            Json(SuccessResponse::new(format!("代理 #{} 已{}", id, action))).into_response()
        }
        Ok(false) => {
            let error = AdminErrorResponse::not_found(format!("代理 #{} 不存在", id));
            (StatusCode::NOT_FOUND, Json(error)).into_response()
        }
        Err(e) => {
            let error = AdminErrorResponse::internal_error(e.to_string());
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error)).into_response()
        }
    }
}

/// POST /api/admin/proxy-pool/{id}/check
pub async fn check_proxy(
    State(state): State<AdminState>,
    Path(id): Path<u32>,
) -> impl IntoResponse {
    let Some(pool) = &state.proxy_pool else {
        let error = AdminErrorResponse::internal_error("代理池未启用");
        return (StatusCode::SERVICE_UNAVAILABLE, Json(error)).into_response();
    };

    match pool.check_single(id).await {
        Ok(Some(entry)) => Json(entry).into_response(),
        Ok(None) => {
            let error = AdminErrorResponse::not_found(format!("代理 #{} 不存在", id));
            (StatusCode::NOT_FOUND, Json(error)).into_response()
        }
        Err(e) => {
            let error = AdminErrorResponse::internal_error(e.to_string());
            (StatusCode::INTERNAL_SERVER_ERROR, Json(error)).into_response()
        }
    }
}

/// GET /api/admin/proxy-pool/bindings
///
/// 返回每个代理当前绑定的凭据列表
pub async fn get_proxy_bindings(State(state): State<AdminState>) -> impl IntoResponse {
    let Some(pool) = &state.proxy_pool else {
        let error = AdminErrorResponse::internal_error("代理池未启用");
        return (StatusCode::SERVICE_UNAVAILABLE, Json(error)).into_response();
    };

    let bindings = pool.get_bindings();
    let proxies = pool.list();
    let cred_status = state.service.get_all_credentials();

    // 构建 credential_id → (email, disabled) 映射
    let cred_info: HashMap<u64, (Option<String>, bool)> = cred_status
        .credentials
        .iter()
        .map(|c| (c.id, (c.email.clone(), c.disabled)))
        .collect();

    // 按 proxy_id 分组
    let mut proxy_bindings_map: HashMap<u32, Vec<BoundCredentialInfo>> = HashMap::new();
    for (&cred_id, &proxy_id) in &bindings {
        let (email, disabled) = cred_info
            .get(&cred_id)
            .cloned()
            .unwrap_or((None, false));
        proxy_bindings_map
            .entry(proxy_id)
            .or_default()
            .push(BoundCredentialInfo {
                id: cred_id,
                email,
                disabled,
            });
    }

    // 为所有代理生成绑定条目（包括没有绑定的代理）
    let mut result: Vec<ProxyBindingEntry> = proxies
        .iter()
        .map(|p| {
            let mut credentials = proxy_bindings_map.remove(&p.id).unwrap_or_default();
            credentials.sort_by_key(|c| c.id);
            ProxyBindingEntry {
                proxy_id: p.id,
                proxy_name: p.name.clone(),
                credentials,
            }
        })
        .collect();
    result.sort_by_key(|e| e.proxy_id);

    Json(result).into_response()
}
