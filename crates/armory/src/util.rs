use serde_json::Value as JsonValue;

pub(crate) fn json_to_string(v: JsonValue) -> String {
    match v {
        JsonValue::Null => String::new(),
        JsonValue::String(s) => s,
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::Array(a) => serde_json::to_string(&a).unwrap_or_default(),
        JsonValue::Object(o) => serde_json::to_string(&o).unwrap_or_default(),
    }
}

pub(crate) fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_dash = false;

    for ch in input.chars() {
        let lc = ch.to_ascii_lowercase();
        if lc.is_ascii_alphanumeric() {
            out.push(lc);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }

    let out = out.trim_matches('-');
    if out.is_empty() {
        "unknown-ttp".to_string()
    } else {
        out.to_string()
    }
}
