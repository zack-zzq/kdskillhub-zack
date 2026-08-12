use anyhow::{anyhow, Result};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_TYPE, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::Value;

include!(concat!(env!("OUT_DIR"), "/generated_token.rs"));

#[derive(Clone)]
pub struct ApiClient {
    base: String,
    token: String,
    client: Client,
}

impl ApiClient {
    pub fn new(base: String, timeout: u64, token: String) -> Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(timeout))
            .build()?;
        Ok(Self {
            base: base.trim_end_matches('/').to_string(),
            token,
            client,
        })
    }
    pub fn default_token() -> String {
        String::from_utf8(TOKEN_DATA.iter().map(|b| b ^ TOKEN_KEY).collect()).unwrap_or_default()
    }
    pub fn search(&self, keyword: Option<String>, page: u32, page_size: u32) -> Result<Value> {
        let mut req = self
            .client
            .get(format!("{}/skills/list", self.base))
            .query(&[("page", page), ("pageSize", page_size)]);
        if let Some(k) = keyword.as_ref() {
            req = req.query(&[("keyword", k)]);
        }
        Ok(req
            .headers(self.headers()?)
            .send()?
            .error_for_status()?
            .json()?)
    }
    pub fn download(&self, id: i64, version: &str) -> Result<Vec<u8>> {
        Ok(self
            .client
            .get(format!("{}/skills/download", self.base))
            .query(&[
                ("id", id.to_string()),
                ("version", version.to_string()),
                ("token", self.token.clone()),
            ])
            .headers(self.headers()?)
            .send()?
            .error_for_status()?
            .bytes()?
            .to_vec())
    }
    fn headers(&self) -> Result<HeaderMap> {
        let mut h = HeaderMap::new();
        h.insert(ACCEPT, HeaderValue::from_static("application/json"));
        h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        h.insert(
            USER_AGENT,
            HeaderValue::from_str(&format!("skillhub-cli/{}", env!("CARGO_PKG_VERSION")))?,
        );
        if let Ok(v) = self.token.parse() {
            h.insert("token", v);
        }
        Ok(h)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SkillInfo {
    pub id: i64,
    pub name: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "currentVersion")]
    pub current_version: Option<String>,
    pub version: Option<String>,
}

pub fn extract_list(v: &Value) -> Result<Vec<SkillInfo>> {
    let list = v
        .get("data")
        .and_then(|d| d.get("list"))
        .ok_or_else(|| anyhow!("missing data.list"))?;
    Ok(serde_json::from_value(list.clone())?)
}
