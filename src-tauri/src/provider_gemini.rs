//! RED STUB (WP-S1-PROV-ADAPT): the native Gemini adapter's interface only.
#![allow(unused_imports, dead_code)]
use crate::model_provider::{
    ApiKey, ChatMessage, ChatRequest, ChatResponse, FinishReason, KeyTest, Method, ModelInfo,
    ModelProvider, ProviderCtx, ProviderError, ProviderId, Role, ToolCall, Usage, Vendor,
};
use serde_json::{json, Value};

pub const REPLAY_SIGNATURE: &str = "skip_thought_signature_validator";
pub struct GeminiAdapter;
pub fn to_wire(_req: &ChatRequest) -> Result<Value, ProviderError> {
    Ok(json!({}))
}

impl ModelProvider for GeminiAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::Vendor(Vendor::Gemini)
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
    include!("provider_gemini_tests.rs");
}
