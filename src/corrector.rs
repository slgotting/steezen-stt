use crate::settings::DictRule;
use regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

const MIN_CONTEXT_WORDS: usize = 12;
const PHRASE_WORDS: usize = 2;
const MARK: (&str, &str) = ("⟦", "⟧");
const ORDINALS: [&str; 10] = ["first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth", "tenth"];
const JEV_URL: &str = "https://api.typesafe.ai/v1/systemone";

static SENTENCE_END: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[.!?]\s+").unwrap());

pub struct Candidate<'a> {
    rule: &'a DictRule,
    start: usize,
    end: usize,
    word: String,
    context: String,
    plain: String,
    occurrence: String,
    choice: Option<String>,
    probability: f64,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Correction {
    pub from: String,
    pub to: Option<String>,
    pub choice: String,
    pub probability: f64,
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '\'' | '-')
}

fn find_candidates<'a>(text: &str, rules: &'a [DictRule]) -> Vec<Candidate<'a>> {
    let mut by_trigger: HashMap<String, &DictRule> = HashMap::new();
    for rule in rules.iter().filter(|r| r.enabled && !r.word.trim().is_empty()) {
        for t in rule.trigger_list() {
            by_trigger.entry(t.to_lowercase()).or_insert(rule);
        }
    }
    if by_trigger.is_empty() {
        return Vec::new();
    }
    let mut triggers: Vec<&String> = by_trigger.keys().collect();
    triggers.sort_by_key(|t| std::cmp::Reverse(t.len()));
    let alternation = triggers.iter().map(|t| regex::escape(t)).collect::<Vec<_>>().join("|");
    let matcher = Regex::new(&format!("(?i)(?:{alternation})")).unwrap();
    let spans = sentence_spans(text);
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(m) = matcher.find_at(text, pos) {
        let before_ok = text[..m.start()].chars().next_back().is_none_or(|c| !is_word_char(c));
        let after_ok = text[m.end()..].chars().next().is_none_or(|c| !is_word_char(c));
        if before_ok && after_ok {
            let (context, plain, occurrence) = extract_context(text, &spans, m.start(), m.end());
            out.push(Candidate {
                rule: by_trigger[&m.as_str().to_lowercase()],
                start: m.start(),
                end: m.end(),
                word: m.as_str().to_string(),
                context,
                plain,
                occurrence,
                choice: None,
                probability: 0.0,
            });
            pos = m.end();
        } else {
            pos = m.start() + text[m.start()..].chars().next().map_or(1, char::len_utf8);
        }
    }
    out
}

fn sentence_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut pos = 0;
    for m in SENTENCE_END.find_iter(text) {
        spans.push((pos, m.start() + 1));
        pos = m.end();
    }
    spans.push((pos, text.len()));
    spans
}

fn count_word(haystack: &str, word: &str) -> usize {
    let lower = haystack.to_lowercase();
    let needle = word.to_lowercase();
    lower
        .match_indices(&needle)
        .filter(|(i, _)| {
            lower[..*i].chars().next_back().is_none_or(|c| !is_word_char(c))
                && lower[i + needle.len()..].chars().next().is_none_or(|c| !is_word_char(c))
        })
        .count()
}

fn extract_context(text: &str, spans: &[(usize, usize)], start: usize, end: usize) -> (String, String, String) {
    let i = spans.iter().position(|&(s, e)| s <= start && start <= e).unwrap_or(0);
    let (mut lo, mut hi) = (i.saturating_sub(1), (i + 1).min(spans.len() - 1));
    while text[spans[lo].0..spans[hi].1].split_whitespace().count() < MIN_CONTEXT_WORDS && (lo > 0 || hi < spans.len() - 1) {
        lo = lo.saturating_sub(1);
        hi = (hi + 1).min(spans.len() - 1);
    }
    let (s, e) = (spans[lo].0, spans[hi].1);
    let (before, word, after) = (&text[s..start], &text[start..end], &text[end..e]);
    let before_words: Vec<&str> = before.split_whitespace().collect();
    let phrase = before_words[before_words.len().saturating_sub(PHRASE_WORDS)..]
        .iter()
        .copied()
        .chain([word])
        .chain(after.split_whitespace().take(PHRASE_WORDS))
        .collect::<Vec<_>>()
        .join(" ");
    let (total, index) = (count_word(&text[s..e], word), count_word(before, word));
    let ordinal = if total > 1 {
        format!("the {} ", ORDINALS.get(index).map_or(format!("#{}", index + 1), |o| o.to_string()))
    } else {
        String::new()
    };
    (
        format!("{before}{}{word}{}{after}", MARK.0, MARK.1),
        text[s..e].to_string(),
        format!("{ordinal}'{word}', in '{phrase}'"),
    )
}

fn options(c: &Candidate) -> Map<String, Value> {
    let rule = c.rule;
    let mut opts = Map::new();
    let described = |m: &str| if m.trim().is_empty() { Value::Null } else { Value::String(m.trim().to_string()) };
    opts.insert(rule.word.trim().to_string(), described(&rule.meaning));
    for alt in rule.alternatives.iter().filter(|a| !a.word.trim().is_empty()) {
        opts.insert(alt.word.trim().to_string(), described(&alt.meaning));
    }
    if !opts.keys().any(|k| k.to_lowercase() == c.word.to_lowercase()) {
        opts.insert(c.word.to_lowercase(), Value::String(format!("\"{}\" exactly as transcribed is correct", c.word)));
    }
    opts
}

fn cache_key(c: &Candidate) -> String {
    let payload = json!([c.rule.word, c.rule.context, options(c), c.context]).to_string();
    let hash = payload.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    format!("{hash:016x}")
}

pub struct Cache {
    path: PathBuf,
    data: Mutex<HashMap<String, (String, f64)>>,
}

impl Cache {
    pub fn load(path: PathBuf) -> Self {
        let data = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Self { path, data: Mutex::new(data) }
    }

    fn save(&self) {
        if let Ok(s) = serde_json::to_vec(&*self.data.lock().unwrap()) {
            let _ = crate::settings::write_private(&self.path, &s);
        }
    }
}

pub const KEY_URL: &str = "https://console.typesafe.ai/keys";

fn post(http: &reqwest::blocking::Client, key: &str, body: &Value) -> Result<Value, String> {
    let mut delay = std::time::Duration::from_millis(250);
    for attempt in 0..3 {
        let resp = http
            .post(JEV_URL)
            .bearer_auth(key)
            .json(body)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .map_err(|e| format!("Jev request failed: {e}"))?;
        match resp.status().as_u16() {
            200..=299 => return resp.json().map_err(|e| format!("Jev returned an unreadable response: {e}")),
            401 => return Err("Jev rejected the API key (401). Check the key in Settings → Dictionary.".into()),
            429 | 529 if attempt < 2 => {
                std::thread::sleep(delay);
                delay *= 2;
            }
            code => return Err(format!("Jev error {code}: {}", resp.text().unwrap_or_default().chars().take(200).collect::<String>())),
        }
    }
    Err("Jev is busy; try again shortly".into())
}

pub fn validate_key(key: &str, model: &str, http: &reqwest::blocking::Client) -> Result<(), String> {
    if key.trim().is_empty() {
        return Err("Enter a key first".into());
    }
    post(http, key.trim(), &json!({"model": model, "state": "test", "questions": {"q": {"type": "noul", "instructions": "Is this a test?"}}})).map(|_| ())
}

pub fn trigger_words(text: &str, rules: &[DictRule]) -> Vec<String> {
    find_candidates(text, rules).into_iter().map(|c| c.word).collect()
}

fn classify(candidates: &[&mut Candidate], key: &str, model: &str, http: &reqwest::blocking::Client) -> Result<Vec<(String, f64)>, String> {
    if key.is_empty() {
        return Err(format!("no Jev API key set; get one at {KEY_URL}"));
    }
    let state: Map<String, Value> = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| (format!("c{i}"), json!({"text": c.plain, "occurrence": c.occurrence})))
        .collect();
    let questions: Map<String, Value> = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| {
            (format!("c{i}"), json!({
                "type": "choice",
                "instructions": {
                    "background": c.rule.context,
                    "question": format!("`c{i}.text` is a speech-to-text transcript. Consider only the occurrence described by `c{i}.occurrence`, which may be a transcription error. Which word did the speaker most likely intend there?"),
                },
                "criteria": options(c),
            }))
        })
        .collect();
    let resp = post(http, key, &json!({"model": model, "state": state, "questions": questions}))?;
    (0..candidates.len())
        .map(|i| {
            let a = &resp["answers"][format!("c{i}")];
            let choice = a["choice"].as_str().ok_or("JEV response missing a choice")?.to_string();
            let p = a["probabilities"][&choice].as_f64().unwrap_or(0.0);
            Ok((choice, p))
        })
        .collect()
}

fn match_case(original: &str, replacement: &str) -> String {
    if original.chars().count() > 1 && original.chars().all(|c| !c.is_lowercase()) && original.chars().any(char::is_uppercase) {
        return replacement.to_uppercase();
    }
    let (o, r) = (original.chars().next(), replacement.chars().next());
    match (o, r) {
        (Some(o), Some(r)) if o.is_uppercase() && r.is_lowercase() => r.to_uppercase().chain(replacement.chars().skip(1)).collect(),
        _ => replacement.to_string(),
    }
}

fn replacement(c: &Candidate) -> Option<String> {
    let choice = c.choice.as_ref()?;
    if c.probability < c.rule.threshold as f64 {
        return None;
    }
    let new = match_case(&c.word, choice);
    (new != c.word).then_some(new)
}

pub fn correct(text: &str, rules: &[DictRule], key: &str, model: &str, cache: &Cache, http: &reqwest::blocking::Client) -> Result<(String, Vec<Correction>), String> {
    let mut candidates = find_candidates(text, rules);
    if candidates.is_empty() {
        return Ok((text.to_string(), Vec::new()));
    }
    let mut pending = Vec::new();
    {
        let data = cache.data.lock().unwrap();
        for c in candidates.iter_mut() {
            match data.get(&cache_key(c)) {
                Some((choice, p)) => (c.choice, c.probability) = (Some(choice.clone()), *p),
                None => pending.push(c),
            }
        }
    }
    if !pending.is_empty() {
        let results = classify(&pending, key, model, http)?;
        let mut data = cache.data.lock().unwrap();
        for (c, (choice, p)) in pending.into_iter().zip(results) {
            data.insert(cache_key(c), (choice.clone(), p));
            (c.choice, c.probability) = (Some(choice), p);
        }
        drop(data);
        cache.save();
    }
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    for c in &candidates {
        if let Some(new) = replacement(c) {
            out.push_str(&text[pos..c.start]);
            out.push_str(&new);
            pos = c.end;
        }
    }
    out.push_str(&text[pos..]);
    let corrections = candidates
        .iter()
        .map(|c| Correction { from: c.word.clone(), to: replacement(c), choice: c.choice.clone().unwrap_or_default(), probability: c.probability })
        .collect();
    Ok((out, corrections))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Vec<DictRule> {
        crate::settings::Settings::default().dictionary
    }

    fn stub(text: &str, answer: &dyn Fn(&Candidate) -> (String, f64)) -> String {
        let rules = rules();
        let mut cands = find_candidates(text, &rules);
        for c in cands.iter_mut() {
            (c.choice, c.probability) = { let (a, p) = answer(c); (Some(a), p) };
        }
        let mut out = String::new();
        let mut pos = 0;
        for c in &cands {
            if let Some(new) = replacement(c) {
                out.push_str(&text[pos..c.start]);
                out.push_str(&new);
                pos = c.end;
            }
        }
        out + &text[pos..]
    }

    #[test]
    fn word_boundaries() {
        assert!(find_candidates("Cloudy skies, cloudflare, modeling.", &rules()).is_empty());
    }

    #[test]
    fn independent_occurrences() {
        let out = stub("I uploaded it to the cloud and then asked cloud to analyze it.", &|c| {
            if c.context.contains("⟦cloud⟧ to analyze") { ("Claude".into(), 0.9) } else { ("cloud".into(), 0.9) }
        });
        assert_eq!(out, "I uploaded it to the cloud and then asked Claude to analyze it.");
    }

    #[test]
    fn threshold_and_case() {
        assert_eq!(stub("Cloud fixed it.", &|_| ("Claude".into(), 0.9)), "Claude fixed it.");
        assert_eq!(stub("cloud fixed it.", &|_| ("Claude".into(), 0.3)), "cloud fixed it.");
        assert_eq!(stub("Open a JASON file.", &|_| ("JSON".into(), 0.9)), "Open a JSON file.");
    }

    #[test]
    fn multi_word_trigger_and_ordinal() {
        let rules = rules();
        let c = find_candidates("I use pie torch daily. Cloud and cloud.", &rules);
        assert_eq!(c[0].word, "pie torch");
        assert!(c[2].occurrence.starts_with("the second"));
    }
}
