//! Smoke test: load model, run inference, validate JSON output.
//!
//! Usage: cargo run -p elven_canopy_llm --bin test_inference
//!
//! Looks for the model at the default Godot user data path. Override with
//! the MODEL_PATH environment variable.

use elven_canopy_llm::{InferenceRequest, LlmEngine};
use std::path::PathBuf;

fn default_model_path() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME not set");
    PathBuf::from(home)
        .join(".local/share/godot/app_userdata/Elven Canopy/models/Qwen3-1.7B-Q5_K_M.gguf")
}

fn main() {
    let model_path = std::env::var("MODEL_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_model_path());

    println!("Loading model from: {}", model_path.display());
    if !model_path.exists() {
        eprintln!("Model file not found. Download it via the game's Settings > AI section,");
        eprintln!("or set MODEL_PATH to point to a GGUF file.");
        std::process::exit(1);
    }

    let mut engine = LlmEngine::new(&model_path, 2048).expect("failed to load model");
    println!("Model loaded successfully.\n");

    // Prompt the model for JSON output. No grammar constraint — we validate
    // the output ourselves, same as the real game pipeline will.
    let request = InferenceRequest {
        prompt: concat!(
            "You are a friendly elf named Aelindra. You see your friend Thalion ",
            "approaching. Respond ONLY with a JSON object, no other text.\n",
            "The JSON must have exactly these fields:\n",
            "- \"choice\": one of \"greet_warmly\", \"greet_coldly\", \"ignore\"\n",
            "- \"say\": a short sentence you say to Thalion\n",
            "JSON:\n",
        )
        .to_string(),
        max_tokens: 100,
    };

    println!("Running inference...");
    let result = engine.infer(&request).expect("inference failed");

    println!("--- Raw output ---");
    println!("{}", result.text);
    println!("---");
    println!(
        "Latency: {}ms, prompt: {} tokens, completion: {} tokens",
        result.metadata.latency_ms,
        result.metadata.prompt_tokens,
        result.metadata.completion_tokens,
    );

    // Try to extract the first complete JSON object from the output.
    // The model may produce extra text after the JSON.
    let json_str = extract_first_json_object(&result.text);

    if let Some(json_str) = json_str {
        println!("\nExtracted JSON: {json_str}");

        match serde_json::from_str::<serde_json::Value>(json_str) {
            Ok(val) => {
                println!(
                    "Valid JSON: {}",
                    serde_json::to_string_pretty(&val).unwrap()
                );
                if let Some(choice) = val.get("choice").and_then(|v| v.as_str()) {
                    if ["greet_warmly", "greet_coldly", "ignore"].contains(&choice) {
                        println!("Choice is valid: {choice}");
                    } else {
                        println!("Choice is not one of the expected values: {choice}");
                    }
                } else {
                    println!("Missing or non-string 'choice' field");
                }
            }
            Err(e) => println!("JSON parse failed: {e}"),
        }
    } else {
        println!("\nNo JSON object found in output.");
    }

    println!("\nDone.");
}

/// Extract the first balanced `{...}` from the text, respecting string literals.
fn extract_first_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let mut depth = 0;
    let mut in_string = false;
    let mut escape_next = false;

    for (i, ch) in text[start..].char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }
        if in_string {
            if ch == '\\' {
                escape_next = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..start + i + 1]);
                }
            }
            _ => {}
        }
    }
    None
}
