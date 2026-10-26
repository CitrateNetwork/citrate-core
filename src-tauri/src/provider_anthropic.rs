//! RED STUB (WP-S1-PROV-ADAPT): the native Anthropic adapter's interface only.
#![allow(unused_imports, dead_code)]
use crate::model_provider::{
    ApiKey, ChatMessage, ChatRequest, ChatResponse, FinishReason, KeyTest, Method, ModelInfo,
    ModelProvider, ProviderCtx, ProviderError, ProviderId, Role, ToolCall, Usage, Vendor,
};
use serde_json::{json, Value};

pub const ANTHROPIC_VERSION: &str = "2023-06-01";
pub const KEY_PREFIX: &str = "sk-ant-api";
pub struct AnthropicAdapter;
pub fn to_wire(_req: &ChatRequest) -> Result<Value, ProviderError> {
    Ok(json!({}))
}
pub(crate) fn arguments_object(_args: &str) -> Result<Value, ProviderError> {
    Ok(json!({}))
}

impl ModelProvider for AnthropicAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::Vendor(Vendor::Anthropic)
    }
    fn models(&self) -> Vec<ModelInfo> {
        vec![]
    }
    fn check_key_format(&self, _key: &ApiKey) -> Result<(), ProviderError> {
        Ok(())
    }
    fn test_key(&self, _ctx: &ProviderCtx<'_>) -> Result<KeyTest, ProviderError> {
        Err(ProviderError::BadResponse("not implemented"))
    }
    fn complete(
        &self,
        _ctx: &ProviderCtx<'_>,
        _req: &ChatRequest,
        _on_text: &mut dyn FnMut(&str),
    ) -> Result<ChatResponse, ProviderError> {
        Err(ProviderError::BadResponse("not implemented"))
    }
}

#[cfg(test)]
mod tests {
    include!("provider_anthropic_tests.rs");
}
