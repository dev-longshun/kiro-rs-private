//! 代理池管理模块
//!
//! 提供代理 IP 池的 CRUD、sticky 绑定选择和后台健康检查

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::http_client::ProxyConfig;
use crate::model::config::TlsBackend;

/// 代理池条目
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyPoolEntry {
    pub id: u32,
    pub name: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    pub enabled: bool,
    pub healthy: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_check_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    pub consecutive_failures: u32,
}

/// 持久化格式
#[derive(Debug, Serialize, Deserialize)]
struct ProxyPoolFile {
    proxies: Vec<ProxyPoolEntry>,
    /// 凭据→代理 sticky 绑定映射（credential_id → proxy_id）
    #[serde(default)]
    bindings: HashMap<u64, u32>,
}

/// 代理池管理器
pub struct ProxyPoolManager {
    entries: RwLock<Vec<ProxyPoolEntry>>,
    /// 凭据→代理 sticky 绑定映射（credential_id → proxy_id）
    bindings: RwLock<HashMap<u64, u32>>,
    /// 缓存的 eligible 凭据 ID 列表（供健康检查后自动 rebalance）
    eligible_credentials: RwLock<Vec<u64>>,
    next_id: AtomicU32,
    cursor: AtomicUsize,
    file_path: PathBuf,
    tls_backend: TlsBackend,
}

impl ProxyPoolManager {
    /// 从 JSON 文件加载代理池，文件不存在则空池
    pub fn load(path: PathBuf, tls_backend: TlsBackend) -> anyhow::Result<Self> {
        let (entries, bindings, max_id) = if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            let file: ProxyPoolFile = serde_json::from_str(&content)?;
            let max_id = file.proxies.iter().map(|e| e.id).max().unwrap_or(0);
            (file.proxies, file.bindings, max_id)
        } else {
            (Vec::new(), HashMap::new(), 0)
        };

        tracing::info!("代理池已加载: {} 个代理, {} 个绑定", entries.len(), bindings.len());

        Ok(Self {
            entries: RwLock::new(entries),
            bindings: RwLock::new(bindings),
            eligible_credentials: RwLock::new(Vec::new()),
            next_id: AtomicU32::new(max_id + 1),
            cursor: AtomicUsize::new(0),
            file_path: path,
            tls_backend,
        })
    }

    /// 持久化到 JSON 文件
    fn save(&self) -> anyhow::Result<()> {
        let entries = self.entries.read();
        let bindings = self.bindings.read();
        let file = ProxyPoolFile {
            proxies: entries.clone(),
            bindings: bindings.clone(),
        };
        let content = serde_json::to_string_pretty(&file)?;
        std::fs::write(&self.file_path, content)?;
        Ok(())
    }

    /// 返回所有代理的快照
    pub fn list(&self) -> Vec<ProxyPoolEntry> {
        self.entries.read().clone()
    }

    /// 代理总数
    pub fn count(&self) -> usize {
        self.entries.read().len()
    }

    /// 新增代理（URL 重复时返回错误）
    pub fn add(&self, name: String, url: String, username: Option<String>, password: Option<String>) -> anyhow::Result<ProxyPoolEntry> {
        let entry = ProxyPoolEntry {
            id: self.next_id.fetch_add(1, Ordering::Relaxed),
            name,
            url,
            username,
            password,
            enabled: true,
            healthy: true,
            last_check_at: None,
            latency_ms: None,
            consecutive_failures: 0,
        };
        {
            let mut entries = self.entries.write();
            if entries.iter().any(|e| e.url == entry.url) {
                anyhow::bail!("代理 URL 已存在: {}", entry.url);
            }
            entries.push(entry.clone());
        }
        self.save()?;
        Ok(entry)
    }

    /// 更新代理（部分更新）
    pub fn update(
        &self,
        id: u32,
        name: Option<String>,
        url: Option<String>,
        username: Option<Option<String>>,
        password: Option<Option<String>>,
    ) -> anyhow::Result<Option<ProxyPoolEntry>> {
        let mut entries = self.entries.write();
        let Some(entry) = entries.iter_mut().find(|e| e.id == id) else {
            return Ok(None);
        };
        if let Some(n) = name { entry.name = n; }
        if let Some(u) = url { entry.url = u; }
        if let Some(u) = username { entry.username = u; }
        if let Some(p) = password { entry.password = p; }
        let result = entry.clone();
        drop(entries);
        self.save()?;
        Ok(Some(result))
    }

    /// 删除代理
    pub fn delete(&self, id: u32) -> anyhow::Result<bool> {
        let mut entries = self.entries.write();
        let len_before = entries.len();
        entries.retain(|e| e.id != id);
        let removed = entries.len() < len_before;
        drop(entries);
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    /// 启停代理
    pub fn set_enabled(&self, id: u32, enabled: bool) -> anyhow::Result<bool> {
        let mut entries = self.entries.write();
        let Some(entry) = entries.iter_mut().find(|e| e.id == id) else {
            return Ok(false);
        };
        entry.enabled = enabled;
        drop(entries);
        self.save()?;
        Ok(true)
    }

    /// Round-robin 选择下一个可用代理（enabled + healthy），作为 sticky 绑定的 fallback
    pub fn next_proxy(&self) -> Option<ProxyConfig> {
        let entries = self.entries.read();
        let available: Vec<&ProxyPoolEntry> = entries
            .iter()
            .filter(|e| e.enabled && e.healthy)
            .collect();
        if available.is_empty() {
            return None;
        }
        let idx = self.cursor.fetch_add(1, Ordering::Relaxed) % available.len();
        let entry = available[idx];
        let mut proxy = ProxyConfig::new(&entry.url);
        if let (Some(u), Some(p)) = (&entry.username, &entry.password) {
            proxy = proxy.with_auth(u, p);
        }
        Some(proxy)
    }

    /// 根据 sticky 绑定查找凭据对应的代理
    ///
    /// 如果凭据已绑定且目标代理仍可用，返回该代理；否则返回 None（调用方 fallback 到 round-robin）
    pub fn next_proxy_for(&self, credential_id: u64) -> Option<ProxyConfig> {
        let bindings = self.bindings.read();
        let proxy_id = *bindings.get(&credential_id)?;
        drop(bindings);

        let entries = self.entries.read();
        let entry = entries.iter().find(|e| e.id == proxy_id && e.enabled && e.healthy)?;
        let mut proxy = ProxyConfig::new(&entry.url);
        if let (Some(u), Some(p)) = (&entry.username, &entry.password) {
            proxy = proxy.with_auth(u, p);
        }
        Some(proxy)
    }

    /// 重平衡凭据→代理绑定（贪心最少负载算法）
    ///
    /// 1. 收集可用代理（enabled && healthy）
    /// 2. 清理失效绑定（代理不可用 or 凭据不在 eligible 列表中）
    /// 3. 对未绑定的 eligible 凭据，分配到绑定数最少的代理
    pub fn rebalance(&self, eligible_credential_ids: &[u64]) {
        let entries = self.entries.read();
        let available_ids: Vec<u32> = entries
            .iter()
            .filter(|e| e.enabled && e.healthy)
            .map(|e| e.id)
            .collect();
        drop(entries);

        let mut bindings = self.bindings.write();

        if available_ids.is_empty() {
            if !bindings.is_empty() {
                tracing::warn!("无可用代理，清空所有绑定");
                bindings.clear();
            }
            drop(bindings);
            let _ = self.save();
            return;
        }

        let eligible_set: std::collections::HashSet<u64> =
            eligible_credential_ids.iter().copied().collect();
        let available_set: std::collections::HashSet<u32> =
            available_ids.iter().copied().collect();

        // 清理失效绑定
        let before = bindings.len();
        bindings.retain(|cred_id, proxy_id| {
            eligible_set.contains(cred_id) && available_set.contains(proxy_id)
        });
        let removed = before - bindings.len();
        if removed > 0 {
            tracing::info!("清理了 {} 个失效绑定", removed);
        }

        // 统计每个代理当前绑定数
        let mut load: HashMap<u32, usize> = available_ids.iter().map(|&id| (id, 0)).collect();
        for proxy_id in bindings.values() {
            if let Some(count) = load.get_mut(proxy_id) {
                *count += 1;
            }
        }

        // 对未绑定的 eligible 凭据分配到最少负载的代理
        let mut assigned = 0usize;
        for &cred_id in eligible_credential_ids {
            if bindings.contains_key(&cred_id) {
                continue;
            }
            // 找绑定数最少的代理（相同负载时取 ID 最小的，保证确定性）
            let best_proxy_id = load
                .iter()
                .min_by_key(|&(pid, count)| (count, pid))
                .map(|(&pid, _)| pid);
            if let Some(pid) = best_proxy_id {
                bindings.insert(cred_id, pid);
                *load.get_mut(&pid).unwrap() += 1;
                assigned += 1;
            }
        }

        if assigned > 0 {
            tracing::info!("新分配了 {} 个凭据绑定", assigned);
        }

        drop(bindings);
        let _ = self.save();
    }

    /// 返回绑定映射快照（credential_id → proxy_id）
    pub fn get_bindings(&self) -> HashMap<u64, u32> {
        self.bindings.read().clone()
    }

    /// 更新缓存的 eligible 凭据列表（供健康检查后自动 rebalance）
    pub fn update_eligible_credentials(&self, ids: Vec<u64>) {
        *self.eligible_credentials.write() = ids;
    }

    /// 对单个代理执行健康检查
    pub async fn check_single(&self, id: u32) -> anyhow::Result<Option<ProxyPoolEntry>> {
        let (url, username, password) = {
            let entries = self.entries.read();
            let Some(entry) = entries.iter().find(|e| e.id == id) else {
                return Ok(None);
            };
            (entry.url.clone(), entry.username.clone(), entry.password.clone())
        };

        let mut proxy = ProxyConfig::new(&url);
        if let (Some(u), Some(p)) = (&username, &password) {
            proxy = proxy.with_auth(u, p);
        }

        let (healthy, latency_ms) = Self::probe_proxy(&proxy, self.tls_backend).await;
        let now = chrono::Utc::now().to_rfc3339();

        let mut entries = self.entries.write();
        let Some(entry) = entries.iter_mut().find(|e| e.id == id) else {
            return Ok(None);
        };
        entry.healthy = healthy;
        entry.latency_ms = latency_ms;
        entry.last_check_at = Some(now);
        if healthy {
            entry.consecutive_failures = 0;
        } else {
            entry.consecutive_failures += 1;
        }
        let result = entry.clone();
        drop(entries);
        let _ = self.save();
        Ok(Some(result))
    }

    /// 启动后台健康检查任务（每 60s 检测所有 enabled 代理）
    pub fn start_health_check(self: &std::sync::Arc<Self>) {
        let pool = std::sync::Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                pool.run_health_check().await;
            }
        });
    }

    /// 执行一轮健康检查
    async fn run_health_check(&self) {
        let targets: Vec<(u32, ProxyConfig)> = {
            let entries = self.entries.read();
            entries
                .iter()
                .filter(|e| e.enabled)
                .map(|e| {
                    let mut proxy = ProxyConfig::new(&e.url);
                    if let (Some(u), Some(p)) = (&e.username, &e.password) {
                        proxy = proxy.with_auth(u, p);
                    }
                    (e.id, proxy)
                })
                .collect()
        };

        if targets.is_empty() {
            return;
        }

        let tls = self.tls_backend;
        let results: Vec<(u32, bool, Option<u64>)> =
            futures::future::join_all(targets.into_iter().map(|(id, proxy)| async move {
                let (healthy, latency) = Self::probe_proxy(&proxy, tls).await;
                (id, healthy, latency)
            }))
            .await;

        let now = chrono::Utc::now().to_rfc3339();
        let mut availability_changed = false;
        let mut entries = self.entries.write();
        for (id, healthy, latency_ms) in results {
            if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
                entry.latency_ms = latency_ms;
                entry.last_check_at = Some(now.clone());
                if healthy {
                    if !entry.healthy {
                        tracing::info!("代理 #{} ({}) 已恢复健康", entry.id, entry.name);
                        availability_changed = true;
                    }
                    entry.healthy = true;
                    entry.consecutive_failures = 0;
                } else {
                    entry.consecutive_failures += 1;
                    if entry.consecutive_failures >= 3 && entry.healthy {
                        tracing::warn!(
                            "代理 #{} ({}) 连续 {} 次失败，标记为不健康",
                            entry.id, entry.name, entry.consecutive_failures
                        );
                        entry.healthy = false;
                        availability_changed = true;
                    }
                }
            }
        }
        drop(entries);
        let _ = self.save();

        // 代理可用性变化时，用缓存的 eligible 列表自动 rebalance
        if availability_changed {
            let eligible = self.eligible_credentials.read().clone();
            if !eligible.is_empty() {
                tracing::info!("代理可用性变化，触发绑定重平衡");
                self.rebalance(&eligible);
            }
        }
    }

    /// 通过代理连接 api.anthropic.com:443 测试连通性
    async fn probe_proxy(proxy: &ProxyConfig, tls_backend: TlsBackend) -> (bool, Option<u64>) {
        use crate::http_client::build_client;
        let start = std::time::Instant::now();
        let client = match build_client(Some(proxy), 15, tls_backend, 1) {
            Ok(c) => c,
            Err(_) => return (false, None),
        };
        // HEAD 请求到一个稳定的 HTTPS 端点，仅测试 TCP+TLS 连通性
        let result = client
            .head("https://api.anthropic.com/")
            .send()
            .await;
        let elapsed = start.elapsed().as_millis() as u64;
        match result {
            Ok(_) => (true, Some(elapsed)),
            Err(_) => (false, Some(elapsed)),
        }
    }
}




