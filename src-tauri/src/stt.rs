use anyhow::{anyhow, Context, Result};
use once_cell::sync::Lazy;
use reqwest::multipart::{Form, Part};
use serde::Deserialize;

const GROQ_TRANSCRIBE_URL: &str = "https://api.groq.com/openai/v1/audio/transcriptions";
const GROQ_MODEL: &str = "whisper-large-v3-turbo";

// Shared client re-uses TLS sessions and connection pool across calls.
// Building a new Client per transcription added ~200-400ms TLS overhead.
// Falls back to a default Client (infallible) if the configured builder
// fails — avoids panicking the Tauri process on rare TLS/proxy init issues.
static HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
});

/// One segment of a `verbose_json` transcription, carrying the provider's own
/// confidence signals. Both fields are optional on purpose: a provider
/// response-shape change must never break parsing — missing fields decode as
/// `None` and the confidence gate in commands.rs fails open (accepts).
#[derive(Debug, Clone, Deserialize)]
pub struct TranscriptSegment {
    pub no_speech_prob: Option<f64>,
    pub avg_logprob: Option<f64>,
}

/// Parsed STT result: the transcript text plus per-segment confidence
/// (`segments` is empty when the response carries no segments array).
#[derive(Debug, Clone)]
pub struct Transcription {
    pub text: String,
    pub segments: Vec<TranscriptSegment>,
}

#[derive(Debug, Deserialize)]
struct GroqResponse {
    text: String,
    #[serde(default)]
    segments: Vec<TranscriptSegment>,
}

pub async fn transcribe_audio(
    api_key: &str,
    audio_bytes: Vec<u8>,
    mime_type: &str,
    language: &str,
    model: &str,
    vocab_hint: Option<&str>,
) -> Result<Transcription> {
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

    // Retry once on transient failures (shared policy in retry.rs): transport
    // errors and 5xx after a short backoff, 429 honouring Retry-After (capped
    // at 2s). multipart::Form is not Clone, so the form is rebuilt per
    // attempt — a small memcpy next to a network round trip.
    let mut attempt = 0u8;
    let res = loop {
        attempt += 1;
        let part = Part::bytes(audio_bytes.clone())
            .file_name(filename.clone())
            .mime_str(&base_mime)
            .with_context(|| format!("invalid mime: {base_mime}"))?;

        let mut form = Form::new()
            .part("file", part)
            .text("model", model.to_string())
            // verbose_json adds the segments array (per-segment no_speech_prob
            // and avg_logprob) that the confidence gate in commands.rs uses to
            // spot silence/noise hallucinations without a phrase denylist.
            .text("response_format", "verbose_json")
            .text("language", language.to_string())
            .text("temperature", "0");
        if let Some(hint) = vocab_hint {
            if !hint.is_empty() {
                form = form.text("prompt", hint.to_string());
            }
        }

        match HTTP_CLIENT
            .post(GROQ_TRANSCRIBE_URL)
            .bearer_auth(api_key)
            .multipart(form)
            .send()
            .await
        {
            Ok(res) => {
                let status = res.status();
                if status.is_success()
                    || attempt >= crate::retry::MAX_ATTEMPTS
                    || !crate::retry::should_retry(Some(status))
                {
                    break res;
                }
                // Read Retry-After before the body consumes the response; mask
                // key-shaped tokens in the provider-controlled body before it
                // hits the persistent log (same convention as llm.rs).
                let delay = crate::retry::retry_delay(Some(status), Some(res.headers()));
                let body = res.text().await.unwrap_or_default();
                log::warn!(
                    "Groq STT {status} on attempt {attempt}, retrying in {}ms: {}",
                    delay.as_millis(),
                    crate::redact::redact_secrets(&body)
                );
                tokio::time::sleep(delay).await;
            }
            Err(e) => {
                if attempt >= crate::retry::MAX_ATTEMPTS {
                    return Err(e).context("Groq STT request failed");
                }
                log::warn!("Groq STT transport error on attempt {attempt}, retrying: {e}");
                tokio::time::sleep(crate::retry::retry_delay(None, None)).await;
            }
        }
    };

    let status = res.status();
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(anyhow!("Groq STT HTTP {status}: {body}"));
    }

    let parsed: GroqResponse = res.json().await.context("Groq STT response parse")?;
    Ok(Transcription {
        text: parsed.text.trim().to_string(),
        segments: parsed.segments,
    })
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
