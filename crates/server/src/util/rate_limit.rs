//! 简易内存滑动窗口 rate limiter。
//!
//! 设计：
//!   * key = "ip:scope" 或 "key_id:scope"，按 (key, window) 维护时间戳列表
//!   * 每次 check 把过期 ts 删掉，剩下的数量 ≥ limit 就拒
//!   * 用 Mutex<HashMap> —— 路由侧并发量很有限，用复杂结构没必要
//!
//! 没引第三方 crate（如 tower-governor）：那些做的更复杂，但也带来集成成本。
//! 当前需要刚好就是"防爆破登录 + 防 AI 失控刷 MCP"两个场景。

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct RateLimiter {
    inner: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// 返回 true = 通过；false = 触发限流。
    pub fn check(&self, key: &str, limit: usize, window: Duration) -> bool {
        let now = Instant::now();
        let mut g = self.inner.lock().unwrap();
        let dq = g.entry(key.to_string()).or_insert_with(VecDeque::new);
        // 移除过期 ts
        while let Some(front) = dq.front() {
            if now.duration_since(*front) > window {
                dq.pop_front();
            } else {
                break;
            }
        }
        if dq.len() >= limit {
            return false;
        }
        dq.push_back(now);
        true
    }

    /// 周期性清理：把 30 分钟没活动过的 key 整桶丢掉。
    /// 当前每个 key 占内存极少（一个 VecDeque 上限 ~120 个 Instant），所以暂未在
    /// router 里挂定时器；服务长跑时如有需要外面 spawn 一个 tokio::interval 调它即可。
    #[allow(dead_code)]
    pub fn gc(&self) {
        let now = Instant::now();
        let mut g = self.inner.lock().unwrap();
        g.retain(|_, dq| {
            dq.back()
                .map(|t| now.duration_since(*t) < Duration::from_secs(1800))
                .unwrap_or(false)
        });
    }
}

/// 从请求里抽 IP（X-Forwarded-For 仅在 trusted_proxy 配了才信）。
pub fn client_ip_key(
    headers: &axum::http::HeaderMap,
    socket_ip: Option<IpAddr>,
    trust_xff: bool,
) -> String {
    if trust_xff {
        if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
            // 取最左 IP
            if let Some(first) = xff.split(',').next() {
                let s = first.trim();
                if !s.is_empty() {
                    return s.to_string();
                }
            }
        }
    }
    socket_ip
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_under_limit() {
        let rl = RateLimiter::new();
        for _ in 0..3 {
            assert!(rl.check("a", 5, Duration::from_secs(60)));
        }
    }

    #[test]
    fn blocks_over_limit() {
        let rl = RateLimiter::new();
        for _ in 0..5 {
            assert!(rl.check("a", 5, Duration::from_secs(60)));
        }
        assert!(!rl.check("a", 5, Duration::from_secs(60)));
    }

    #[test]
    fn keys_dont_share() {
        let rl = RateLimiter::new();
        for _ in 0..5 {
            assert!(rl.check("a", 5, Duration::from_secs(60)));
        }
        // b 仍然通畅
        assert!(rl.check("b", 5, Duration::from_secs(60)));
    }
}
