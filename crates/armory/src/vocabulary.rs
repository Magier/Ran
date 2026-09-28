use crate::Ttp;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;
use thiserror::Error;

const VOCABULARY_JSON: &str = include_str!("../../../armory/vocabulary.json");

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArmoryVocabulary {
    pub schema_version: u32,
    pub title: String,
    pub stability: String,
    pub extension_policy: String,
    pub interpolation: InterpolationDefinition,
    pub support_levels: Vec<SupportLevel>,
    pub requirements: Vec<RequirementDefinition>,
    pub effects: Vec<EffectDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterpolationDefinition {
    pub syntax: String,
    pub applies_to: Vec<String>,
    pub unknown_variables: String,
    pub description: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SupportLevel {
    pub id: String,
    pub description: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequirementDefinition {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub value_types: Vec<JsonValueType>,
    pub support: String,
    pub matching: String,
    pub description: String,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum JsonValueType {
    Boolean,
    String,
    Array,
    Object,
    Number,
    Null,
}

impl JsonValueType {
    fn matches(self, value: &Value) -> bool {
        matches!(
            (self, value),
            (Self::Boolean, Value::Bool(_))
                | (Self::String, Value::String(_))
                | (Self::Array, Value::Array(_))
                | (Self::Object, Value::Object(_))
                | (Self::Number, Value::Number(_))
                | (Self::Null, Value::Null)
        )
    }

    fn label(self) -> &'static str {
        match self {
            Self::Boolean => "boolean",
            Self::String => "string",
            Self::Array => "array",
            Self::Object => "object",
            Self::Number => "number",
            Self::Null => "null",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectDefinition {
    pub kind: String,
    pub syntax: String,
    pub support: String,
    pub processing: String,
    pub description: String,
}

#[derive(Debug, Error)]
pub enum VocabularyError {
    #[error("bundled Armory vocabulary is invalid JSON: {0}")]
    InvalidDocument(#[from] serde_json::Error),
    #[error("TTP '{ttp_id}' uses unknown requirement '{requirement}'")]
    UnknownRequirement { ttp_id: String, requirement: String },
    #[error(
        "TTP '{ttp_id}' requirement '{requirement}' has type {actual}; expected one of {expected}"
    )]
    InvalidRequirementType {
        ttp_id: String,
        requirement: String,
        actual: &'static str,
        expected: String,
    },
    #[error("TTP '{ttp_id}' uses unknown effect kind '{effect_kind}' in '{effect}'")]
    UnknownEffect {
        ttp_id: String,
        effect_kind: String,
        effect: String,
    },
    #[error("Armory vocabulary contains duplicate requirement '{0}'")]
    DuplicateRequirement(String),
    #[error("Armory vocabulary contains duplicate effect kind '{0}'")]
    DuplicateEffect(String),
    #[error("Armory vocabulary entry '{entry}' uses unknown support level '{support}'")]
    UnknownSupport { entry: String, support: String },
}

pub fn bundled_vocabulary() -> Result<ArmoryVocabulary, VocabularyError> {
    let vocabulary: ArmoryVocabulary = serde_json::from_str(VOCABULARY_JSON)?;
    validate_document(&vocabulary)?;
    Ok(vocabulary)
}

fn validate_document(vocabulary: &ArmoryVocabulary) -> Result<(), VocabularyError> {
    let support_levels: HashSet<&str> = vocabulary
        .support_levels
        .iter()
        .map(|level| level.id.as_str())
        .collect();
    let mut requirement_names = HashSet::new();
    for requirement in &vocabulary.requirements {
        for name in std::iter::once(&requirement.name).chain(&requirement.aliases) {
            if !requirement_names.insert(name.to_ascii_lowercase()) {
                return Err(VocabularyError::DuplicateRequirement(name.clone()));
            }
        }
        if !support_levels.contains(requirement.support.as_str()) {
            return Err(VocabularyError::UnknownSupport {
                entry: requirement.name.clone(),
                support: requirement.support.clone(),
            });
        }
    }

    let mut effect_kinds = HashSet::new();
    for effect in &vocabulary.effects {
        if !effect_kinds.insert(effect.kind.to_ascii_lowercase()) {
            return Err(VocabularyError::DuplicateEffect(effect.kind.clone()));
        }
        if !support_levels.contains(effect.support.as_str()) {
            return Err(VocabularyError::UnknownSupport {
                entry: effect.kind.clone(),
                support: effect.support.clone(),
            });
        }
    }
    Ok(())
}

pub fn validate_ttp_vocabulary(
    ttp: &Ttp,
    vocabulary: &ArmoryVocabulary,
) -> Result<(), VocabularyError> {
    for (name, value) in &ttp.requires {
        let definition = vocabulary.requirements.iter().find(|requirement| {
            requirement.name.eq_ignore_ascii_case(name)
                || requirement
                    .aliases
                    .iter()
                    .any(|alias| alias.eq_ignore_ascii_case(name))
        });
        let Some(definition) = definition else {
            return Err(VocabularyError::UnknownRequirement {
                ttp_id: ttp.id.clone(),
                requirement: name.clone(),
            });
        };
        if !definition
            .value_types
            .iter()
            .any(|value_type| value_type.matches(value))
        {
            return Err(VocabularyError::InvalidRequirementType {
                ttp_id: ttp.id.clone(),
                requirement: name.clone(),
                actual: json_type(value),
                expected: definition
                    .value_types
                    .iter()
                    .map(|value_type| value_type.label())
                    .collect::<Vec<_>>()
                    .join(", "),
            });
        }
    }

    for effect in &ttp.effects {
        let kind = effect_kind(effect);
        if !vocabulary
            .effects
            .iter()
            .any(|definition| definition.kind.eq_ignore_ascii_case(kind))
        {
            return Err(VocabularyError::UnknownEffect {
                ttp_id: ttp.id.clone(),
                effect_kind: kind.to_string(),
                effect: effect.clone(),
            });
        }
    }
    Ok(())
}

fn effect_kind(effect: &str) -> &str {
    effect
        .split_once('(')
        .map(|(kind, _)| kind)
        .unwrap_or(effect)
        .trim()
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

pub fn render_vocabulary_markdown(vocabulary: &ArmoryVocabulary) -> String {
    let mut output = String::new();
    output.push_str("<!-- Generated from armory/vocabulary.json. Do not edit directly. -->\n\n");
    output.push_str(&format!("# {}\n\n", vocabulary.title));
    output.push_str(
        "The machine-readable source is `armory/vocabulary.json`. Regenerate this page with `cargo run -p armory --bin generate-vocabulary-docs`.\n\n",
    );
    output.push_str(&format!(
        "Vocabulary schema version: `{}`. Stability: **{}**.\n\n",
        vocabulary.schema_version, vocabulary.stability
    ));
    output.push_str(&format!("{}\n\n", vocabulary.extension_policy));
    output.push_str("## Interpolation\n\n");
    output.push_str(&format!("{}\n\n", vocabulary.interpolation.description));
    output.push_str(&format!(
        "Syntax: `{}`. Applies to: {}. Unknown variables: {}.\n\n",
        vocabulary.interpolation.syntax,
        vocabulary.interpolation.applies_to.join(", "),
        vocabulary.interpolation.unknown_variables
    ));

    output.push_str("## Support levels\n\n");
    output.push_str("| Level | Meaning |\n| --- | --- |\n");
    for level in &vocabulary.support_levels {
        output.push_str(&format!(
            "| `{}` | {} |\n",
            level.id,
            table_text(&level.description)
        ));
    }

    output.push_str("\n## Requirements\n\n");
    output.push_str("All requirement predicates must pass for a TTP to be applicable. Entries marked `declarative-only` are preserved in the API but do not currently gate applicability.\n\n");
    output.push_str("| Name | Accepted value types | Support | Matching semantics |\n| --- | --- | --- | --- |\n");
    for requirement in &vocabulary.requirements {
        let aliases = if requirement.aliases.is_empty() {
            String::new()
        } else {
            format!(
                " (YAML alias: {})",
                requirement
                    .aliases
                    .iter()
                    .map(|alias| format!("`{alias}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        output.push_str(&format!(
            "| `{}`{} | {} | `{}` | {} {} |\n",
            requirement.name,
            aliases,
            requirement
                .value_types
                .iter()
                .map(|value_type| format!("`{}`", value_type.label()))
                .collect::<Vec<_>>()
                .join(", "),
            requirement.support,
            table_text(&requirement.description),
            table_text(&requirement.matching)
        ));
    }

    output.push_str("\n## Effects\n\n");
    output.push_str("Effect kinds are the part before the first `(`. Effect matching is ASCII case-insensitive. Every effect string is interpolated before processing.\n\n");
    output.push_str(
        "| Kind | Syntax | Support | Processing | Meaning |\n| --- | --- | --- | --- | --- |\n",
    );
    for effect in &vocabulary.effects {
        output.push_str(&format!(
            "| `{}` | `{}` | `{}` | {} | {} |\n",
            effect.kind,
            effect.syntax.replace('|', "\\|"),
            effect.support,
            table_text(&effect.processing),
            table_text(&effect.description)
        ));
    }
    output
}

fn table_text(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Armory;
    use std::path::PathBuf;

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|path| path.parent())
            .expect("armory crate should be under <workspace>/crates")
            .to_path_buf()
    }

    #[test]
    fn bundled_ttps_are_covered_by_the_published_vocabulary() {
        let vocabulary = bundled_vocabulary().expect("vocabulary must be valid");
        let armory = Armory::load_from_dir(workspace_root().join("armory/TTPs"))
            .expect("bundled TTPs must load");
        assert_eq!(armory.ttps().len(), 87, "unexpected bundled TTP count");
        for ttp in armory.ttps() {
            validate_ttp_vocabulary(ttp, &vocabulary).unwrap_or_else(|error| panic!("{error}"));
        }
    }

    #[test]
    fn generated_reference_is_current() {
        let vocabulary = bundled_vocabulary().expect("vocabulary must be valid");
        let expected = render_vocabulary_markdown(&vocabulary);
        let path = workspace_root().join("docs/book/src/reference/armory-vocabulary.md");
        let actual = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        assert_eq!(
            actual,
            expected,
            "{} is stale; run `cargo run -p armory --bin generate-vocabulary-docs`",
            path.display()
        );
    }

    #[test]
    fn rejects_unknown_and_mistyped_declarations() {
        let vocabulary = bundled_vocabulary().expect("vocabulary must be valid");
        let mut ttp = Ttp::new("bad", "Bad", "Discovery");
        ttp.requires.insert(
            "activeSession".to_string(),
            Value::String("yes".to_string()),
        );
        assert!(matches!(
            validate_ttp_vocabulary(&ttp, &vocabulary),
            Err(VocabularyError::InvalidRequirementType { .. })
        ));

        ttp.requires.clear();
        ttp.effects.push("unknown.effect(${VALUE})".to_string());
        assert!(matches!(
            validate_ttp_vocabulary(&ttp, &vocabulary),
            Err(VocabularyError::UnknownEffect { .. })
        ));
    }
}
