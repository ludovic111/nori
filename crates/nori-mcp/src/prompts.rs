//! MCP prompts: one per harness skill (`harness.skills`), so a person can start a job from
//! their MCP client's prompt list and the agent gets the playbook with it.

use nori_control::harness;
use serde_json::{Value, json};

/// A string argument, or `default` when it is missing or blank.
pub fn arg<'a>(arguments: &'a Value, key: &str, default: &'a str) -> &'a str {
    arguments.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).unwrap_or(default)
}

/// Argument names earlier versions of the prompts took, still read as the request.
const OLD_ARGUMENTS: &[&str] = &["brief", "look", "photo", "text", "idea", "size", "pages"];

/// `prompts/list`.
pub fn list() -> Vec<Value> {
    harness::skills()
        .iter()
        .map(|k| {
            json!({
                "name": k.name,
                "title": k.title,
                "description": format!("{}. {}", k.title, k.when),
                "arguments": [{ "name": "request", "description": "What to make or change, in your words (sizes, text, files). Omit to be asked.", "required": false }],
            })
        })
        .collect()
}

/// `prompts/get`: the skill's recipe, then the person's request and the finish routine.
pub fn get(name: &str, arguments: &Value) -> Result<Value, String> {
    let k = harness::skill(name)?;
    let mut request: Vec<String> = vec![];
    if let Some(r) = Some(arg(arguments, "request", "")).filter(|r| !r.is_empty()) {
        request.push(r.to_string());
    }
    for key in OLD_ARGUMENTS {
        if let Some(v) = Some(arg(arguments, key, "")).filter(|v| !v.is_empty()) {
            request.push(format!("{key}: {v}"));
        }
    }
    let job = if request.is_empty() { "Ask me what I want before you start.".to_string() } else { format!("The job: {}", request.join("; ")) };
    let text = format!(
        "Use nori's skill \"{}\" for this.\n\n{}\n\n{job}\n\nStart with doc_overview (or harness_context). Before you say it's done: harness_look every page you changed, compare with the job point by point, fix what's off (up to three passes), then tell me in a few lines what you made and which files you wrote.",
        k.name, k.markdown
    );
    Ok(json!({
        "description": format!("{}. {}", k.title, k.when),
        "messages": [{ "role": "user", "content": { "type": "text", "text": text } }],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_skill_is_a_prompt() {
        let list = list();
        assert_eq!(list.len(), harness::skills().len());
        assert!(list.iter().any(|p| p["name"] == "poster"));
        let got = get("poster", &json!({ "request": "jazz night, Friday 8 pm" })).unwrap();
        let text = got["messages"][0]["content"]["text"].as_str().unwrap();
        assert!(text.contains("jazz night") && text.contains("## Steps") && text.contains("harness_look"), "{text}");
        // What the earlier prompts took still reaches the agent.
        let old = get("retouch-photo", &json!({ "look": "moody black and white" })).unwrap();
        assert!(old["messages"][0]["content"]["text"].as_str().unwrap().contains("moody black and white"));
        assert!(get("nope", &json!({})).is_err());
    }
}
