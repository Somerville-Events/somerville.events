use actix_rt::System;
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as b64, Engine as _};
use mime_guess::MimeGuess;
use serde_json::json;
use std::{env, fs, path::Path};
use dotenv::dotenv;

fn main() -> Result<()> {
    // Load .env file if present
    dotenv().ok();

    // Actix runtime entrypoint
    System::new().block_on(async_main())
}

async fn async_main() -> Result<()> {
    let image_path = env::args().nth(1).ok_or_else(|| {
        anyhow!("Usage: actix_ocr_gpt <image-file.(png|jpg|jpeg|webp|gif)>")
    })?;

    let api_key = env::var("OPENAI_API_KEY")
        .context("Set OPENAI_API_KEY in your .env file or environment")?;

    // Read file
    let bytes = fs::read(&image_path)
        .with_context(|| format!("Failed to read file: {}", image_path))?;

    // Guess MIME type
    let mime = guess_mime(&image_path).unwrap_or_else(|| "image/png".to_string());

    // Base64 encode -> data URL
    let b64_data = b64.encode(&bytes);
    let data_url = format!("data:{mime};base64,{b64_data}");

    // Build Chat Completions payload
    let payload = json!({
        "model": "gpt-4o-mini",
        "temperature": 0,
        "messages": [
            {"role": "system", "content": "You are an OCR tool. Output only the recognized text."},
            {
                "role": "user",
                "content": [
                    { "type": "text", "text": "Extract all text from this image. Return plain UTF-8 text." },
                    { "type": "image_url", "image_url": { "url": data_url } }
                ]
            }
        ]
    });

    // Send request with Actix awc client
    let client = awc::Client::default();
    let mut resp = client
        .post("https://api.openai.com/v1/chat/completions")
        .insert_header(("Authorization", format!("Bearer {}", api_key)))
        .insert_header(("Content-Type", "application/json"))
        .send_json(&payload)
        .await
        .map_err(|e| anyhow!("HTTP request failed: {}", e))?;

    let body = resp.body().await
        .map_err(|e| anyhow!("Failed to read response body: {}", e))?;
    
    if !resp.status().is_success() {
        return Err(anyhow!(
            "OpenAI API error ({}): {}",
            resp.status(),
            truncate(&String::from_utf8_lossy(&body), 2000)
        ));
    }
    let json: serde_json::Value =
        serde_json::from_slice(&body)
            .map_err(|e| anyhow!("Failed to parse JSON response: {}", e))?;

    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();

    println!("{}", content);

    Ok(())
}

// Helpers

fn guess_mime<P: AsRef<Path>>(p: P) -> Option<String> {
    let guess: MimeGuess = mime_guess::from_path(p);
    guess.first_raw().map(|m| m.to_string())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}… (truncated)", &s[..max])
    }
}
