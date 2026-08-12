use crate::api::ApiClient;
use anyhow::{anyhow, Context, Result};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf};

pub const ALL_TARGETS: &[&str] = &["codex", "qoder", "qoderwork", "kiro", "workbuddy"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub api: Api,
    #[serde(default)]
    pub source: SourceConfig,
    pub install: Install,
    #[serde(skip)]
    path: PathBuf,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Api {
    pub endpoint: String,
    pub timeout: u64,
    pub token: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceConfig {
    #[serde(default = "default_source")]
    pub default: String,
    #[serde(default)]
    pub items: serde_yaml::Value,
}
fn default_source() -> String {
    "kingdee".into()
}
impl Default for SourceConfig {
    fn default() -> Self {
        Self {
            default: default_source(),
            items: serde_yaml::Value::Null,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Install {
    pub targets: Vec<String>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            api: Api {
                endpoint: "https://skills.kingdee.com/api".into(),
                timeout: 30,
                token: None,
            },
            source: SourceConfig::default(),
            install: Install {
                targets: ALL_TARGETS.iter().map(|s| s.to_string()).collect(),
            },
            path: PathBuf::new(),
        }
    }
}
pub fn config_path(custom: Option<&str>) -> Result<PathBuf> {
    if let Some(c) = custom {
        return Ok(PathBuf::from(c));
    }

    Ok(home_dir()?
        .join(".config")
        .join("skillhub")
        .join("config.yaml"))
}
impl Config {
    pub fn load(custom: Option<&str>) -> Result<Self> {
        let path = config_path(custom)?;
        if let Some(p) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(p)
                .with_context(|| format!("failed to create config directory {}", p.display()))?;
        }
        let mut c = Self {
            path: path.clone(),
            ..Default::default()
        };
        if path.exists() {
            let t = fs::read_to_string(&path)
                .with_context(|| format!("failed to read config {}", path.display()))?;
            let user: serde_yaml::Value = serde_yaml::from_str(&t)
                .with_context(|| format!("failed to parse config {}", path.display()))?;
            let mut base = serde_yaml::to_value(&c)?;
            merge(&mut base, user);
            c = serde_yaml::from_value(base)?;
            c.path = path;
        }
        Ok(c)
    }
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create config directory {}", parent.display())
            })?;
        }
        fs::write(&self.path, serde_yaml::to_string(self)?)
            .with_context(|| format!("failed to write config {}", self.path.display()))?;
        Ok(())
    }
    pub fn get(&self, key: &str) -> Option<String> {
        match key {
            "api.endpoint" => Some(self.api.endpoint.clone()),
            "api.timeout" => Some(self.api.timeout.to_string()),
            "api.token" => self.api.token.clone(),
            "source.default" => Some(self.source.default.clone()),
            "install.targets" => Some(self.install.targets.join(",")),
            _ => None,
        }
    }
    pub fn set(&mut self, key: &str, val: &str) -> Result<()> {
        match key {
            "api.endpoint" => self.api.endpoint = val.into(),
            "api.timeout" => {
                let timeout = val
                    .parse::<u64>()
                    .with_context(|| format!("invalid api.timeout value: {val}"))?;
                if timeout == 0 {
                    return Err(anyhow!("api.timeout must be greater than 0"));
                }
                self.api.timeout = timeout;
            }
            "api.token" => self.api.token = Some(val.into()),
            "source.default" => {
                let source = val.trim();
                if source.is_empty() {
                    return Err(anyhow!("source.default cannot be empty"));
                }
                self.source.default = source.into();
            }
            "install.targets" => self.install.targets = parse_targets(val)?,
            _ => return Err(anyhow!("unknown config key: {key}")),
        }
        Ok(())
    }
    pub fn resolve_token(&self, cli_token: Option<String>) -> String {
        cli_token
            .or_else(|| env::var("KDSKILLHUB_TOKEN").ok())
            .or_else(|| self.api.token.clone())
            .unwrap_or_else(ApiClient::default_token)
    }
}
fn home_dir() -> Result<PathBuf> {
    BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .ok_or_else(|| anyhow!("failed to resolve user home directory"))
}

fn merge(a: &mut serde_yaml::Value, b: serde_yaml::Value) {
    match (a, b) {
        (serde_yaml::Value::Mapping(a), serde_yaml::Value::Mapping(b)) => {
            for (k, v) in b {
                merge(a.entry(k).or_insert(serde_yaml::Value::Null), v)
            }
        }
        (a, b) => *a = b,
    }
}
pub fn parse_targets(v: &str) -> Result<Vec<String>> {
    parse_targets_iter(v.split([',', ' ', '\t', '\n']))
}

pub fn parse_targets_args(values: &[String]) -> Result<Vec<String>> {
    parse_targets_iter(values.iter().flat_map(|v| v.split([',', ' ', '\t', '\n'])))
}

fn parse_targets_iter<'a, I>(parts: I) -> Result<Vec<String>>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut out = Vec::new();
    for raw in parts {
        let s = raw.trim().to_lowercase();
        if s.is_empty() {
            continue;
        }
        if !ALL_TARGETS.contains(&s.as_str()) {
            return Err(anyhow!(
                "invalid target '{s}', allowed targets: {}",
                ALL_TARGETS.join(", ")
            ));
        }
        if !out.contains(&s) {
            out.push(s);
        }
    }
    Ok(out)
}
pub fn install_check_path(t: &str) -> Result<PathBuf> {
    let h = home_dir()?;
    Ok(match t {
        "codex" => h.join(".codex/skills"),
        "workbuddy" => h.join(".workbuddy/skills"),
        "qoder" => h.join(".qoder"),
        "qoderwork" => h.join(".qoderwork"),
        "kiro" => h.join(".kiro"),
        _ => h,
    })
}
pub fn target_skill_dir(t: &str) -> Result<PathBuf> {
    let h = home_dir()?;
    Ok(match t {
        "codex" => h.join(".codex/skills"),
        _ => h.join(format!(".{t}/skills")),
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_targets, parse_targets_args, Config, SourceConfig};

    #[test]
    fn parse_targets_comma() {
        let v = parse_targets("qoderwork,codex").expect("parse ok");
        assert_eq!(v, vec!["qoderwork".to_string(), "codex".to_string()]);
    }

    #[test]
    fn parse_targets_space_and_mixed_whitespace() {
        let v = parse_targets_args(&["qoderwork codex,\tkiro".to_string()]).expect("parse ok");
        assert_eq!(
            v,
            vec![
                "qoderwork".to_string(),
                "codex".to_string(),
                "kiro".to_string()
            ]
        );
    }

    #[test]
    fn parse_targets_invalid() {
        let err = parse_targets("codex,badone").expect_err("must fail");
        assert!(err.to_string().contains("allowed targets"));
    }

    #[test]
    fn default_source_is_kingdee() {
        assert_eq!(SourceConfig::default().default, "kingdee");
        assert_eq!(Config::default().source.default, "kingdee");
    }

    #[test]
    fn set_rejects_unknown_key_and_invalid_timeout() {
        let mut cfg = Config::default();

        assert!(cfg.set("bad.key", "value").is_err());
        assert!(cfg.set("api.timeout", "nope").is_err());
        assert!(cfg.set("api.timeout", "0").is_err());
    }
}
