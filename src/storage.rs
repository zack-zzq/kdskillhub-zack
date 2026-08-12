use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fs, path::PathBuf};

pub struct SkillStorage {
    pub base: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledSkill {
    pub name: String,

    #[serde(default)]
    pub version: String,

    #[serde(default)]
    pub slug: Option<String>,

    #[serde(default)]
    pub display_name: Option<String>,

    #[serde(default)]
    pub target: Option<String>,

    #[serde(default)]
    pub source: Value,
}

impl SkillStorage {
    pub fn new(base: PathBuf) -> Self {
        Self { base }
    }

    pub fn skill_path(&self, n: &str) -> PathBuf {
        self.base.join(n)
    }

    pub fn installed(&self, n: &str) -> bool {
        let p = self.skill_path(n);

        p.join("SKILL.md").exists()
            || p.join("skill.md").exists()
            || p.join(".skillhub/info.json").exists()
    }

    pub fn remove(&self, n: &str) -> Result<bool> {
        validate_skill_name(n)?;

        let p = self.skill_path(n);

        if !p.exists() {
            return Ok(false);
        }

        if p.is_file() || p.is_symlink() {
            fs::remove_file(&p)?;
        } else {
            fs::remove_dir_all(&p)?;
        }

        Ok(true)
    }

    pub fn save_info(&self, n: &str, v: &Value) -> Result<()> {
        validate_skill_name(n)?;

        let d = self.skill_path(n).join(".skillhub");

        fs::create_dir_all(&d)
            .with_context(|| format!("failed to create metadata directory {}", d.display()))?;
        fs::write(d.join("info.json"), serde_json::to_vec_pretty(v)?)
            .with_context(|| format!("failed to write metadata for skill {n}"))?;

        Ok(())
    }

    pub fn load_info(&self, n: &str) -> Option<InstalledSkill> {
        if validate_skill_name(n).is_err() {
            return None;
        }

        let p = self.skill_path(n).join(".skillhub/info.json");
        let b = fs::read(p).ok()?;

        serde_json::from_slice(&b).ok()
    }

    pub fn list(&self) -> Result<Vec<String>> {
        if !self.base.exists() {
            return Ok(Vec::new());
        }

        if !self.base.is_dir() {
            return Err(anyhow!(
                "skill storage path is not a directory: {}",
                self.base.display()
            ));
        }

        let mut names = Vec::new();
        for entry in fs::read_dir(&self.base)
            .with_context(|| format!("failed to read skill storage {}", self.base.display()))?
        {
            let entry = entry?;
            names.push(entry.file_name().to_string_lossy().to_string());
        }
        names.sort();
        Ok(names)
    }
}

pub fn validate_skill_name(name: &str) -> Result<()> {
    let trimmed = name.trim();

    if trimmed.is_empty() {
        return Err(anyhow!("skill name cannot be empty"));
    }

    if trimmed != name {
        return Err(anyhow!(
            "skill name must not have leading or trailing whitespace: {name}"
        ));
    }

    if trimmed == "." || trimmed == ".." {
        return Err(anyhow!("invalid skill name: {name}"));
    }

    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err(anyhow!(
            "skill name must not contain path separators: {name}"
        ));
    }

    if trimmed.chars().any(char::is_whitespace) {
        return Err(anyhow!("skill name must not contain whitespace: {name}"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{validate_skill_name, SkillStorage};
    use std::fs;

    #[test]
    fn validate_skill_name_rejects_path_traversal() {
        assert!(validate_skill_name("..").is_err());
        assert!(validate_skill_name("../demo").is_err());
        assert!(validate_skill_name("foo\\bar").is_err());
    }

    #[test]
    fn list_missing_storage_is_empty() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let storage = SkillStorage::new(tmp.path().join("missing"));

        assert!(storage.list().expect("list").is_empty());
    }

    #[test]
    fn list_file_storage_is_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("skills");
        fs::write(&path, "not a dir").expect("write");
        let storage = SkillStorage::new(path);

        assert!(storage.list().is_err());
    }
}
