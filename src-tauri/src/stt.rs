use anyhow::{anyhow, Context, Result};
use reqwest::multipart::{Form, Part};
use serde::Deserialize;

const GROQ_TRANSCRIBE_URL: &str = "https://api.groq.com/openai/v1/audio/transcriptions";
const GROQ_MODEL: &str = "whisper-large-v3-turbo";

#[derive(Debug, Deserialize)]
struct GroqResponse {
    text: String,
}

pub async fn transcribe_audio(
    api_key: &str,
    audio_bytes: Vec<u8>,
    mime_type: &str,
    language: &str,
    model: &str,
) -> Result<String> {
    if api_key.is_empty() {
        return Err(anyhow!("GROQ_API_KEY is empty"));
    }
    if audio_bytes.is_empty() {
        return Err(anyhow!("empty audio buffer"));
    }

    // Settings dropdowns can legitimately be blank when the user has never
    // touched them; fall back to the shipped defaults rather than POSTing an
    // empty form field to Groq.
    let language = if language.trim().is_empty() { "en" } else { language };
    let model = if model.trim().is_empty() { GROQ_MODEL } else { model };

    // MediaRecorder mime often includes a codec parameter ("audio/webm; codecs=opus").
    // reqwest's mime_str can be picky about that; collapse to the base type for the part.
    let base_mime = mime_type
        .split(';')
        .next()
        .unwrap_or("audio/webm")
        .trim()
        .to_string();
    let filename = guess_filename(&base_mime);
    let part = Part::bytes(audio_bytes)
        .file_name(filename.clone())
        .mime_str(&base_mime)
        .with_context(|| format!("invalid mime: {base_mime}"))?;

    let form = Form::new()
        .part("file", part)
        .text("model", model.to_string())
        .text("response_format", "json")
        .text("language", language.to_string())
        .text("temperature", "0");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let res = client
        .post(GROQ_TRANSCRIBE_URL)
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .await
        .context("Groq STT request failed")?;

    let status = res.status();
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(anyhow!("Groq STT HTTP {status}: {body}"));
    }

    let parsed: GroqResponse = res.json().await.context("Groq STT response parse")?;
    Ok(parsed.text.trim().to_string())
}

fn guess_filename(mime: &str) -> String {
    let ext = if mime.contains("webm") {
        "webm"
    } else if mime.contains("mp4") || mime.contains("m4a") {
        "m4a"
    } else if mime.contains("wav") {
        "wav"
    } else if mime.contains("ogg") {
        "ogg"
    } else {
        "webm"
    };
    format!("audio.{ext}")
}
