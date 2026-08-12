use anyhow::{anyhow, Result};
use reqwest::blocking::{Client, Response};
use reqwest::header::{ACCEPT, CONTENT_TYPE, RETRY_AFTER};
use reqwest::StatusCode;
use serde_json::Value;

use super::{RemoteSkill, RemoteSkillVersion, SkillSource};

#[derive(Clone)]
pub struct ClawHubSource {
    pub base: String,
    client: Client,
}

impl ClawHubSource {
    pub fn new(base: Option<String>, timeout: u64) -> Result<Self> {
        Ok(Self {
            base: base
                .unwrap_or_else(|| "https://clawhub.ai".into())
                .trim_end_matches('/')
                .to_string(),
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(timeout))
                .build()?,
        })
    }

    fn get_json(&self, path: &str, params: &[(&str, String)]) -> Result<Value> {
        let resp = self
            .client
            .get(format!("{}{}", self.base, path))
            .header(ACCEPT, "application/json")
            .query(params)
            .send()?;

        let resp = ensure_success(resp, "clawhub api")?;

        Ok(resp.json()?)
    }

    fn map_skill(&self, v: &Value) -> RemoteSkill {
        let slug = first_string(v, &["slug", "id", "key"]).unwrap_or_default();

        let name = first_string(v, &["name", "displayName", "display_name", "title"])
            .or_else(|| {
                v.get("skill")
                    .and_then(|skill| first_string(skill, &["name", "displayName", "title"]))
            })
            .unwrap_or_else(|| slug.clone());

        let version = extract_version(v);

        let description =
            first_string(v, &["description", "summary", "shortDescription"]).or_else(|| {
                v.get("skill").and_then(|skill| {
                    first_string(skill, &["description", "summary", "shortDescription"])
                })
            });

        RemoteSkill {
            name,
            slug: slug.clone(),
            version,
            canonical_url: Some(format!("{}/skills/{}", self.base, slug)),
            source: "clawhub".into(),
            description,
            meta: v.clone(),
        }
    }
}

impl SkillSource for ClawHubSource {
    fn name(&self) -> &'static str {
        "clawhub"
    }

    fn search(&self, keyword: Option<String>, limit: u32, _page: u32) -> Result<Vec<RemoteSkill>> {
        let q = keyword.unwrap_or_default();

        let v = self.get_json(
            "/api/v1/search",
            &[
                ("q", q),
                ("limit", limit.to_string()),
                ("nonSuspiciousOnly", "true".into()),
            ],
        )?;

        let arr = v
            .get("data")
            .and_then(|d| d.as_array())
            .cloned()
            .or_else(|| v.as_array().cloned())
            .unwrap_or_default();

        Ok(arr.iter().map(|i| self.map_skill(i)).collect())
    }

    fn resolve(&self, slug: &str) -> Result<RemoteSkill> {
        let v = self.get_json(&format!("/api/v1/skills/{slug}"), &[])?;

        Ok(self.map_skill(v.get("data").unwrap_or(&v)))
    }

    fn versions(&self, slug: &str) -> Result<Vec<RemoteSkillVersion>> {
        let v = self.get_json(&format!("/api/v1/skills/{slug}/versions"), &[])?;

        let arr = v
            .get("data")
            .and_then(|d| d.as_array())
            .cloned()
            .or_else(|| v.as_array().cloned())
            .unwrap_or_default();

        Ok(arr
            .into_iter()
            .filter_map(|x| extract_version(&x).map(|version| RemoteSkillVersion { version }))
            .collect())
    }

    fn download(&self, slug: &str, version: Option<&str>) -> Result<Vec<u8>> {
        let params = download_params(slug, version);

        let resp = self
            .client
            .get(format!("{}/api/v1/download", self.base))
            .header(ACCEPT, "application/zip, application/octet-stream, */*")
            .query(&params)
            .send()?;

        let resp = ensure_success(resp, "clawhub download")?;

        let content_type = resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let bytes = resp.bytes()?.to_vec();

        if !looks_like_zip(&bytes) {
            return Err(anyhow!(
                "clawhub download did not return a ZIP archive; \
                content-type: {}; response preview: {}",
                if content_type.is_empty() {
                    "<missing>"
                } else {
                    content_type.as_str()
                },
                preview_bytes(&bytes, 500)
            ));
        }

        Ok(bytes)
    }

    fn pre_install_check(&self, slug: &str) -> Result<()> {
        let paths = ["scan", "verify"];

        for p in paths {
            let url = format!("{}/api/v1/skills/{}/{}", self.base, slug, p);

            let Ok(resp) = self
                .client
                .get(&url)
                .header(ACCEPT, "application/json")
                .send()
            else {
                continue;
            };

            if !resp.status().is_success() {
                continue;
            }

            let v: Value = resp.json().unwrap_or(Value::Null);
            let status = v.get("status").and_then(|x| x.as_str()).unwrap_or("");

            if status.eq_ignore_ascii_case("malicious")
                || status.eq_ignore_ascii_case("malware-blocked")
            {
                return Err(anyhow!("blocked by security scan: {status}"));
            }
        }

        Ok(())
    }
}

fn first_string(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| v.get(*key).and_then(|x| x.as_str()))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn extract_version(v: &Value) -> Option<String> {
    first_string(v, &["version", "tag"])
        .or_else(|| {
            v.get("latestVersion").and_then(|x| {
                x.as_str()
                    .map(|s| s.to_string())
                    .or_else(|| first_string(x, &["version", "tag", "name"]))
            })
        })
        .or_else(|| {
            v.get("latest").and_then(|x| {
                x.as_str()
                    .map(|s| s.to_string())
                    .or_else(|| first_string(x, &["version", "tag", "name"]))
            })
        })
}

fn download_params(slug: &str, version: Option<&str>) -> Vec<(&'static str, String)> {
    let mut params = vec![("slug", slug.to_string())];

    match version.map(str::trim) {
        Some(v) if !v.is_empty() && !v.eq_ignore_ascii_case("latest") => {
            params.push(("version", v.to_string()));
        }
        _ => {
            params.push(("tag", "latest".to_string()));
        }
    }

    params
}

fn ensure_success(resp: Response, context: &str) -> Result<Response> {
    let status = resp.status();

    if status.is_success() {
        return Ok(resp);
    }

    let retry_after = resp
        .headers()
        .get(RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let body = resp
        .text()
        .unwrap_or_else(|_| "<failed to read response body>".to_string());

    if status == StatusCode::TOO_MANY_REQUESTS {
        if let Some(retry_after) = retry_after {
            return Err(anyhow!(
                "{context} failed with rate limit 429; retry after {retry_after}s; response: {}",
                preview_text(&body, 500)
            ));
        }

        return Err(anyhow!(
            "{context} failed with rate limit 429; response: {}",
            preview_text(&body, 500)
        ));
    }

    Err(anyhow!(
        "{context} failed with HTTP {status}; response: {}",
        preview_text(&body, 500)
    ))
}

fn looks_like_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
        || bytes.starts_with(b"PK\x05\x06")
        || bytes.starts_with(b"PK\x07\x08")
}

fn preview_bytes(bytes: &[u8], max_len: usize) -> String {
    if bytes.is_empty() {
        return "<empty response>".to_string();
    }

    let take = bytes.len().min(max_len);
    let text = String::from_utf8_lossy(&bytes[..take]);

    preview_text(&text, max_len)
}

fn preview_text(text: &str, max_len: usize) -> String {
    let one_line = text.replace(['\r', '\n'], " ").trim().to_string();

    if one_line.is_empty() {
        return "<non-text response>".to_string();
    }

    if one_line.chars().count() > max_len {
        format!("{}...", one_line.chars().take(max_len).collect::<String>())
    } else {
        one_line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_uses_tag_not_version() {
        let params = download_params("resume-assistant", Some("latest"));

        assert_eq!(
            params,
            vec![
                ("slug", "resume-assistant".to_string()),
                ("tag", "latest".to_string())
            ]
        );
    }

    #[test]
    fn none_uses_latest_tag() {
        let params = download_params("resume-assistant", None);

        assert_eq!(
            params,
            vec![
                ("slug", "resume-assistant".to_string()),
                ("tag", "latest".to_string())
            ]
        );
    }

    #[test]
    fn semver_uses_version() {
        let params = download_params("resume-assistant", Some("1.2.3"));

        assert_eq!(
            params,
            vec![
                ("slug", "resume-assistant".to_string()),
                ("version", "1.2.3".to_string())
            ]
        );
    }

    #[test]
    fn zip_magic_is_detected() {
        assert!(looks_like_zip(b"PK\x03\x04abc"));
        assert!(looks_like_zip(b"PK\x05\x06abc"));
        assert!(looks_like_zip(b"PK\x07\x08abc"));
        assert!(!looks_like_zip(b"not a zip"));
    }

    #[test]
    fn preview_text_handles_multibyte_boundary() {
        assert_eq!(preview_text("技能市场", 3), "技能市...");
    }

    #[test]
    fn map_skill_accepts_display_name_and_summary() {
        let source = ClawHubSource::new(None, 30).expect("source");

        let value = serde_json::json!({
            "slug": "resume-assistant",
            "displayName": "Resume Assistant",
            "summary": "Help polish resumes",
            "latestVersion": {
                "version": "1.0.6"
            }
        });

        let skill = source.map_skill(&value);

        assert_eq!(skill.slug, "resume-assistant");
        assert_eq!(skill.name, "Resume Assistant");
        assert_eq!(skill.description.as_deref(), Some("Help polish resumes"));
        assert_eq!(skill.version.as_deref(), Some("1.0.6"));
    }
}
