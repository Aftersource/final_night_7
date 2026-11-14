//! Choosing the LLM: Claude (Anthropic Messages API) when `ANTHROPIC_API_KEY` is set and no OpenAI-compatible
//! `LLM_BASE_URL` is configured; otherwise the core's gateway (which, unconfigured, fails fast so every mind falls back).

use std::sync::Arc;
use std::time::Duration;

use theatre_llm::{GatewayConfig, LlmGateway};
use theatre_plugin_api::LlmProvider;
use theatre_server::Config;

use crate::anthropic::{AnthropicBackend, Usage, DEFAULT_MODEL};

pub struct Built {
    pub llm: Arc<dyn LlmProvider>,
    /// What it is ("claude:<model>", "gateway", "none").
    pub label: String,
    /// Claude usage counters (for spend estimates), when Claude is used.
    pub usage: Option<Arc<Usage>>,
}

/// Claude with these gateway settings (no response cache: replays come from recordings).
pub fn claude(
    model: &str,
    timeout: Duration,
    max_concurrency: usize,
    retries: u32,
) -> anyhow::Result<Option<Built>> {
    let Some(b) = AnthropicBackend::from_env(timeout).transpose()? else {
        return Ok(None);
    };
    let usage = b.usage.clone();
    let gw = LlmGateway::new(
        Some(Arc::new(b)),
        GatewayConfig {
            default_model: Some(model.to_string()),
            cache_dir: None,
            timeout: timeout * 2,
            max_concurrency,
            retries,
            ..Default::default()
        },
    );
    Ok(Some(Built {
        llm: Arc::new(gw),
        label: format!("claude:{model}"),
        usage: Some(usage),
    }))
}

/// The server's provider from its config (see the module doc).
pub fn from_config(cfg: &Config) -> anyhow::Result<Built> {
    if cfg.llm_base_url.as_deref().unwrap_or("").is_empty() {
        let model = cfg
            .llm_default_model
            .clone()
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| DEFAULT_MODEL.into());
        if let Some(b) = claude(
            &model,
            Duration::from_secs_f64(cfg.llm_timeout_secs.max(30.0)),
            cfg.llm_max_concurrency,
            cfg.llm_retries,
        )? {
            return Ok(b);
        }
    }
    let gw = theatre_server::build_llm(cfg)?;
    let label = if gw.is_configured() {
        "gateway"
    } else {
        "none"
    }
    .to_string();
    Ok(Built {
        llm: gw,
        label,
        usage: None,
    })
}
