use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use chrono::{SecondsFormat, Utc};
use kubetier::{
    Catalog, EscalationPath, PermissionAssessment, RoleAssessment, RoleRule, Scope, Tier,
    ATTRIBUTION, SOURCE_URL,
};
use reqwest::header::ETAG;
use sha2::{Digest, Sha256};

const BASE: &str = "https://kubetier.com";

#[derive(Debug)]
struct Args {
    output: PathBuf,
    full: bool,
    fetched_at: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = parse_args()?;
    let client = reqwest::Client::builder()
        .user_agent("Ran KubeTier snapshot importer (+https://github.com/Magier/Ran)")
        .build()?;

    let (llms, etag) = fetch(&client, "/llms.txt").await?;
    let (reference, _) = fetch(&client, "/reference.md").await?;
    let (roles_index, _) = fetch(&client, "/roles.md").await?;
    let (methodology, _) = fetch(&client, "/llms-full.txt").await?;

    let mut source_hasher = Sha256::new();
    for text in [&llms, &reference, &roles_index, &methodology] {
        source_hasher.update(text.as_bytes());
    }

    let permission_rows = parse_permission_index(&reference)?;
    validate_permission_count(&reference, permission_rows.len())?;

    let mut permissions = Vec::with_capacity(permission_rows.len());
    for row in permission_rows {
        let (detail, _) = fetch(&client, &row.path).await?;
        source_hasher.update(detail.as_bytes());
        permissions.push(parse_permission_detail(row, &detail, args.full)?);
    }

    let role_rows = parse_role_index(&roles_index)?;
    anyhow::ensure!(
        role_rows.len() == 15,
        "expected 15 built-in roles, found {}",
        role_rows.len()
    );
    let mut roles = Vec::with_capacity(role_rows.len());
    for row in role_rows {
        let (detail, _) = fetch(&client, &row.path).await?;
        source_hasher.update(detail.as_bytes());
        roles.push(parse_role_detail(row, &detail, args.full)?);
    }

    permissions.sort_by(|a, b| a.id.cmp(&b.id));
    roles.sort_by(|a, b| a.id.cmp(&b.id));
    let catalog = Catalog {
        schema_version: 1,
        attribution: ATTRIBUTION.to_string(),
        source_url: SOURCE_URL.to_string(),
        fetched_at: args
            .fetched_at
            .unwrap_or_else(|| Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)),
        source_etag: etag,
        source_sha256: source_hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        validated_kubernetes_version: parse_validated_version(&methodology),
        full: args.full,
        permissions,
        roles,
    };
    catalog.validate()?;

    if let Ok(existing) = std::fs::read(&args.output) {
        if let Ok(mut existing) = serde_json::from_slice::<serde_json::Value>(&existing) {
            let mut candidate = serde_json::to_value(&catalog)?;
            // Retrieval time alone must not create weekly update PRs. Compare
            // every other field so importer fixes and metadata changes still
            // regenerate the snapshot even when the upstream bytes are equal.
            existing
                .as_object_mut()
                .and_then(|value| value.remove("fetchedAt"));
            candidate
                .as_object_mut()
                .and_then(|value| value.remove("fetchedAt"));
            if existing == candidate {
                eprintln!(
                    "KubeTier source is unchanged; keeping {}",
                    args.output.display()
                );
                return Ok(());
            }
        }
    }

    let encoded = serde_json::to_string_pretty(&catalog)? + "\n";
    if let Some(parent) = args.output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&args.output, encoded)
        .with_context(|| format!("failed to write {}", args.output.display()))?;
    eprintln!(
        "wrote {} permissions and {} roles to {}",
        catalog.permissions.len(),
        catalog.roles.len(),
        args.output.display()
    );
    Ok(())
}

fn parse_args() -> Result<Args> {
    let mut output = None;
    let mut full = false;
    let mut fetched_at = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--output" => output = args.next().map(PathBuf::from),
            "--full" => full = true,
            "--fetched-at" => fetched_at = args.next(),
            "-h" | "--help" => {
                println!("usage: update_catalog --output PATH [--full] [--fetched-at RFC3339]");
                std::process::exit(0);
            }
            other => bail!("unknown argument {other}"),
        }
    }
    Ok(Args {
        output: output.context("--output is required")?,
        full,
        fetched_at,
    })
}

async fn fetch(client: &reqwest::Client, path: &str) -> Result<(String, Option<String>)> {
    anyhow::ensure!(
        path.starts_with('/') && !path.contains(".."),
        "unsafe KubeTier path {path}"
    );
    let url = format!("{BASE}{path}");
    let response = client.get(&url).send().await?.error_for_status()?;
    let etag = response
        .headers()
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let text = response.text().await?;
    anyhow::ensure!(!text.trim().is_empty(), "empty response from {url}");
    Ok((text, etag))
}

#[derive(Debug)]
struct PermissionRow {
    label: String,
    path: String,
    tier: Tier,
    scope: Scope,
    escalation_count: usize,
}

fn parse_permission_index(input: &str) -> Result<Vec<PermissionRow>> {
    let mut rows = Vec::new();
    for line in input.lines().filter(|line| line.starts_with("| [")) {
        let cells = table_cells(line);
        anyhow::ensure!(cells.len() == 4, "malformed permission row: {line}");
        let (label, path) = markdown_link(cells[0])?;
        anyhow::ensure!(
            path.ends_with(".md"),
            "permission link is not Markdown: {path}"
        );
        rows.push(PermissionRow {
            label,
            path,
            tier: parse_tier(cells[1])?,
            scope: parse_scope(cells[2])?,
            escalation_count: cells[3].parse().context("invalid escalation count")?,
        });
    }
    Ok(rows)
}

fn parse_permission_detail(
    row: PermissionRow,
    input: &str,
    full: bool,
) -> Result<PermissionAssessment> {
    let id = row
        .path
        .trim_start_matches('/')
        .trim_end_matches(".md")
        .to_string();
    let (verb, resource) = split_permission_label(&row.label)?;
    let heading = input.lines().next().unwrap_or_default();
    anyhow::ensure!(
        heading.starts_with("# ") && heading.len() > 2,
        "missing heading for {id}"
    );
    let tier_line = field(input, "Tier").context("missing permission tier")?;
    anyhow::ensure!(
        parse_tier(tier_line.split_whitespace().next().unwrap_or(""))? == row.tier,
        "tier mismatch for {id}"
    );
    let scope = field(input, "Scope")
        .map(parse_scope)
        .transpose()?
        .unwrap_or(row.scope);
    anyhow::ensure!(scope == row.scope, "scope mismatch for {id}");
    let api_group = field(input, "API group")
        .unwrap_or("(core)")
        .trim_matches('`')
        .to_string();
    let api_group = if api_group == "(core)" {
        String::new()
    } else {
        api_group
    };
    let source_url = format!("{BASE}/{}", id);
    let kubernetes_doc_url = input
        .lines()
        .find_map(|line| line.strip_prefix("Kubernetes docs: "))
        .map(str::to_string);
    let description = full.then(|| first_prose(input)).flatten();
    let escalation_paths = if full {
        parse_escalation_paths(input)?
    } else {
        Vec::new()
    };
    if full {
        anyhow::ensure!(
            escalation_paths.len() == row.escalation_count,
            "escalation count mismatch for {id}: index declares {}, detail contains {}",
            row.escalation_count,
            escalation_paths.len()
        );
    }
    Ok(PermissionAssessment {
        id,
        verb,
        resource,
        api_group,
        scope,
        tier: row.tier,
        escalation_count: row.escalation_count,
        source_url,
        kubernetes_doc_url,
        description,
        escalation_paths,
    })
}

#[derive(Debug)]
struct RoleRow {
    name: String,
    path: String,
    tier: Tier,
    scope: Scope,
}

fn parse_role_index(input: &str) -> Result<Vec<RoleRow>> {
    let mut rows = Vec::new();
    for line in input.lines().filter(|line| line.starts_with("| [")) {
        let cells = table_cells(line);
        anyhow::ensure!(cells.len() == 3, "malformed role row: {line}");
        let (name, path) = markdown_link(cells[0])?;
        rows.push(RoleRow {
            name,
            path,
            tier: parse_tier(cells[1])?,
            scope: parse_scope(cells[2])?,
        });
    }
    Ok(rows)
}

fn parse_role_detail(row: RoleRow, input: &str, full: bool) -> Result<RoleAssessment> {
    let id = row
        .path
        .trim_start_matches('/')
        .trim_end_matches(".md")
        .to_string();
    anyhow::ensure!(
        input.lines().next() == Some(format!("# {}", row.name).as_str()),
        "role heading mismatch for {id}"
    );
    let tier = parse_tier(
        field(input, "Tier")
            .context("missing role tier")?
            .split_whitespace()
            .next()
            .unwrap_or(""),
    )?;
    let scope = parse_scope(field(input, "Scope").context("missing role scope")?)?;
    anyhow::ensure!(
        tier == row.tier && scope == row.scope,
        "role index/detail mismatch for {id}"
    );
    let kubernetes_doc_url = input
        .lines()
        .find_map(|line| line.strip_prefix("Kubernetes docs: "))
        .map(str::to_string);
    let rules = parse_role_rules(input)?;
    anyhow::ensure!(!rules.is_empty(), "role {id} contains no rules");
    Ok(RoleAssessment {
        id: id.clone(),
        name: row.name,
        scope,
        tier,
        source_url: format!("{BASE}/{id}"),
        kubernetes_doc_url,
        rules,
        description: full.then(|| first_prose(input)).flatten(),
        notes: if full {
            bullet_section(input, "## Notes")
        } else {
            Vec::new()
        },
    })
}

fn parse_role_rules(input: &str) -> Result<Vec<RoleRule>> {
    let section = section(input, "## Rules");
    let mut rules = Vec::new();
    for line in section.lines().filter(|line| line.starts_with("| ")) {
        if line.contains("apiGroup") || line.contains("---") {
            continue;
        }
        let cells = table_cells(line);
        anyhow::ensure!(cells.len() == 3, "malformed role rule: {line}");
        let verbs = split_csv_or_space(cells[2]);
        if cells[0] == "(non-resource)" {
            rules.push(RoleRule {
                api_groups: vec![],
                resources: vec![],
                non_resource_urls: split_csv(cells[1]),
                verbs,
            });
        } else {
            rules.push(RoleRule {
                api_groups: split_csv(cells[0])
                    .into_iter()
                    .map(|group| {
                        if group == "\"\"" {
                            String::new()
                        } else {
                            group
                        }
                    })
                    .collect(),
                resources: split_csv(cells[1]),
                non_resource_urls: vec![],
                verbs,
            });
        }
    }
    Ok(rules)
}

fn parse_escalation_paths(input: &str) -> Result<Vec<EscalationPath>> {
    let section = section(input, "## Escalation paths");
    let mut paths = Vec::new();
    let mut current: Option<EscalationPath> = None;
    for line in section.lines() {
        if let Some(rest) = line.strip_prefix("### [") {
            if let Some(path) = current.take() {
                paths.push(path);
            }
            let close = rest.find("](").context("malformed escalation heading")?;
            let name = rest[..close].to_string();
            let after = &rest[close + 2..];
            let end = after.find(')').context("malformed escalation URL")?;
            let relative = &after[..end];
            let tier_start = after[end + 1..]
                .find("(T")
                .context("missing escalation tier")?;
            let tier = parse_tier(
                after[end + 1 + tier_start + 1..]
                    .split_whitespace()
                    .next()
                    .unwrap_or(""),
            )?;
            current = Some(EscalationPath {
                name,
                tier,
                source_url: format!("{BASE}{}", relative.trim_end_matches(".md")),
                steps: Vec::new(),
            });
        } else if let Some(step) = ordered_item(line) {
            if let Some(path) = &mut current {
                path.steps.push(step.to_string());
            }
        }
    }
    if let Some(path) = current {
        paths.push(path);
    }
    Ok(paths)
}

fn split_permission_label(label: &str) -> Result<(String, String)> {
    let (verb, resource) = label
        .split_once(' ')
        .context("permission label lacks verb/resource")?;
    let resource = resource.split(" (").next().unwrap_or(resource).to_string();
    Ok((verb.to_string(), resource))
}

fn declared_count(input: &str, noun: &str) -> Result<usize> {
    input
        .lines()
        .find_map(|line| line.strip_suffix(&format!(" {noun} ranked T0 to T3.")))
        .and_then(|count| count.parse().ok())
        .context("missing declared catalog count")
}

fn validate_permission_count(input: &str, actual: usize) -> Result<()> {
    let declared = declared_count(input, "permissions")?;
    anyhow::ensure!(
        actual == declared,
        "reference declared {declared} permissions but contained {actual} rows"
    );
    Ok(())
}

fn parse_validated_version(input: &str) -> Option<String> {
    let marker = "validated against Kubernetes ";
    let start = input.find(marker)? + marker.len();
    let rest = &input[start..];
    Some(
        rest.split(|c: char| c == ',' || c.is_whitespace())
            .next()?
            .trim_end_matches('.')
            .to_string(),
    )
}

fn parse_tier(value: &str) -> Result<Tier> {
    match value.trim().trim_matches('*') {
        "T0" => Ok(Tier::T0),
        "T1" => Ok(Tier::T1),
        "T2" => Ok(Tier::T2),
        "T3" => Ok(Tier::T3),
        other => bail!("unknown KubeTier tier {other}"),
    }
}

fn parse_scope(value: &str) -> Result<Scope> {
    match value.trim() {
        "cluster" => Ok(Scope::Cluster),
        "namespaced" => Ok(Scope::Namespaced),
        other => bail!("unknown KubeTier scope {other}"),
    }
}

fn table_cells(line: &str) -> Vec<&str> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .collect()
}

fn markdown_link(value: &str) -> Result<(String, String)> {
    let value = value
        .strip_prefix('[')
        .context("link lacks opening bracket")?;
    let split = value.find("](").context("malformed Markdown link")?;
    let label = value[..split].to_string();
    let path = value[split + 2..]
        .strip_suffix(')')
        .context("link lacks closing parenthesis")?;
    anyhow::ensure!(
        path.starts_with('/') && !path.contains(".."),
        "unsafe link {path}"
    );
    Ok((label, path.to_string()))
}

fn field<'a>(input: &'a str, name: &str) -> Option<&'a str> {
    let prefix = format!("**{name}:** ");
    input.lines().find_map(|line| {
        line.strip_prefix(&prefix).or_else(|| {
            line.strip_prefix("- ")
                .and_then(|line| line.strip_prefix(&prefix))
        })
    })
}

fn first_prose(input: &str) -> Option<String> {
    let mut metadata_done = false;
    let mut paragraphs = Vec::new();
    for line in input.lines().skip(1) {
        if line.starts_with("**") || line.trim().is_empty() && !metadata_done {
            continue;
        }
        metadata_done = true;
        if line.starts_with("## ") {
            break;
        }
        if !line.trim().is_empty() {
            paragraphs.push(line.trim());
        }
    }
    (!paragraphs.is_empty()).then(|| paragraphs.join(" "))
}

fn section<'a>(input: &'a str, heading: &str) -> &'a str {
    let Some(start) = input.find(heading) else {
        return "";
    };
    let rest = &input[start + heading.len()..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    &rest[..end]
}

fn bullet_section(input: &str, heading: &str) -> Vec<String> {
    section(input, heading)
        .lines()
        .filter_map(|line| line.strip_prefix("- ").map(str::to_string))
        .collect()
}

fn ordered_item(line: &str) -> Option<&str> {
    let (number, text) = line.split_once(". ")?;
    number.chars().all(|c| c.is_ascii_digit()).then_some(text)
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .collect()
}

fn split_csv_or_space(value: &str) -> Vec<String> {
    value
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_index_and_rejects_unknown_tier() {
        let md = "# Permission reference\n\n1 permissions ranked T0 to T3.\n\n| Permission | Tier | Scope | Escalation paths |\n|---|---|---|---|\n| [get secrets](/secrets-get.md) | T1 | namespaced | 3 |\n";
        let rows = parse_permission_index(md).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "get secrets");
        assert_eq!(declared_count(md, "permissions").unwrap(), 1);
        assert!(parse_tier("T4").is_err());
    }

    #[test]
    fn rejects_foreign_or_parent_links() {
        assert!(markdown_link("[x](https://example.com/x.md)").is_err());
        assert!(markdown_link("[x](/../x.md)").is_err());
    }

    #[test]
    fn rejects_changed_or_malformed_permission_counts() {
        let changed = "# Permission reference\n\n2 permissions ranked T0 to T3.\n\n| Permission | Tier | Scope | Escalation paths |\n|---|---|---|---|\n| [get secrets](/secrets-get.md) | T1 | namespaced | 3 |\n";
        let rows = parse_permission_index(changed).unwrap();
        assert!(validate_permission_count(changed, rows.len()).is_err());
        assert!(parse_permission_index("| [broken](/broken.md) | T1 |").is_err());
    }

    #[test]
    fn parses_list_item_metadata_and_full_permission_details() {
        let row = PermissionRow {
            label: "create roles".into(),
            path: "/roles-create.md".into(),
            tier: Tier::T2,
            scope: Scope::Namespaced,
            escalation_count: 1,
        };
        let detail = "# create roles\n\n**Tier:** T2 Conditional Escalation\n\nDescription.\n\n## Details\n\n- **API group:** rbac.authorization.k8s.io\n- **Scope:** namespaced\n\n## Escalation paths\n\n### [Path](/escalation-path.md) (T1 High-Risk Escalation)\n\n1. First step\n";
        let parsed = parse_permission_detail(row, detail, true).unwrap();
        assert_eq!(parsed.api_group, "rbac.authorization.k8s.io");
        assert_eq!(parsed.description.as_deref(), Some("Description."));
        assert_eq!(parsed.escalation_paths.len(), 1);
        assert_eq!(parsed.escalation_paths[0].steps, ["First step"]);
    }

    #[test]
    fn full_import_fails_closed_when_escalation_count_changes() {
        let row = PermissionRow {
            label: "get secrets".into(),
            path: "/secrets-get.md".into(),
            tier: Tier::T1,
            scope: Scope::Namespaced,
            escalation_count: 1,
        };
        let detail = "# get secrets\n\n**Tier:** T1 High-Risk Escalation\n\nDescription.\n\n## Details\n\n- **API group:** (core)\n- **Scope:** namespaced\n\n## Escalation paths\n";
        assert!(parse_permission_detail(row, detail, true).is_err());
    }
}
