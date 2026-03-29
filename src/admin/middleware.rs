//! Admin API 中间件

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
};
use tokio::sync::Semaphore;

use super::service::AdminService;
use super::types::AdminErrorResponse;
use crate::common::auth;
use crate::model::api_key::ApiKeyManager;
use crate::model::proxy_pool::ProxyPoolManager;
use crate::model::rpm::RpmTracker;
use crate::model::usage::UsageTracker;

/// Admin API 共享状态
#[derive(Clone)]
pub struct AdminState {
    pub admin_api_key: String,
    pub master_api_key: Option<String>,
    pub service: Arc<AdminService>,
    pub api_key_manager: Option<Arc<ApiKeyManager>>,
    pub usage_tracker: Option<Arc<UsageTracker>>,
    pub rpm_tracker: Option<Arc<RpmTracker>>,
    pub proxy_pool: Option<Arc<ProxyPoolManager>>,
    /// 并发信号量池（与 KiroProvider 共享，用于监控）
    pub credential_limits: Option<Arc<parking_lot::Mutex<HashMap<u64, Arc<Semaphore>>>>>,
    pub max_concurrent_per_credential: Option<Arc<parking_lot::Mutex<usize>>>,
    /// 用户并发限制引用（与 KiroProvider 共享）
    pub max_concurrent_per_api_key: Option<Arc<parking_lot::Mutex<usize>>>,
}

impl AdminState {
    pub fn new(admin_api_key: impl Into<String>, service: AdminService) -> Self {
        Self {
            admin_api_key: admin_api_key.into(),
            master_api_key: None,
            service: Arc::new(service),
            api_key_manager: None,
            usage_tracker: None,
            rpm_tracker: None,
            proxy_pool: None,
            credential_limits: None,
            max_concurrent_per_credential: None,
            max_concurrent_per_api_key: None,
        }
    }

    pub fn with_master_api_key(mut self, key: impl Into<String>) -> Self { self.master_api_key = Some(key.into()); self }
    pub fn with_api_key_manager(mut self, m: Arc<ApiKeyManager>) -> Self { self.api_key_manager = Some(m); self }
    pub fn with_usage_tracker(mut self, t: Arc<UsageTracker>) -> Self { self.usage_tracker = Some(t); self }
    pub fn with_rpm_tracker(mut self, t: Arc<RpmTracker>) -> Self { self.rpm_tracker = Some(t); self }
    pub fn with_proxy_pool(mut self, p: Arc<ProxyPoolManager>) -> Self { self.proxy_pool = Some(p); self }

    pub fn with_concurrency_refs(
        mut self,
        limits: Arc<parking_lot::Mutex<HashMap<u64, Arc<Semaphore>>>>,
        max_per: Arc<parking_lot::Mutex<usize>>,
    ) -> Self {
        self.credential_limits = Some(limits);
        self.max_concurrent_per_credential = Some(max_per);
        self
    }

    /// 获取每个凭据的当前并发数
    pub fn credential_concurrency_snapshot(&self) -> HashMap<u64, usize> {
        let (Some(limits_ref), Some(max_ref)) = (&self.credential_limits, &self.max_concurrent_per_credential) else {
            return HashMap::new();
        };
        let limit = *max_ref.lock();
        if limit == 0 {
            return HashMap::new();
        }
        let pool = limits_ref.lock();
        pool.iter()
            .map(|(&id, sem)| (id, limit - sem.available_permits()))
            .collect()
    }
}

/// Admin API 认证中间件
pub async fn admin_auth_middleware(
    State(state): State<AdminState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let api_key = auth::extract_api_key(&request);
    match api_key {
        Some(key) if auth::constant_time_eq(&key, &state.admin_api_key) => next.run(request).await,
        _ => {
            let error = AdminErrorResponse::authentication_error();
            (StatusCode::UNAUTHORIZED, Json(error)).into_response()
        }
    }
}
