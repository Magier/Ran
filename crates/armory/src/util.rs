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

/// Convert an effect identifier into the canonical stem used for parser scripts.
pub fn canonical_parser_stem(effect_id: &str) -> Option<String> {
    let stem = effect_id
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .to_ascii_lowercase();

    (!stem.is_empty() && !stem.starts_with('.')).then_some(stem)
}

#[cfg(test)]
mod tests {
    use super::canonical_parser_stem;

    #[test]
    fn canonical_parser_stem_normalizes_effect_ids() {
        assert_eq!(
            canonical_parser_stem("  K8s/List Pods:v1  ").as_deref(),
            Some("k8s_list_pods_v1")
        );
    }

    #[test]
    fn canonical_parser_stem_rejects_unsafe_or_empty_stems() {
        assert_eq!(canonical_parser_stem("  "), None);
        assert_eq!(canonical_parser_stem("../parser"), None);
    }
}
