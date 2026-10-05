use crate::corrector;
use crate::settings::{Alternative, DictRule, Provider, Settings};
use regex::Regex;
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::LazyLock;
use std::time::Duration;

const EDGE_SENTENCES: usize = 2;
const CHAT_URL: &str = "https://api.openai.com/v1/chat/completions";
const QUESTION: &str = "`start` holds the first sentences and `end` the last sentences of a speech-to-text dictation. Is the speaker telling the dictation app itself to format, clean up or organise this very dictation (e.g. \"put this in markdown\", \"clean this up\"), as opposed to formatting being part of what they are dictating (e.g. asking a colleague to format a report or a date)?";

const DESCRIBE_PROMPT: &str = "You write entries for a speech-to-text correction dictionary. A classifier reads each entry to decide, from the surrounding sentence, whether a transcribed word should be replaced by the correct word. You get the correct word, the words speech recognition mishears it as (may be empty) and the user's notes (may be empty; they say which sense of the word is meant). Reply with a JSON object: \"meaning\": one line saying what the correct word refers to and how a speaker would use it, e.g. \"Anthropic's AI assistant, which the speaker asks, tells or uses to write, fix or analyze things\"; \"triggers\": when no misheard words were given, the 2-4 words or spellings speech recognition most likely writes instead (think about how the word sounds aloud, including how people actually pronounce abbreviations), each different from the correct word ignoring case, otherwise exactly the given ones; \"alternatives\": for each trigger that is a real word or name with its own legitimate meaning, {\"word\", \"meaning\"} describing those meanings in one line; \"context\": one or two sentences contrasting the correct word with the alternatives so they can be told apart.";

#[derive(Debug, Deserialize)]
pub struct Description {
    #[serde(default)]
    triggers: Vec<String>,
    meaning: String,
    #[serde(default)]
    alternatives: Vec<Alternative>,
    #[serde(default)]
    context: String,
}

impl Description {
    pub fn apply(self, rule: &mut DictRule) {
        let same = |a: &str, b: &str| a.trim().eq_ignore_ascii_case(b.trim());
        if rule.triggers.trim().is_empty() {
            let mut triggers: Vec<String> = Vec::new();
            for t in self.triggers.iter().map(|t| t.trim().to_lowercase()).filter(|t| !same(t, &rule.word)) {
                if !triggers.contains(&t) {
                    triggers.push(t);
                }
            }
            rule.triggers = triggers.join(", ");
        }
        let triggers = rule.trigger_list();
        rule.meaning = self.meaning;
        rule.alternatives = self.alternatives.into_iter().filter(|a| triggers.iter().any(|t| same(t, &a.word))).collect();
        rule.context = self.context;
    }
}

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

fn chat(http: &Client, key: &str, model: &str, system: &str, user: &str, schema: Option<Value>) -> Result<String, String> {
    let mut body = json!({"model": model, "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}]});
    if let Some(schema) = schema {
        body["response_format"] = json!({"type": "json_schema", "json_schema": {"name": "answer", "strict": true, "schema": schema}});
    }
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
        let answer = chat(http, &openai, &s.format_model, "Answer with exactly one word: yes or no.", &format!("{QUESTION}\n\nstart: {start}\nend: {end}"), None)?;
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
    chat(http, &key, &s.format_model, prompt, text, None)
}

pub fn describe(rule: &DictRule, key: &str, model: &str, http: &Client) -> Result<Description, String> {
    if key.is_empty() {
        return Err("Generating needs an OpenAI API key (Settings → Transcription, or $OPENAI_API_KEY)".into());
    }
    let user = json!({"word": rule.word.trim(), "misheard_as": rule.trigger_list(), "notes": rule.meaning.trim()}).to_string();
    let object = |props: Value| json!({"type": "object", "additionalProperties": false, "required": props.as_object().unwrap().keys().collect::<Vec<_>>(), "properties": props});
    let text = json!({"type": "string"});
    let alternative = object(json!({"word": text, "meaning": text}));
    let schema = object(json!({"meaning": text, "triggers": {"type": "array", "items": text}, "alternatives": {"type": "array", "items": alternative}, "context": text}));
    let answer = chat(http, key, model, DESCRIBE_PROMPT, &user, Some(schema))?;
    serde_json::from_str(&answer).map_err(|e| format!("OpenAI returned an unexpected answer: {e}"))
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

    #[test]
    #[ignore = "calls OpenAI with the saved settings"]
    fn live_describe() {
        let s = Settings::load();
        let mut rule = DictRule { word: "env".into(), ..DictRule::default() };
        describe(&rule, &s.api_key(Provider::OpenAI), &s.format_model, &Client::new()).unwrap().apply(&mut rule);
        println!("{rule:#?}");
        assert!(!rule.meaning.is_empty() && !rule.triggers.is_empty() && !rule.context.is_empty());
    }
}
