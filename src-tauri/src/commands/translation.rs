use crate::history::HistoryEntry;
use crate::translate::claude::ClaudeProvider;
use crate::translate::gemini::GeminiProvider;
use crate::translate::ollama::OllamaProvider;
use crate::translate::openai_compat::OpenAICompatProvider;
use crate::translate::provider::TranslateResult;
use crate::translate::provider::{TranslateError, TranslateProvider};
use crate::AppState;
use crate::{config, config::ActionPromptConfig};
use reqwest::Client;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::State;

const AI_ACTION_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const AI_ACTION_READ_TIMEOUT: Duration = Duration::from_secs(20);
const AI_ACTION_TOTAL_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AiActionStreamEvent {
    Delta { text: String },
    Done,
    Error { message: String },
}

#[derive(Debug, PartialEq, Eq)]
enum ParsedStreamEvent {
    Delta(String),
    Done,
    None,
}

struct AiActionStreamSink {
    channel: Channel<AiActionStreamEvent>,
    sent_delta: bool,
}

impl AiActionStreamSink {
    fn new(channel: Channel<AiActionStreamEvent>) -> Self {
        Self {
            channel,
            sent_delta: false,
        }
    }

    fn delta(&mut self, text: String) -> Result<(), String> {
        if text.is_empty() {
            return Ok(());
        }
        self.sent_delta = true;
        self.channel
            .send(AiActionStreamEvent::Delta { text })
            .map_err(|e| e.to_string())
    }

    fn done(&self) -> Result<(), String> {
        self.channel
            .send(AiActionStreamEvent::Done)
            .map_err(|e| e.to_string())
    }

    fn error(&self, message: String) -> Result<(), String> {
        self.channel
            .send(AiActionStreamEvent::Error { message })
            .map_err(|e| e.to_string())
    }
}

fn ai_action_prompt_template<'a>(
    prompts: &'a ActionPromptConfig,
    action: &str,
) -> Result<&'a str, String> {
    match action {
        "explain" => Ok(&prompts.explain),
        "dict" => Ok(&prompts.dict),
        "summary" | "summarize" => Ok(&prompts.summary),
        "polish" => Ok(&prompts.polish),
        _ => Err(format!("Unknown action: {}", action)),
    }
}

fn format_ai_action_prompt(
    template: &str,
    text: &str,
    source_lang: &str,
    target_lang: &str,
) -> String {
    let source_label = if source_lang == "auto" {
        "the detected language"
    } else {
        source_lang
    };
    template
        .replace("{from}", source_label)
        .replace("{to}", target_lang)
        .replace("{text}", text)
}

fn ai_action_request(
    config: &config::AppConfig,
    text: &str,
    action: &str,
    source_lang: &str,
    target_lang: &str,
) -> Result<(config::ProviderConfig, String), String> {
    let provider = config
        .providers
        .iter()
        .find(|provider| provider.name == config.active_provider)
        .ok_or_else(|| format!("Provider '{}' not found", config.active_provider))?
        .clone();
    let prompt_template = ai_action_prompt_template(&config.action_prompts, action)?;
    let prompt = format_ai_action_prompt(prompt_template, text, source_lang, target_lang);

    Ok((provider, prompt))
}

fn system_action_prompt(prompt: &str, source_lang: &str, target_lang: &str) -> String {
    format_ai_action_prompt(
        &config::default_ai_action_system_prompt(),
        prompt,
        source_lang,
        target_lang,
    )
}

fn ai_action_provider(
    provider: &config::ProviderConfig,
) -> Result<Arc<dyn TranslateProvider>, TranslateError> {
    let system_prompt = config::default_ai_action_system_prompt();
    match provider.name.as_str() {
        "openai" => Ok(Arc::new(OpenAICompatProvider::new(
            &provider.base_url,
            &provider.api_key,
            &provider.model,
            &system_prompt,
            "{text}",
            provider.extra_params.clone(),
        ))),
        "gemini" => Ok(Arc::new(GeminiProvider::new(
            &provider.base_url,
            &provider.api_key,
            &provider.model,
            &system_prompt,
            "{text}",
        ))),
        "claude" => Ok(Arc::new(ClaudeProvider::new(
            &provider.base_url,
            &provider.api_key,
            &provider.model,
            &system_prompt,
            "{text}",
        ))),
        "ollama" => Ok(Arc::new(OllamaProvider::new(
            &provider.base_url,
            &provider.model,
            &system_prompt,
            "{text}",
        ))),
        other => Err(TranslateError::Config(format!(
            "AI actions require an LLM provider; '{}' is not supported",
            other
        ))),
    }
}

fn ai_action_http_client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(AI_ACTION_CONNECT_TIMEOUT)
        .read_timeout(AI_ACTION_READ_TIMEOUT)
        .timeout(AI_ACTION_TOTAL_TIMEOUT)
        .build()
        .map_err(|e| format!("Failed to create AI action HTTP client: {}", e))
}

fn content_value_to_text(value: &serde_json::Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }

    let parts = value.as_array()?;
    let text = parts
        .iter()
        .filter_map(|part| {
            part.get("text")
                .or_else(|| part.get("content"))
                .and_then(|value| value.as_str())
        })
        .collect::<Vec<_>>()
        .join("");

    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn json_error_message(json: &serde_json::Value) -> Option<String> {
    let error = json.get("error")?;
    if let Some(message) = error.as_str() {
        return Some(message.to_string());
    }
    error
        .get("message")
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
}

fn parse_openai_stream_event(json: &serde_json::Value) -> Result<ParsedStreamEvent, String> {
    if let Some(message) = json_error_message(json) {
        return Err(message);
    }

    if json
        .get("type")
        .and_then(|value| value.as_str())
        .is_some_and(|event_type| event_type == "response.completed")
    {
        return Ok(ParsedStreamEvent::Done);
    }

    if let Some(delta) = json.get("delta").and_then(|value| value.as_str()) {
        return Ok(ParsedStreamEvent::Delta(delta.to_string()));
    }

    let Some(choice) = json.get("choices").and_then(|value| value.get(0)) else {
        return Ok(ParsedStreamEvent::None);
    };

    if let Some(content) = choice
        .get("delta")
        .and_then(|delta| delta.get("content"))
        .and_then(content_value_to_text)
    {
        return Ok(ParsedStreamEvent::Delta(content));
    }

    if let Some(text) = choice.get("text").and_then(|value| value.as_str()) {
        return Ok(ParsedStreamEvent::Delta(text.to_string()));
    }

    if choice
        .get("finish_reason")
        .is_some_and(|value| !value.is_null())
    {
        return Ok(ParsedStreamEvent::Done);
    }

    Ok(ParsedStreamEvent::None)
}

fn parse_ollama_stream_event(json: &serde_json::Value) -> Result<ParsedStreamEvent, String> {
    if let Some(message) = json_error_message(json) {
        return Err(message);
    }

    if json.get("done").and_then(|value| value.as_bool()) == Some(true) {
        return Ok(ParsedStreamEvent::Done);
    }

    if let Some(content) = json
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(|value| value.as_str())
    {
        return Ok(ParsedStreamEvent::Delta(content.to_string()));
    }

    Ok(ParsedStreamEvent::None)
}

fn parse_claude_stream_event(json: &serde_json::Value) -> Result<ParsedStreamEvent, String> {
    if json.get("type").and_then(|value| value.as_str()) == Some("error") {
        return Err(json_error_message(json).unwrap_or_else(|| "Claude stream error".to_string()));
    }

    if json.get("type").and_then(|value| value.as_str()) == Some("message_stop") {
        return Ok(ParsedStreamEvent::Done);
    }

    if let Some(text) = json
        .get("delta")
        .and_then(|delta| delta.get("text"))
        .and_then(|value| value.as_str())
    {
        return Ok(ParsedStreamEvent::Delta(text.to_string()));
    }

    Ok(ParsedStreamEvent::None)
}

fn parse_gemini_stream_event(json: &serde_json::Value) -> Result<ParsedStreamEvent, String> {
    if let Some(message) = json_error_message(json) {
        return Err(message);
    }

    if let Some(text) = json
        .get("candidates")
        .and_then(|value| value.get(0))
        .and_then(|candidate| candidate.get("content"))
        .and_then(|content| content.get("parts"))
        .and_then(|parts| parts.get(0))
        .and_then(|part| part.get("text"))
        .and_then(|value| value.as_str())
    {
        return Ok(ParsedStreamEvent::Delta(text.to_string()));
    }

    Ok(ParsedStreamEvent::None)
}

fn process_stream_event(
    parsed: ParsedStreamEvent,
    sink: &mut AiActionStreamSink,
) -> Result<bool, String> {
    match parsed {
        ParsedStreamEvent::Delta(text) => {
            sink.delta(text)?;
            Ok(false)
        }
        ParsedStreamEvent::Done => Ok(true),
        ParsedStreamEvent::None => Ok(false),
    }
}

fn process_sse_line<F>(line: &str, sink: &mut AiActionStreamSink, parse: F) -> Result<bool, String>
where
    F: Fn(&serde_json::Value) -> Result<ParsedStreamEvent, String>,
{
    let line = line.trim();
    if line.is_empty() || line.starts_with(':') {
        return Ok(false);
    }

    let Some(data) = line.strip_prefix("data:") else {
        return Ok(false);
    };
    let data = data.trim();
    if data == "[DONE]" {
        return Ok(true);
    }

    let json: serde_json::Value =
        serde_json::from_str(data).map_err(|e| format!("Stream JSON parse failed: {}", e))?;
    process_stream_event(parse(&json)?, sink)
}

fn process_json_line<F>(line: &str, sink: &mut AiActionStreamSink, parse: F) -> Result<bool, String>
where
    F: Fn(&serde_json::Value) -> Result<ParsedStreamEvent, String>,
{
    let line = line.trim();
    if line.is_empty() {
        return Ok(false);
    }
    let json: serde_json::Value =
        serde_json::from_str(line).map_err(|e| format!("Stream JSON parse failed: {}", e))?;
    process_stream_event(parse(&json)?, sink)
}

fn next_line(buffer: &mut Vec<u8>) -> Result<Option<String>, String> {
    let Some(index) = buffer.iter().position(|byte| *byte == b'\n') else {
        return Ok(None);
    };
    let mut line = buffer.drain(..=index).collect::<Vec<_>>();
    if line.last() == Some(&b'\n') {
        line.pop();
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    String::from_utf8(line)
        .map(Some)
        .map_err(|e| format!("Stream UTF-8 parse failed: {}", e))
}

fn remaining_line(buffer: &mut Vec<u8>) -> Result<Option<String>, String> {
    if buffer.is_empty() {
        return Ok(None);
    }
    let bytes = std::mem::take(buffer);
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|e| format!("Stream UTF-8 parse failed: {}", e))
}

async fn stream_sse_response<F>(
    mut response: reqwest::Response,
    sink: &mut AiActionStreamSink,
    parse: F,
) -> Result<(), String>
where
    F: Fn(&serde_json::Value) -> Result<ParsedStreamEvent, String> + Copy,
{
    let mut buffer = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        buffer.extend_from_slice(&chunk);
        while let Some(line) = next_line(&mut buffer)? {
            if process_sse_line(&line, sink, parse)? {
                return Ok(());
            }
        }
    }

    if let Some(line) = remaining_line(&mut buffer)? {
        let _ = process_sse_line(&line, sink, parse)?;
    }
    Ok(())
}

async fn stream_json_lines_response<F>(
    mut response: reqwest::Response,
    sink: &mut AiActionStreamSink,
    parse: F,
) -> Result<(), String>
where
    F: Fn(&serde_json::Value) -> Result<ParsedStreamEvent, String> + Copy,
{
    let mut buffer = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        buffer.extend_from_slice(&chunk);
        while let Some(line) = next_line(&mut buffer)? {
            if process_json_line(&line, sink, parse)? {
                return Ok(());
            }
        }
    }

    if let Some(line) = remaining_line(&mut buffer)? {
        let _ = process_json_line(&line, sink, parse)?;
    }
    Ok(())
}

async fn ensure_stream_response(response: reqwest::Response) -> Result<reqwest::Response, String> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let body_text = response.text().await.unwrap_or_default();
    Err(format!("HTTP {}: {}", status, body_text))
}

async fn stream_openai_action(
    provider: &config::ProviderConfig,
    prompt: &str,
    source_lang: &str,
    target_lang: &str,
    sink: &mut AiActionStreamSink,
) -> Result<(), String> {
    let mut body = serde_json::json!({
        "model": provider.model,
        "messages": [
            { "role": "system", "content": system_action_prompt(prompt, source_lang, target_lang) },
            { "role": "user", "content": prompt }
        ],
        "temperature": 0.3,
        "max_tokens": 2048,
        "chat_template_kwargs": {
            "enable_thinking": false
        }
    });
    OpenAICompatProvider::merge_extra_params(&mut body, &provider.extra_params);
    body["stream"] = serde_json::json!(true);

    let response = ai_action_http_client()?
        .post(format!("{}/chat/completions", provider.base_url))
        .header("Authorization", format!("Bearer {}", provider.api_key))
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    stream_sse_response(
        ensure_stream_response(response).await?,
        sink,
        parse_openai_stream_event,
    )
    .await
}

async fn stream_ollama_action(
    provider: &config::ProviderConfig,
    prompt: &str,
    source_lang: &str,
    target_lang: &str,
    sink: &mut AiActionStreamSink,
) -> Result<(), String> {
    let body = serde_json::json!({
        "model": provider.model,
        "messages": [
            { "role": "system", "content": system_action_prompt(prompt, source_lang, target_lang) },
            { "role": "user", "content": prompt }
        ],
        "stream": true
    });

    let response = ai_action_http_client()?
        .post(format!("{}/api/chat", provider.base_url))
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    stream_json_lines_response(
        ensure_stream_response(response).await?,
        sink,
        parse_ollama_stream_event,
    )
    .await
}

async fn stream_claude_action(
    provider: &config::ProviderConfig,
    prompt: &str,
    source_lang: &str,
    target_lang: &str,
    sink: &mut AiActionStreamSink,
) -> Result<(), String> {
    let body = serde_json::json!({
        "model": provider.model,
        "max_tokens": 2048,
        "system": system_action_prompt(prompt, source_lang, target_lang),
        "messages": [
            { "role": "user", "content": prompt }
        ],
        "stream": true
    });

    let response = ai_action_http_client()?
        .post(format!("{}/v1/messages", provider.base_url))
        .header("x-api-key", &provider.api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    stream_sse_response(
        ensure_stream_response(response).await?,
        sink,
        parse_claude_stream_event,
    )
    .await
}

async fn stream_gemini_action(
    provider: &config::ProviderConfig,
    prompt: &str,
    source_lang: &str,
    target_lang: &str,
    sink: &mut AiActionStreamSink,
) -> Result<(), String> {
    let body = serde_json::json!({
        "systemInstruction": {
            "parts": [{ "text": system_action_prompt(prompt, source_lang, target_lang) }]
        },
        "contents": [{
            "parts": [{ "text": prompt }]
        }],
        "generationConfig": {
            "temperature": 0.3,
            "maxOutputTokens": 2048
        }
    });

    let response = ai_action_http_client()?
        .post(format!(
            "{}/models/{}:streamGenerateContent?alt=sse&key={}",
            provider.base_url, provider.model, provider.api_key
        ))
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    stream_sse_response(
        ensure_stream_response(response).await?,
        sink,
        parse_gemini_stream_event,
    )
    .await
}

async fn stream_ai_action(
    provider: &config::ProviderConfig,
    prompt: &str,
    source_lang: &str,
    target_lang: &str,
    sink: &mut AiActionStreamSink,
) -> Result<(), String> {
    config::validate_provider(provider)?;

    match provider.name.as_str() {
        "openai" => stream_openai_action(provider, prompt, source_lang, target_lang, sink).await,
        "ollama" => stream_ollama_action(provider, prompt, source_lang, target_lang, sink).await,
        "claude" => stream_claude_action(provider, prompt, source_lang, target_lang, sink).await,
        "gemini" => stream_gemini_action(provider, prompt, source_lang, target_lang, sink).await,
        other => Err(format!(
            "AI actions require an LLM provider; '{}' is not supported",
            other
        )),
    }
}

async fn run_ai_action_stream_task(
    provider_config: config::ProviderConfig,
    prompt: String,
    source_lang: String,
    target_lang: String,
    on_event: Channel<AiActionStreamEvent>,
) {
    let mut sink = AiActionStreamSink::new(on_event);
    log::info!(
        "[ai_action] Streaming action with provider={}",
        provider_config.name
    );

    async fn fallback_non_stream(
        provider_config: &config::ProviderConfig,
        prompt: &str,
        source_lang: &str,
        target_lang: &str,
        sink: &mut AiActionStreamSink,
    ) -> Result<(), String> {
        let provider = ai_action_provider(provider_config).map_err(|e| e.to_string())?;
        let result = provider
            .translate(prompt, source_lang, target_lang)
            .await
            .map_err(|e| e.to_string())?;
        sink.delta(result.translated)?;
        sink.done()
    }

    match stream_ai_action(
        &provider_config,
        &prompt,
        &source_lang,
        &target_lang,
        &mut sink,
    )
    .await
    {
        Ok(()) if sink.sent_delta => {
            let _ = sink.done();
            log::info!("[ai_action] Streaming action finished");
        }
        Ok(()) => {
            log::warn!("[ai_action] Stream finished without tokens; falling back to non-stream");
            match fallback_non_stream(
                &provider_config,
                &prompt,
                &source_lang,
                &target_lang,
                &mut sink,
            )
            .await
            {
                Ok(()) => log::info!("[ai_action] Non-stream fallback finished"),
                Err(error) => {
                    let _ = sink.error(error.clone());
                    log::warn!("[ai_action] Non-stream fallback failed: {}", error);
                }
            }
        }
        Err(stream_error) if !sink.sent_delta => {
            log::warn!(
                "[ai_action] Streaming failed before first token; falling back to non-stream: {}",
                stream_error
            );
            match fallback_non_stream(
                &provider_config,
                &prompt,
                &source_lang,
                &target_lang,
                &mut sink,
            )
            .await
            {
                Ok(()) => log::info!("[ai_action] Non-stream fallback finished"),
                Err(error) => {
                    let _ = sink.error(error.clone());
                    log::warn!("[ai_action] Non-stream fallback failed: {}", error);
                }
            }
        }
        Err(stream_error) => {
            let _ = sink.error(stream_error.clone());
            log::warn!("[ai_action] Streaming action failed: {}", stream_error);
        }
    }
}

#[tauri::command]
pub async fn translate_text(
    text: String,
    from: String,
    to: String,
    state: State<'_, AppState>,
) -> Result<TranslateResult, String> {
    let engine = state.translation_engine.read().await;
    engine
        .translate(&text, &from, &to)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_history(state: State<'_, AppState>) -> Result<Vec<HistoryEntry>, String> {
    Ok(state.history.get_all())
}

#[tauri::command]
pub async fn clear_history(state: State<'_, AppState>) -> Result<(), String> {
    state.history.clear();
    Ok(())
}

#[tauri::command]
pub async fn ai_action(
    text: String,
    action: String,
    source_lang: String,
    target_lang: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let (provider_config, prompt_template) = {
        let config = state
            .config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        ai_action_request(&config, &text, &action, &source_lang, &target_lang)?
    };

    config::validate_provider(&provider_config)?;
    let provider = ai_action_provider(&provider_config).map_err(|e| e.to_string())?;
    let result = provider
        .translate(&prompt_template, &source_lang, &target_lang)
        .await
        .map_err(|e| e.to_string())?;

    Ok(result.translated)
}

#[tauri::command]
pub async fn ai_action_stream(
    text: String,
    action: String,
    source_lang: String,
    target_lang: String,
    on_event: Channel<AiActionStreamEvent>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let (provider_config, prompt) = {
        let config = state
            .config
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        ai_action_request(&config, &text, &action, &source_lang, &target_lang)?
    };
    config::validate_provider(&provider_config)?;

    tauri::async_runtime::spawn(run_ai_action_stream_task(
        provider_config,
        prompt,
        source_lang,
        target_lang,
        on_event,
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_action_prompt_supports_polish_and_summarize_alias() {
        let prompts = ActionPromptConfig::default();
        let polish = ai_action_prompt_template(&prompts, "polish").unwrap();
        assert!(polish.contains("clearer"));

        let summary = ai_action_prompt_template(&prompts, "summarize").unwrap();
        assert!(summary.contains("Summarize"));
    }

    #[test]
    fn ai_action_prompt_rejects_unknown_actions() {
        let prompts = ActionPromptConfig::default();
        let error = ai_action_prompt_template(&prompts, "unknown").unwrap_err();

        assert_eq!(error, "Unknown action: unknown");
    }

    #[test]
    fn ai_action_prompt_formats_template_tokens() {
        let prompt = format_ai_action_prompt(
            "Explain from {from} to {to}: {text}",
            "hello {to}",
            "auto",
            "zh",
        );

        assert_eq!(
            prompt,
            "Explain from the detected language to zh: hello {to}"
        );
    }

    #[test]
    fn ai_action_provider_rejects_non_llm_provider() {
        let provider = config::ProviderConfig {
            name: "deeplx".to_string(),
            base_url: "http://127.0.0.1:1188".to_string(),
            api_key: String::new(),
            model: String::new(),
            system_prompt: String::new(),
            user_prompt: String::new(),
            extra_params: serde_json::json!({}),
        };

        let error = match ai_action_provider(&provider) {
            Ok(_) => panic!("expected non-LLM provider to be rejected"),
            Err(error) => error.to_string(),
        };

        assert!(error.contains("AI actions require an LLM provider"));
    }

    #[test]
    fn ai_action_stream_timeouts_are_bounded() {
        assert!(AI_ACTION_CONNECT_TIMEOUT <= Duration::from_secs(10));
        assert!(AI_ACTION_READ_TIMEOUT <= Duration::from_secs(30));
        assert!(AI_ACTION_TOTAL_TIMEOUT <= Duration::from_secs(180));
    }

    #[test]
    fn parses_openai_stream_delta() {
        let json = serde_json::json!({
            "choices": [{ "delta": { "content": "hello" } }]
        });

        assert_eq!(
            parse_openai_stream_event(&json).unwrap(),
            ParsedStreamEvent::Delta("hello".to_string())
        );
    }

    #[test]
    fn parses_ollama_stream_delta() {
        let json = serde_json::json!({
            "message": { "content": "world" },
            "done": false
        });

        assert_eq!(
            parse_ollama_stream_event(&json).unwrap(),
            ParsedStreamEvent::Delta("world".to_string())
        );
    }

    #[test]
    fn parses_claude_stream_delta() {
        let json = serde_json::json!({
            "type": "content_block_delta",
            "delta": { "type": "text_delta", "text": "chunk" }
        });

        assert_eq!(
            parse_claude_stream_event(&json).unwrap(),
            ParsedStreamEvent::Delta("chunk".to_string())
        );
    }

    #[test]
    fn parses_gemini_stream_delta() {
        let json = serde_json::json!({
            "candidates": [{
                "content": {
                    "parts": [{ "text": "gemini" }]
                }
            }]
        });

        assert_eq!(
            parse_gemini_stream_event(&json).unwrap(),
            ParsedStreamEvent::Delta("gemini".to_string())
        );
    }
}
