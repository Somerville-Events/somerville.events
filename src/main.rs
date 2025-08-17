use actix_rt::System;
use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD as b64, Engine as _};
use mime_guess::MimeGuess;
use std::{env, fs, path::Path};
use dotenv::dotenv;
use serde::{Deserialize, Serialize};
use serde_json::json;
use schemars::{schema_for, JsonSchema};



#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct EventExtraction {
    /// The name of the event
    name: String,
    /// The full description of the event or content
    full_description: String,
    /// The date and time of the event (ISO 8601 format: YYYY-MM-DDTHH:MM:SSZ)
    start_date: Option<Vec<String>>,
    /// The end date of the event (ISO 8601 format: YYYY-MM-DDTHH:MM:SSZ)
    end_date: Option<Vec<String>>,
    /// The location of the event
    location: Option<String>,
    /// Type of event (e.g., "YardSale", "Art", "Dance", "Performance", "Food", "PersonalService", "CivicEvent", "Other")
    event_type: Option<String>,
    /// Any additional relevant details
    additional_details: Option<Vec<String>>,
    /// Confidence level of the extraction (0.0 to 1.0)
    confidence: f64,
}

fn parse_and_validate_response(content: &str) -> Result<EventExtraction> {
    // First try to parse as the exact struct
    if let Ok(result) = serde_json::from_str::<EventExtraction>(content) {
        return Ok(result);
    }
    
    // If that fails, parse as generic JSON and clean it up
    let mut json: serde_json::Value = serde_json::from_str(content)
        .map_err(|e| anyhow!("Failed to parse JSON: {}", e))?;
    
    // Remove any extra fields not in our struct
    let allowed_fields = ["name", "full_description", "date", "location", "event_type", "additional_details", "confidence"];
    
    if let Some(obj) = json.as_object_mut() {
        // Keep only allowed fields
        let keys: Vec<String> = obj.keys().cloned().collect();
        for key in keys {
            if !allowed_fields.contains(&key.as_str()) {
                obj.remove(&key);
            }
        }
        
        // Convert lists to single values where appropriate
        if let Some(location) = obj.get("location") {
            if location.is_array() {
                if let Some(arr) = location.as_array() {
                    if !arr.is_empty() {
                        obj.insert("location".to_string(), arr[0].clone());
                    }
                }
            }
        }
        
        if let Some(event_type) = obj.get("event_type") {
            if event_type.is_array() {
                if let Some(arr) = event_type.as_array() {
                    if !arr.is_empty() {
                        obj.insert("event_type".to_string(), arr[0].clone());
                    }
                }
            }
        }
    }
    
    // Now try to parse the cleaned JSON
    serde_json::from_value(json)
        .map_err(|e| anyhow!("Failed to parse cleaned response: {}", e))
}

fn main() -> Result<()> {
    // Load .env file if present
    dotenv().ok();

    // Actix runtime entrypoint
    System::new().block_on(async_main())
}

async fn async_main() -> Result<()> {
    let image_path = env::args().nth(1).ok_or_else(|| {
        anyhow!("Usage: somerville-events <image-file.(png|jpg|jpeg|webp|gif)>")
    })?;

    let api_key = env::var("OPENAI_API_KEY")
        .map_err(|_| anyhow!("Set OPENAI_API_KEY in your .env file or environment"))?;

    // Read file
    let bytes = fs::read(&image_path)
        .map_err(|e| anyhow!("Failed to read file: {} - {}", image_path, e))?;

    // Guess MIME type
    let mime = guess_mime(&image_path).unwrap_or_else(|| "image/png".to_string());

    // Base64 encode -> data URL
    let b64_data = b64.encode(&bytes);
    let data_url = format!("data:{mime};base64,{b64_data}");

    let schema = schema_for!(EventExtraction);
    let schema_str = serde_json::to_string_pretty(&schema).unwrap();
    println!("{schema_str}");


    // Build Chat Completions payload with instructor format
    let payload = json!({
        "model": "gpt-4o-mini",
        "temperature": 0,
        "response_format": { "type": "json_object" },
        "messages": [
            {
                "role": "system", 
                "content": format!(
                    r#"You are an expert at extracting event information from images. 
                    You must respond with a JSON object that matches this exact schema:
                    {schema:#?}
                    The text field should contain all readable text from the image. 
                    The confidence should be a number between 0.0 and 1.0 indicating how confident you are in the extraction. 
                    Focus on extracting event-related information like the name, date, time, location, and description. 
                    Never return multiple locations.
                    Never return multiple event types.
                    Be thorough but accurate. Return only valid JSON.
                    Do not return the schema in your response.
                    "#
                )
            },
            {
                "role": "user", 
                "content": [
                    { "type": "text", "text": "Extract all text from this image and return it in the specified JSON format." },
                    { "type": "image_url", "image_url": { "url": data_url } }
                ]
            }
        ]
    });

    // Send request with Actix awc client (with longer timeout)
    let client = awc::ClientBuilder::new()
        .timeout(std::time::Duration::from_secs(120)) // 2 minute timeout
        .finish();
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
            String::from_utf8_lossy(&body)
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

    println!("Debug: Extracted content: {}", content);

    // Parse and validate the structured response
    let event_result = parse_and_validate_response(&content)?;

    // Display the structured output
    println!("=== Event Information Extraction ===");
    println!("Name: {}", event_result.name);
    println!("Full Description: {}", event_result.full_description);
    if let Some(start_date) = &event_result.start_date {
        println!("Start Date: {}", start_date.join(", "));
    }
    if let Some(end_date) = &event_result.end_date {
        println!("End Date: {}", end_date.join(", "));
    }
    if let Some(location) = &event_result.location {
        println!("Location: {}", location);
    }
    if let Some(event_type) = &event_result.event_type {
        println!("Event Type: {}", event_type);
    }
    if let Some(details) = &event_result.additional_details {
        println!("Additional Details: {}", details.join(", "));
    }
    println!("Confidence: {:.1}%", event_result.confidence * 100.0);
    println!("===================================");

    Ok(())
}

// Helpers

fn guess_mime<P: AsRef<Path>>(p: P) -> Option<String> {
    let guess: MimeGuess = mime_guess::from_path(p);
    guess.first_raw().map(|m| m.to_string())
}
