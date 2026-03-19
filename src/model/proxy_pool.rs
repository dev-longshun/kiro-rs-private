//! 代理池管理模块
//!
//! 提供代理 IP 池的 CRUD、round-robin 选择和后台健康检查

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
}

/// 代理池管理器
pub struct ProxyPoolManager {
    entries: RwLock<Vec<ProxyPoolEntry>>,
    next_id: AtomicU32,
    cursor: AtomicUsize,
    file_path: PathBuf,
    tls_backend: TlsBackend,
}

impl ProxyPoolManager {
    /// 从 JSON 文件加载代理池，文件不存在则空池
    pub fn load(path: PathBuf, tls_backend: TlsBackend) -> anyhow::Result<Self> {
        let (entries, max_id) = if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            let file: ProxyPoolFile = serde_json::from_str(&content)?;
            let max_id = file.proxies.iter().map(|e| e.id).max().unwrap_or(0);
            (file.proxies, max_id)
        } else {
            (Vec::new(), 0)
        };

        tracing::info!("代理池已加载: {} 个代理", entries.len());

        Ok(Self {
            entries: RwLock::new(entries),
            next_id: AtomicU32::new(max_id + 1),
            cursor: AtomicUsize::new(0),
            file_path: path,
            tls_backend,
        })
    }

    /// 持久化到 JSON 文件
    fn save(&self) -> anyhow::Result<()> {
        let entries = self.entries.read();
        let file = ProxyPoolFile {
            proxies: entries.clone(),
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

    /// Round-robin 选择下一个可用代理（enabled + healthy）
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
        let mut entries = self.entries.write();
        for (id, healthy, latency_ms) in results {
            if let Some(entry) = entries.iter_mut().find(|e| e.id == id) {
                entry.latency_ms = latency_ms;
                entry.last_check_at = Some(now.clone());
                if healthy {
                    if !entry.healthy {
                        tracing::info!("代理 #{} ({}) 已恢复健康", entry.id, entry.name);
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
                    }
                }
            }
        }
        drop(entries);
        let _ = self.save();
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




