use crate::corrector;
use crate::settings::{Provider, Settings};
use regex::Regex;
use reqwest::blocking::Client;
use serde_json::{Value, json};
use std::sync::LazyLock;
use std::time::Duration;

const EDGE_SENTENCES: usize = 2;
const CHAT_URL: &str = "https://api.openai.com/v1/chat/completions";
const QUESTION: &str = "`start` holds the first sentences and `end` the last sentences of a speech-to-text dictation. Is the speaker telling the dictation app itself to format, clean up or organise this very dictation (e.g. \"put this in markdown\", \"clean this up\"), as opposed to formatting being part of what they are dictating (e.g. asking a colleague to format a report or a date)?";

pub const DEFAULT_PROMPT: &str = "You clean up dictated speech-to-text transcripts. Rewrite the transcript as well-organised Markdown: a short heading wherever the topic changes, bullet or numbered lists for lists and steps, paragraphs for prose. Keep the speaker's meaning, wording and every detail. Remove filler words, false starts and repetition; fix punctuation and obvious transcription errors. If the speaker gives instructions about how to format or clean up this text, follow them and leave the instructions themselves out. Output only the result, with no preamble and no code fences around it.";

static ASKED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(format|markdown|mark down|clean|tidy|bullet|organi[sz]e|structure|rewrite|polish|pretty|neat|heading|document)").unwrap()
});

fn edges(text: &str) -> (String, String) {
    let spans = corrector::sentence_spans(text);
    if spans.len() <= 2 * EDGE_SENTENCES {
        return (text.trim().to_string(), String::new());
    }
    let join = |part: &[(usize, usize)]| part.iter().map(|&(s, e)| text[s..e].trim()).collect::<Vec<_>>().join(" ");
    (join(&spans[..EDGE_SENTENCES]), join(&spans[spans.len() - EDGE_SENTENCES..]))
}

fn chat(http: &Client, key: &str, model: &str, system: &str, user: &str) -> Result<String, String> {
    let body = json!({"model": model, "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}]});
    let resp = http.post(CHAT_URL).bearer_auth(key).json(&body).timeout(Duration::from_secs(90)).send().map_err(|e| format!("OpenAI request failed: {e}"))?;
    let status = resp.status();
    let body: Value = resp.json().map_err(|e| format!("OpenAI returned an unreadable response: {e}"))?;
    if !status.is_success() {
        return Err(format!("OpenAI error {}: {}", status.as_u16(), body["error"]["message"].as_str().unwrap_or_default()));
    }
    body["choices"][0]["message"]["content"].as_str().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).ok_or_else(|| "OpenAI returned no text".into())
}

pub fn wanted(text: &str, force: bool, s: &Settings, http: &Client) -> Result<(bool, String), String> {
    if force {
        return Ok((true, "You used the format hotkey".into()));
    }
    if s.format_always {
        return Ok((true, "\"Format every transcript\" is on".into()));
    }
    if !s.format_voice {
        return Ok((false, String::new()));
    }
    let (start, end) = edges(text);
    if !ASKED.is_match(&start) && !ASKED.is_match(&end) {
        return Ok((false, String::new()));
    }
    let openai = s.api_key(Provider::OpenAI);
    if openai.is_empty() {
        return Ok((false, "You may have asked for formatting, but there is no OpenAI API key to format with".into()));
    }
    let jev = s.typesafe_key();
    if jev.is_empty() {
        let answer = chat(http, &openai, &s.format_model, "Answer with exactly one word: yes or no.", &format!("{QUESTION}\n\nstart: {start}\nend: {end}"))?;
        let yes = answer.to_lowercase().starts_with("yes");
        return Ok((yes, format!("{} answered {}", s.format_model, if yes { "yes" } else { "no" })));
    }
    let question = json!({"q": {"type": "choice", "instructions": QUESTION, "criteria": {
        "yes": "An instruction to the dictation app to format or clean up this dictation",
        "no": "No such instruction; any mention of formatting is part of the dictated content",
    }}});
    let resp = corrector::post(http, &jev, &json!({"model": s.corrector_model, "state": {"start": start, "end": end}, "questions": question}))?;
    let p = resp["answers"]["q"]["probabilities"]["yes"].as_f64().unwrap_or(0.0);
    let yes = resp["answers"]["q"]["choice"].as_str() == Some("yes");
    Ok((yes, format!("Jev: {:.0}% that you asked for formatting", p * 100.0)))
}

pub fn format(text: &str, s: &Settings, http: &Client) -> Result<String, String> {
    let key = s.api_key(Provider::OpenAI);
    if key.is_empty() {
        return Err("Formatting needs an OpenAI API key (Settings → Transcription, or $OPENAI_API_KEY)".into());
    }
    let prompt = if s.format_prompt.trim().is_empty() { DEFAULT_PROMPT } else { s.format_prompt.as_str() };
    chat(http, &key, &s.format_model, prompt, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_take_first_and_last_sentences() {
        let text = "One. Two. Three. Four. Five. Please format this as markdown.";
        assert_eq!(edges(text), ("One. Two.".to_string(), "Five. Please format this as markdown.".to_string()));
        assert_eq!(edges("Short. Text."), ("Short. Text.".to_string(), String::new()));
    }

    #[test]
    fn request_in_the_middle_is_not_noticed() {
        let (start, end) = edges("One. Two. Format this nicely. Four. Five.");
        assert!(!ASKED.is_match(&format!("{start} {end}")));
    }

    #[test]
    #[ignore = "calls OpenAI (and Jev when $TYPESAFE_API_KEY is set) with the saved settings"]
    fn live_classify_and_format() {
        let s = Settings::load();
        let http = Client::new();
        let ramble = "So I went to the store and bought eggs. Then the car broke down on the way home. I called a tow truck. It took an hour. Anyway the eggs survived. Can you clean this up and put it in markdown for me?";
        let plain = "So I went to the store and bought eggs. Then the car broke down. I called a tow truck. It took an hour. Please format the date in the report differently. Anyway the eggs survived.";
        let (yes, why) = wanted(ramble, false, &s, &http).unwrap();
        println!("ramble: {yes} {why}");
        assert!(yes);
        let (no, why) = wanted(plain, false, &s, &http).unwrap();
        println!("plain: {no} {why}");
        assert!(!no);
        let out = format(ramble, &s, &http).unwrap();
        println!("{out}");
        assert!(!out.to_lowercase().contains("markdown for me"));
    }
}
