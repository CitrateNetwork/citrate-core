//! RED STUB (WP-S1-PROV-ADAPT): the xAI / DeepSeek / Meta adapters' interface only.
#![allow(unused_imports, dead_code)]
use crate::model_provider::{
    ApiKey, ChatMessage, ChatRequest, ChatResponse, FinishReason, KeyTest, Method, ModelInfo,
    ModelProvider, ProviderCtx, ProviderError, ProviderId, Role, ToolCall, Usage, Vendor,
};
use serde_json::{json, Value};

pub struct ChatVendorAdapter {
    v: Vendor,
}
impl ChatVendorAdapter {
    pub fn xai() -> ChatVendorAdapter {
        ChatVendorAdapter { v: Vendor::Xai }
    }
    pub fn deepseek() -> ChatVendorAdapter {
        ChatVendorAdapter { v: Vendor::DeepSeek }
    }
    pub fn meta() -> ChatVendorAdapter {
        ChatVendorAdapter { v: Vendor::Meta }
    }
    pub fn to_wire(&self, _req: &ChatRequest) -> Value {
        json!({})
    }
}
pub fn catalog(_v: Vendor) -> Vec<ModelInfo> {
    vec![]
}
impl ModelProvider for ChatVendorAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::Vendor(self.v)
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
    include!("provider_chat_vendors_tests.rs");
}
