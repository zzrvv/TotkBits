use regex::Regex;

pub fn update_json(mut base: serde_json::Value, update: serde_json::Value) -> serde_json::Value {
    if let (Some(base), Some(update)) = (base.as_object_mut(), update.as_object()) {
        for (key, value) in update {
            base.insert(key.clone(), value.clone());
        }
    }
    base
}

pub fn process_inline_content(mut input: String, inline_count: usize) -> String {
    let Ok(regex) = Regex::new(r"(\s*)\{(.*?)\}") else {
        return input;
    };
    input = regex
        .replace_all(&input, |captures: &regex::Captures| {
            let Some(indentation) = captures.get(1) else {
                return String::new();
            };
            let Some(content) = captures.get(2) else {
                return String::new();
            };
            let indent = indentation.as_str();
            let content = content.as_str();
            let items: Vec<_> = content.split(',').collect();
            if items.len() <= inline_count {
                return format!("{indent}{{{content}}}");
            }
            items
                .into_iter()
                .filter_map(|item| item.split_once(':'))
                .map(|(key, value)| format!("{indent}{}: {}", key.trim(), value.trim()))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .into_owned();
    input
}
