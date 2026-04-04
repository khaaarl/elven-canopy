//! Core inference engine wrapping llama-cpp-2.
//!
//! Provides `LlmEngine` for model loading and `InferenceRequest`/`InferenceResult`
//! for running grammar-constrained generation. The engine is designed to live on
//! a dedicated worker thread — `LlamaContext` is `!Send`, so the model and context
//! must be created and used on the same thread.

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Instant;

/// Metadata about an inference run, for observability.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InferenceMetadata {
    pub latency_ms: u32,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

/// A request for grammar-constrained inference.
pub struct InferenceRequest {
    /// The full prompt text (preambles + ephemeral context concatenated).
    pub prompt: String,
    /// JSON schema the output must conform to.
    pub response_schema: String,
    /// Maximum tokens to generate.
    pub max_tokens: u32,
}

/// The result of a successful inference.
pub struct InferenceResult {
    /// The generated text (guaranteed valid JSON per the schema).
    pub text: String,
    /// Observability metadata.
    pub metadata: InferenceMetadata,
}

/// Error type for inference operations.
#[derive(Debug)]
pub enum LlmError {
    BackendInit(String),
    ModelLoad(String),
    ContextCreate(String),
    Grammar(String),
    Inference(String),
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmError::BackendInit(e) => write!(f, "backend init failed: {e}"),
            LlmError::ModelLoad(e) => write!(f, "model load failed: {e}"),
            LlmError::ContextCreate(e) => write!(f, "context creation failed: {e}"),
            LlmError::Grammar(e) => write!(f, "grammar error: {e}"),
            LlmError::Inference(e) => write!(f, "inference failed: {e}"),
        }
    }
}

impl std::error::Error for LlmError {}

/// The LLM inference engine. Owns the backend, model, and context.
///
/// Must be created and used on the same thread (`LlamaContext` is `!Send`).
pub struct LlmEngine {
    _backend: LlamaBackend,
    model: LlamaModel,
    ctx_params: LlamaContextParams,
}

impl LlmEngine {
    /// Load a GGUF model from the filesystem.
    ///
    /// `n_ctx` is the context window size in tokens. 2048 is generous for our
    /// ~600 token prompts and leaves room for generation.
    pub fn new(model_path: &Path, n_ctx: u32) -> Result<Self, LlmError> {
        let backend =
            LlamaBackend::init().map_err(|e| LlmError::BackendInit(e.to_string()))?;

        let model_params = LlamaModelParams::default();
        let model = LlamaModel::load_from_file(&backend, model_path, &model_params)
            .map_err(|e| LlmError::ModelLoad(e.to_string()))?;

        let ctx_params = LlamaContextParams::default().with_n_ctx(std::num::NonZero::new(n_ctx));

        Ok(Self {
            _backend: backend,
            model,
            ctx_params,
        })
    }

    /// Run grammar-constrained inference on the given request.
    ///
    /// Converts the JSON schema to a GBNF grammar, tokenizes the prompt,
    /// processes it through the model, and samples tokens until EOS or
    /// `max_tokens`.
    pub fn infer(&self, request: &InferenceRequest) -> Result<InferenceResult, LlmError> {
        let start = Instant::now();

        // Create a fresh context for each request. KV cache reuse across
        // requests (preamble caching) is a future optimization.
        let mut ctx = self
            .model
            .new_context(&self._backend, self.ctx_params.clone())
            .map_err(|e| LlmError::ContextCreate(e.to_string()))?;

        // Convert JSON schema to GBNF grammar for constrained generation.
        let grammar_str =
            llama_cpp_2::grammar::json_schema_to_grammar(&request.response_schema)
                .map_err(|e| LlmError::Grammar(e.to_string()))?;

        // Build sampler chain with grammar constraint.
        let sampler = LlamaSampler::chain_simple([
            LlamaSampler::grammar(&self.model, &grammar_str, "root"),
            LlamaSampler::greedy(),
        ]);

        // Tokenize the prompt.
        let tokens = self
            .model
            .str_to_token(&request.prompt, llama_cpp_2::model::AddBos::Always)
            .map_err(|e| LlmError::Inference(e.to_string()))?;

        let prompt_token_count = tokens.len() as u32;

        // Process prompt tokens in a single batch.
        let mut batch = LlamaBatch::new(tokens.len(), 1);
        for (i, &token) in tokens.iter().enumerate() {
            let is_last = i == tokens.len() - 1;
            batch
                .add(token, i as i32, &[0], is_last)
                .map_err(|e| LlmError::Inference(format!("batch add failed: {e}")))?;
        }

        ctx.decode(&mut batch)
            .map_err(|e| LlmError::Inference(format!("prompt decode failed: {e}")))?;

        // Generate tokens one at a time until EOS or max_tokens.
        let mut output_tokens: Vec<LlamaToken> = Vec::new();
        let mut n_cur = tokens.len() as i32;

        for _ in 0..request.max_tokens {
            let token = sampler.sample(&ctx, -1);
            sampler.accept(token);

            // Check for end of generation.
            if self.model.is_eog_token(token) {
                break;
            }

            output_tokens.push(token);

            // Prepare next batch with just this token.
            batch.clear();
            batch
                .add(token, n_cur, &[0], true)
                .map_err(|e| LlmError::Inference(format!("batch add failed: {e}")))?;
            n_cur += 1;

            ctx.decode(&mut batch)
                .map_err(|e| LlmError::Inference(format!("decode failed: {e}")))?;
        }

        // Detokenize the output.
        let mut text = String::new();
        for token in &output_tokens {
            let piece = self
                .model
                .token_to_str(*token, llama_cpp_2::token::Special::Tokenize)
                .map_err(|e| LlmError::Inference(format!("detokenize failed: {e}")))?;
            text.push_str(&piece);
        }

        let elapsed = start.elapsed();

        Ok(InferenceResult {
            text,
            metadata: InferenceMetadata {
                latency_ms: elapsed.as_millis() as u32,
                prompt_tokens: prompt_token_count,
                completion_tokens: output_tokens.len() as u32,
            },
        })
    }
}
