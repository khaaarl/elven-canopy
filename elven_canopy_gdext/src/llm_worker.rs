// LLM inference worker thread.
//
// Provides `LlmWorker`, a long-lived background thread that runs LLM inference
// via `elven_canopy_llm::LlmEngine`. The main thread communicates through two
// mpsc channels: one for commands (load model, submit inference, shutdown) and
// one for results (completed inferences ready to send back to the relay).
//
// The worker is spawned lazily on first `load_llm_model` call and lives for the
// rest of the game session.
// It starts idle (no model loaded). When GDScript detects a model file is
// available (via `ModelManager`), it calls `SimBridge::load_llm_model(path)`
// which sends a `LoadModel` command. Model loading happens on the worker thread
// since `LlmEngine` is `!Send` — it must be created and used on the same thread.
//
// Inference requests arrive via `LlmDispatch` relay messages. If no model is
// loaded, requests are silently dropped (the sim's deadline expiry handles it).
// Requests are processed serially from a queue.
//
// See also: `sim_bridge.rs` for integration, `elven_canopy_llm/src/engine.rs`
// for the inference API, `mesh_cache.rs` for a similar worker pattern.

use std::sync::mpsc;
use std::thread::{self, JoinHandle};

/// Command sent from the main thread to the worker.
pub enum LlmWorkerCmd {
    /// Load a GGUF model from the given path. If a model is already loaded,
    /// it is dropped first. `use_gpu` offloads all layers to GPU when true.
    LoadModel { path: String, use_gpu: bool },
    /// Unload the current model (if any), freeing memory.
    UnloadModel,
    /// Run inference for a relay-dispatched request.
    Infer {
        request_id: u64,
        prompt: String,
        max_tokens: u32,
    },
    /// Shut down the worker thread. Completes any in-progress inference first.
    Shutdown,
}

/// Result of a completed inference, sent from the worker to the main thread.
pub struct LlmWorkerResult {
    pub request_id: u64,
    /// Serialized `LlmResponsePayload` bytes, ready to send as
    /// `ClientMessage::LlmResponse` payload.
    pub payload: Vec<u8>,
}

/// Handle to the LLM inference worker thread. Owned by `SimBridge`.
pub struct LlmWorker {
    cmd_tx: mpsc::Sender<LlmWorkerCmd>,
    pub result_rx: mpsc::Receiver<LlmWorkerResult>,
    thread: Option<JoinHandle<()>>,
}

impl LlmWorker {
    /// Spawn the worker thread. It starts idle (no model loaded).
    pub fn new() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<LlmWorkerCmd>();
        let (result_tx, result_rx) = mpsc::channel::<LlmWorkerResult>();

        let thread = thread::Builder::new()
            .name("llm-worker".into())
            .spawn(move || {
                worker_loop(cmd_rx, result_tx);
            })
            .expect("failed to spawn llm-worker thread");

        Self {
            cmd_tx,
            result_rx,
            thread: Some(thread),
        }
    }

    /// Send a command to the worker. Returns false if the worker has exited.
    pub fn send(&self, cmd: LlmWorkerCmd) -> bool {
        self.cmd_tx.send(cmd).is_ok()
    }

    /// Signal the worker to shut down and block until it exits.
    pub fn shutdown(&mut self) {
        let _ = self.cmd_tx.send(LlmWorkerCmd::Shutdown);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for LlmWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// The worker thread's main loop. Blocks on the command channel, processes
/// commands serially. Model loading and inference happen here.
fn worker_loop(cmd_rx: mpsc::Receiver<LlmWorkerCmd>, result_tx: mpsc::Sender<LlmWorkerResult>) {
    let mut engine: Option<elven_canopy_llm::LlmEngine> = None;

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            LlmWorkerCmd::LoadModel { path, use_gpu } => {
                let mode = if use_gpu { "GPU" } else { "CPU" };
                eprintln!("llm-worker: loading model from {path} ({mode})");
                let model_path = std::path::Path::new(&path);
                let n_gpu_layers = if use_gpu { 999 } else { 0 };
                match elven_canopy_llm::LlmEngine::new(model_path, 2048, n_gpu_layers) {
                    Ok(e) => {
                        eprintln!("llm-worker: model loaded successfully");
                        engine = Some(e);
                    }
                    Err(e) => {
                        eprintln!("llm-worker: model load failed: {e}");
                        engine = None;
                    }
                }
            }
            LlmWorkerCmd::UnloadModel => {
                if engine.is_some() {
                    eprintln!("llm-worker: unloading model");
                    engine = None;
                }
            }
            LlmWorkerCmd::Infer {
                request_id,
                prompt,
                max_tokens,
            } => {
                let Some(eng) = engine.as_mut() else {
                    // No model loaded — silently drop. The sim's deadline
                    // expiry will clean up the pending request.
                    continue;
                };

                let request = elven_canopy_llm::InferenceRequest { prompt, max_tokens };

                match eng.infer(&request) {
                    Ok(inference_result) => {
                        // Build the response payload using the shared wire-format
                        // type from sim_bridge.rs.
                        let response = crate::sim_bridge::LlmResponsePayload {
                            result_json: inference_result.text,
                            metadata: elven_canopy_sim::llm::InferenceMetadata {
                                latency_ms: inference_result.metadata.latency_ms,
                                token_count: inference_result.metadata.prompt_tokens
                                    + inference_result.metadata.completion_tokens,
                                cache_hit: false,
                                prefill_tokens: inference_result.metadata.prompt_tokens,
                                decode_tokens: inference_result.metadata.completion_tokens,
                            },
                        };
                        match serde_json::to_vec(&response) {
                            Ok(payload) => {
                                let _ = result_tx.send(LlmWorkerResult {
                                    request_id,
                                    payload,
                                });
                            }
                            Err(e) => {
                                eprintln!(
                                    "llm-worker: failed to serialize response for request {request_id}: {e}"
                                );
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("llm-worker: inference failed for request {request_id}: {e}");
                        // Silently drop — deadline expiry handles it.
                    }
                }
            }
            LlmWorkerCmd::Shutdown => {
                eprintln!("llm-worker: shutting down");
                break;
            }
        }
    }
}
