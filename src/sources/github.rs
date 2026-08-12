use anyhow::{anyhow, Result};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GitHubSource {
    pub owner: String,
    pub repo: String,
    pub url: String,
    pub r#ref: String,
    pub subdir: Option<String>,
}

pub fn parse(input: &str) -> Option<GitHubSource> {
    let path = input
        .strip_prefix("gh:")
        .or_else(|| input.strip_prefix("github:"))
        .or_else(|| input.strip_prefix("https://github.com/"))?;
    let mut p = path.split('/');
    let owner = p.next()?.to_string();
    let repo = p.next()?.trim_end_matches(".git").to_string();
    let (repo, r#ref) = repo
        .split_once("@")
        .map(|(r, rf)| (r.to_string(), rf.to_string()))
        .unwrap_or((repo, "HEAD".into()));
    if !is_safe_repo_segment(&owner) || !is_safe_repo_segment(&repo) {
        return None;
    }

    Some(GitHubSource {
        url: format!("https://github.com/{owner}/{repo}"),
        owner,
        repo,
        r#ref,
        subdir: None,
    })
}

pub fn cache_key(owner: &str, repo: &str, r: &str) -> String {
    format!(
        "{}/{}/{}",
        sanitize_cache_segment(owner),
        sanitize_cache_segment(repo),
        sanitize_cache_segment(r)
    )
}

fn is_safe_repo_segment(value: &str) -> bool {
    let value = value.trim();

    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

fn sanitize_cache_segment(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();

    if sanitized.is_empty() || sanitized == "." || sanitized == ".." {
        "_".to_string()
    } else {
        sanitized
    }
}

pub fn validate_skill_root(root: &std::path::Path) -> Result<()> {
    if !root.join("SKILL.md").exists() {
        return Err(anyhow!("missing SKILL.md in source directory"));
    }
    Ok(())
}

pub fn repo_description(owner: &str, repo: &str) -> Option<String> {
    let url = format!("https://api.github.com/repos/{owner}/{repo}");
    let c = reqwest::blocking::Client::new();
    let r = c.get(url).header("user-agent", "skillmux").send().ok()?;
    let v: serde_json::Value = r.json().ok()?;
    v.get("description")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn parse_supports_prefix_and_ref() {
        let s = parse("gh:owner/repo@main").expect("parsed");
        assert_eq!(s.owner, "owner");
        assert_eq!(s.repo, "repo");
        assert_eq!(s.r#ref, "main");

        let s2 = parse("github:owner/repo").expect("parsed");
        assert_eq!(s2.r#ref, "HEAD");
    }

    #[test]
    fn parse_rejects_unsafe_owner_or_repo() {
        assert!(parse("gh:../repo").is_none());
        assert!(parse("gh:owner/..").is_none());
    }

    #[test]
    fn cache_key_sanitizes_ref() {
        assert_eq!(
            super::cache_key("owner", "repo", "feature/x"),
            "owner/repo/feature_x"
        );
        assert_eq!(
            super::cache_key("owner", "repo", "..\\x"),
            "owner/repo/.._x"
        );
    }
}
