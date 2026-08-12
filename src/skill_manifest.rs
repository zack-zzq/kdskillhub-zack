use anyhow::{anyhow, Result};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct SkillManifest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawSkillManifest {
    #[serde(default)]
    name: Option<String>,

    #[serde(default)]
    description: Option<String>,
}

pub fn parse_skill_md(content: &str) -> Result<SkillManifest> {
    let normalized = normalize_markdown(content);

    let frontmatter = extract_frontmatter(&normalized)
        .ok_or_else(|| anyhow!("SKILL.md must contain frontmatter"))?;

    let raw: RawSkillManifest = serde_yaml::from_str(frontmatter)
        .map_err(|e| anyhow!("invalid SKILL.md frontmatter: {e}"))?;

    let name = raw
        .name
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("frontmatter name is required"))?;

    let description = raw
        .description
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    Ok(SkillManifest { name, description })
}

fn normalize_markdown(content: &str) -> String {
    content
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

fn extract_frontmatter(content: &str) -> Option<&str> {
    let content = content.trim_start();

    let mut offset = 0;
    let mut lines = content.split_inclusive('\n');

    let first = lines.next()?;
    if first.trim() != "---" {
        return None;
    }

    offset += first.len();
    let body_start = offset;

    for line in lines {
        let line_start = offset;

        if line.trim() == "---" {
            return Some(&content[body_start..line_start]);
        }

        offset += line.len();
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lf_frontmatter() {
        let content = "---\nname: demo\ndescription: test skill\n---\n# Demo\n";
        let manifest = parse_skill_md(content).expect("parse");

        assert_eq!(manifest.name, "demo");
        assert_eq!(manifest.description.as_deref(), Some("test skill"));
    }

    #[test]
    fn parse_crlf_frontmatter() {
        let content = "---\r\nname: demo\r\ndescription: test skill\r\n---\r\n# Demo\r\n";
        let manifest = parse_skill_md(content).expect("parse");

        assert_eq!(manifest.name, "demo");
        assert_eq!(manifest.description.as_deref(), Some("test skill"));
    }

    #[test]
    fn parse_bom_frontmatter() {
        let content = "\u{feff}---\nname: demo\n---\n# Demo\n";
        let manifest = parse_skill_md(content).expect("parse");

        assert_eq!(manifest.name, "demo");
        assert_eq!(manifest.description, None);
    }

    #[test]
    fn parse_quoted_values() {
        let content = "---\nname: \"demo\"\ndescription: 'test skill'\n---\n";
        let manifest = parse_skill_md(content).expect("parse");

        assert_eq!(manifest.name, "demo");
        assert_eq!(manifest.description.as_deref(), Some("test skill"));
    }

    #[test]
    fn parse_description_with_colon() {
        let content = "---\nname: demo\ndescription: \"foo: bar: baz\"\n---\n";
        let manifest = parse_skill_md(content).expect("parse");

        assert_eq!(manifest.name, "demo");
        assert_eq!(manifest.description.as_deref(), Some("foo: bar: baz"));
    }

    #[test]
    fn parse_multiline_description() {
        let content = "---\nname: demo\ndescription: |\n  line one\n  line two\n---\n";
        let manifest = parse_skill_md(content).expect("parse");

        assert_eq!(manifest.name, "demo");
        assert_eq!(manifest.description.as_deref(), Some("line one\nline two"));
    }

    #[test]
    fn reject_missing_frontmatter() {
        let content = "# Demo\n";
        let err = parse_skill_md(content).expect_err("should fail");

        assert!(err.to_string().contains("frontmatter"));
    }

    #[test]
    fn reject_missing_name() {
        let content = "---\ndescription: test skill\n---\n";
        let err = parse_skill_md(content).expect_err("should fail");

        assert!(err.to_string().contains("name"));
    }
}
