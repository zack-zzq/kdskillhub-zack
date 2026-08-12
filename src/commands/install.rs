use crate::{
    api::ApiClient,
    config::{target_skill_dir, Config},
    git_backend::git2_backend,
    installer, skill_manifest,
    sources::{clawhub::ClawHubSource, github, SkillSource},
    storage::{validate_skill_name, SkillStorage},
};
use anyhow::{anyhow, Result};
use directories::BaseDirs;
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

pub fn parse_skill_identifier(v: &str) -> (Option<String>, String, Option<String>) {
    let (s, ver) = v
        .split_once('@')
        .map(|(a, b)| (a.to_string(), Some(b.to_string())))
        .unwrap_or((v.to_string(), None));

    if let Some((src, slug)) = s.split_once(':') {
        (Some(src.into()), slug.into(), ver)
    } else {
        (None, s, ver)
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct InstallReport {
    pub name: String,
    pub version: String,
    pub source: String,
    pub targets: Vec<TargetInstallReport>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TargetInstallReport {
    pub target: String,
    pub status: InstallStatus,
    pub previous_version: Option<String>,
    pub version: String,
    pub legacy_aliases_removed: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallStatus {
    Installed,
    Updated,
    Unchanged,
    Reinstalled,
}

impl InstallStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Installed => "installed",
            Self::Updated => "updated",
            Self::Unchanged => "unchanged",
            Self::Reinstalled => "reinstalled",
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    api: &ApiClient,
    claw: &ClawHubSource,
    source: &str,
    cfg: &Config,
    skill: &str,
    version: Option<String>,
    ref_name: Option<String>,
    subdir: Option<String>,
    as_name: Option<String>,
    yes: bool,
    force: bool,
    json: bool,
) -> Result<InstallReport> {
    let report = run_silent(
        api, claw, source, cfg, skill, version, ref_name, subdir, as_name, yes, force,
    )?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_install_report(&report);
    }

    Ok(report)
}

#[allow(clippy::too_many_arguments)]
pub fn run_silent(
    api: &ApiClient,
    claw: &ClawHubSource,
    source: &str,
    cfg: &Config,
    skill: &str,
    version: Option<String>,
    ref_name: Option<String>,
    subdir: Option<String>,
    as_name: Option<String>,
    yes: bool,
    force: bool,
) -> Result<InstallReport> {
    if cfg.install.targets.is_empty() {
        return Err(anyhow!(
            "no install targets configured. Use `skillmux config targets add <target>` first"
        ));
    }

    if let Some(mut gh) = github::parse(skill) {
        if let Some(ref_name) = ref_name {
            gh.r#ref = ref_name;
        }
        gh.subdir = subdir.clone();

        if !yes {
            print!("Install third-party GitHub skill from {} ? [y/N]: ", gh.url);
            io::stdout().flush()?;

            let mut ans = String::new();
            io::stdin().read_line(&mut ans)?;

            if ans.trim().to_lowercase() != "y" {
                return Err(anyhow!("aborted"));
            }
        }

        let cache_base = BaseDirs::new()
            .ok_or_else(|| anyhow!("failed to resolve user cache directory"))?
            .cache_dir()
            .join("kdskillhub/github")
            .join(github::cache_key(&gh.owner, &gh.repo, &gh.r#ref));

        let sync = git2_backend::sync(&gh.url, &cache_base.join("repo"), Some(&gh.r#ref))?;

        let src_root = match gh.subdir.as_deref() {
            Some(subdir) => safe_join_subdir(&sync.repo_dir, subdir)?,
            None => sync.repo_dir.clone(),
        };

        github::validate_skill_root(&src_root)?;

        let manifest =
            skill_manifest::parse_skill_md(&fs::read_to_string(src_root.join("SKILL.md"))?)?;

        let install_name = as_name.unwrap_or_else(|| manifest.name.clone());
        validate_install_name(&install_name)?;

        let description = manifest
            .description
            .clone()
            .or_else(|| github::repo_description(&gh.owner, &gh.repo));

        let info = serde_json::json!({
            "name": install_name,
            "display_name": manifest.name,
            "version": sync.commit,
            "slug": install_name,
            "canonical_url": gh.url,
            "source": {
                "type": "github",
                "description": description,
                "owner": gh.owner,
                "repo": gh.repo,
                "ref": gh.r#ref,
                "commit": sync.commit
            }
        });

        return install_to_targets(
            cfg,
            &src_root,
            &install_name,
            &install_name,
            "github",
            &sync.commit,
            &info,
            &[],
            true,
            force,
        );
    }

    let (pref, slug, ver2) = parse_skill_identifier(skill);
    let src = pref.unwrap_or_else(|| source.into());
    let ver = version.or(ver2);

    let s: &dyn SkillSource = if src == "clawhub" { claw } else { api };

    s.pre_install_check(&slug)?;

    let resolved = s.resolve(&slug)?;
    let v = ver
        .or(resolved.version.clone())
        .unwrap_or_else(|| "latest".into());

    let zip = s.download(&slug, Some(&v))?;

    let tmp = tempfile::tempdir()?;

    extract_zip_safely(&zip, tmp.path())?;

    let skill_root = resolve_skill_root(tmp.path())?;

    let local_manifest = fs::read_to_string(skill_root.join("SKILL.md"))
        .ok()
        .and_then(|content| skill_manifest::parse_skill_md(&content).ok());

    let install_name = as_name.unwrap_or_else(|| slug.clone());
    validate_install_name(&install_name)?;

    let display_name = first_non_empty([
        local_manifest.as_ref().map(|m| m.name.clone()),
        Some(resolved.name.clone()),
    ]);

    let description = first_non_empty([
        resolved.description.clone(),
        local_manifest.as_ref().and_then(|m| m.description.clone()),
    ]);

    let info = serde_json::json!({
        "name": install_name,
        "display_name": display_name,
        "version": v,
        "slug": slug,
        "canonical_url": resolved.canonical_url,
        "source": {
            "type": src,
            "description": description
        }
    });

    let legacy_aliases: Vec<String> = [
        local_manifest.as_ref().map(|m| m.name.clone()),
        Some(resolved.name.clone()),
    ]
    .into_iter()
    .flatten()
    .filter(|name| name != &install_name)
    .collect();

    install_to_targets(
        cfg,
        &skill_root,
        &install_name,
        &slug,
        &src,
        &v,
        &info,
        &legacy_aliases,
        false,
        force,
    )
}

#[allow(clippy::too_many_arguments)]
fn install_to_targets(
    cfg: &Config,
    src_root: &Path,
    install_name: &str,
    source_slug: &str,
    source_type: &str,
    version: &str,
    info: &serde_json::Value,
    legacy_aliases: &[String],
    allow_symlink: bool,
    force: bool,
) -> Result<InstallReport> {
    let mut targets = Vec::new();

    for t in &cfg.install.targets {
        let st = SkillStorage::new(target_skill_dir(t)?);
        let dst = st.skill_path(install_name);
        let previous = st.load_info(install_name);
        let previous_version = previous
            .as_ref()
            .map(|info| info.version.trim().to_string())
            .filter(|v| !v.is_empty());
        let was_installed = st.installed(install_name);
        let version_changed = previous_version
            .as_deref()
            .map(|previous| previous != version)
            .unwrap_or(false);

        let status = if was_installed && !force && !version_changed {
            st.save_info(install_name, info)?;
            InstallStatus::Unchanged
        } else {
            if allow_symlink {
                installer::install_dir(src_root, &dst)?;
            } else {
                installer::install_dir_copy(src_root, &dst)?;
            }

            verify_installed_skill(&dst)?;
            st.save_info(install_name, info)?;

            if !was_installed {
                InstallStatus::Installed
            } else if version_changed {
                InstallStatus::Updated
            } else {
                InstallStatus::Reinstalled
            }
        };

        let legacy_aliases_removed =
            remove_legacy_aliases(&st, install_name, source_slug, source_type, legacy_aliases)?;

        targets.push(TargetInstallReport {
            target: t.clone(),
            status,
            previous_version,
            version: version.to_string(),
            legacy_aliases_removed,
        });
    }

    Ok(InstallReport {
        name: install_name.to_string(),
        version: version.to_string(),
        source: source_type.to_string(),
        targets,
    })
}

fn verify_installed_skill(dst: &Path) -> Result<()> {
    if dst.join("SKILL.md").exists() {
        return Ok(());
    }

    if dst.exists() {
        if dst.is_dir() && !dst.is_symlink() {
            fs::remove_dir_all(dst)?;
        } else {
            fs::remove_file(dst)?;
        }
    }

    Err(anyhow!(
        "install verification failed: SKILL.md missing in {}",
        dst.display()
    ))
}

fn remove_legacy_aliases(
    storage: &SkillStorage,
    install_name: &str,
    source_slug: &str,
    source_type: &str,
    aliases: &[String],
) -> Result<Vec<String>> {
    let mut removed = Vec::new();

    for alias in aliases {
        if alias == install_name || !is_removable_alias(alias) || removed.contains(alias) {
            continue;
        }

        if !storage.installed(alias) {
            continue;
        }

        let Some(info) = storage.load_info(alias) else {
            continue;
        };

        let installed_source = info
            .source
            .get("type")
            .and_then(|v| v.as_str())
            .or_else(|| info.source.as_str())
            .unwrap_or("kingdee");

        if info.slug.as_deref() == Some(source_slug) && installed_source == source_type {
            storage.remove(alias)?;
            removed.push(alias.clone());
        }
    }

    Ok(removed)
}

fn is_removable_alias(alias: &str) -> bool {
    let trimmed = alias.trim();

    !trimmed.is_empty()
        && trimmed == alias
        && trimmed != "."
        && trimmed != ".."
        && !trimmed.contains('/')
        && !trimmed.contains('\\')
}

fn print_install_report(report: &InstallReport) {
    for target in &report.targets {
        match target.status {
            InstallStatus::Installed => {
                println!("Installed {} -> {}", report.name, target.target);
            }
            InstallStatus::Updated => {
                println!(
                    "Updated {} -> {} ({})",
                    report.name,
                    target.target,
                    version_transition(target)
                );
            }
            InstallStatus::Unchanged => {
                println!(
                    "Already installed {} -> {} ({})",
                    report.name, target.target, target.version
                );
            }
            InstallStatus::Reinstalled => {
                println!("Reinstalled {} -> {}", report.name, target.target);
            }
        }

        for alias in &target.legacy_aliases_removed {
            println!(
                "Removed duplicate install alias {} -> {}",
                alias, target.target
            );
        }
    }
}

fn version_transition(target: &TargetInstallReport) -> String {
    target
        .previous_version
        .as_ref()
        .map(|previous| format!("{previous} -> {}", target.version))
        .unwrap_or_else(|| target.version.clone())
}

fn first_non_empty(values: impl IntoIterator<Item = Option<String>>) -> Option<String> {
    values
        .into_iter()
        .flatten()
        .map(|s| s.trim().to_string())
        .find(|s| !s.is_empty())
}

fn validate_install_name(name: &str) -> Result<()> {
    validate_skill_name(name).map_err(|e| anyhow!("invalid skill install name: {e}"))
}

fn resolve_skill_root(extract_root: &Path) -> Result<PathBuf> {
    if extract_root.join("SKILL.md").exists() {
        return Ok(extract_root.to_path_buf());
    }

    let mut top_dirs = Vec::new();
    for entry in fs::read_dir(extract_root)? {
        let entry = entry?;
        if entry.path().is_dir() {
            top_dirs.push(entry);
        }
    }

    if top_dirs.len() == 1 {
        let p = top_dirs[0].path();

        if p.join("SKILL.md").exists() {
            return Ok(p);
        }
    }

    let mut candidates = Vec::new();

    for entry in walkdir::WalkDir::new(extract_root) {
        let entry = entry?;
        if entry.file_type().is_file() && entry.file_name() == "SKILL.md" {
            if let Some(parent) = entry.path().parent() {
                candidates.push(parent.to_path_buf());
            }
        }
    }

    candidates.sort();
    candidates.dedup();

    match candidates.len() {
        1 => Ok(candidates.remove(0)),
        0 => Err(anyhow!("SKILL.md not found in downloaded archive")),
        _ => Err(anyhow!(
            "multiple SKILL.md candidates found in downloaded archive"
        )),
    }
}

fn safe_join_subdir(root: &Path, subdir: &str) -> Result<PathBuf> {
    let rel = safe_relative_path(subdir)
        .ok_or_else(|| anyhow!("subdir must be a relative path inside the repository: {subdir}"))?;

    Ok(root.join(rel))
}

fn extract_zip_safely(bytes: &[u8], dest: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;

    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        let name = file.name().to_string();
        let rel =
            safe_relative_path(&name).ok_or_else(|| anyhow!("unsafe ZIP entry path: {name}"))?;
        let outpath = dest.join(rel);

        if zip_entry_is_symlink(file.unix_mode()) {
            return Err(anyhow!("refusing to extract symlink from ZIP: {name}"));
        }

        if file.is_dir() {
            fs::create_dir_all(&outpath)?;
            continue;
        }

        if let Some(parent) = outpath.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut outfile = fs::File::create(&outpath)?;
        std::io::copy(&mut file, &mut outfile)?;
    }

    Ok(())
}

fn safe_relative_path(value: &str) -> Option<PathBuf> {
    let trimmed = value.trim();

    if trimmed.is_empty() || trimmed != value {
        return None;
    }

    let path = Path::new(value);
    let mut out = PathBuf::new();

    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => out.push(part),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => return None,
        }
    }

    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
}

fn zip_entry_is_symlink(mode: Option<u32>) -> bool {
    const UNIX_FILE_TYPE_MASK: u32 = 0o170000;
    const UNIX_SYMLINK_TYPE: u32 = 0o120000;

    mode.map(|m| (m & UNIX_FILE_TYPE_MASK) == UNIX_SYMLINK_TYPE)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{
        extract_zip_safely, first_non_empty, parse_skill_identifier, resolve_skill_root,
        safe_join_subdir, validate_install_name,
    };
    use std::{fs, io::Write, path::Path};

    #[test]
    fn parse_skill_identifier_with_source_and_version() {
        let (src, slug, version) = parse_skill_identifier("clawhub:resume-assistant@1.0.6");

        assert_eq!(src.as_deref(), Some("clawhub"));
        assert_eq!(slug, "resume-assistant");
        assert_eq!(version.as_deref(), Some("1.0.6"));
    }

    #[test]
    fn parse_skill_identifier_without_source() {
        let (src, slug, version) = parse_skill_identifier("digital-hunter");

        assert_eq!(src, None);
        assert_eq!(slug, "digital-hunter");
        assert_eq!(version, None);
    }

    #[test]
    fn first_non_empty_skips_blank_values() {
        let value = first_non_empty([
            Some("".to_string()),
            Some("   ".to_string()),
            Some("demo".to_string()),
        ]);

        assert_eq!(value.as_deref(), Some("demo"));
    }

    #[test]
    fn validate_install_name_accepts_safe_id() {
        validate_install_name("resume-assistant").expect("valid");
        validate_install_name("kingdee-ppt").expect("valid");
        validate_install_name("kdclub_ai_product_qa").expect("valid");
    }

    #[test]
    fn validate_install_name_rejects_whitespace() {
        assert!(validate_install_name("Resume Assistant").is_err());
        assert!(validate_install_name(" resume-assistant").is_err());
        assert!(validate_install_name("resume-assistant ").is_err());
    }

    #[test]
    fn validate_install_name_rejects_path_separators() {
        assert!(validate_install_name("../demo").is_err());
        assert!(validate_install_name("foo/bar").is_err());
        assert!(validate_install_name("foo\\bar").is_err());
    }

    #[test]
    fn safe_join_subdir_rejects_escape_paths() {
        let root = Path::new("repo");

        assert!(safe_join_subdir(root, "skills/demo").is_ok());
        assert!(safe_join_subdir(root, "../demo").is_err());
        assert!(safe_join_subdir(root, "skills\\..\\demo").is_err());
    }

    #[test]
    fn resolve_github_style_single_top_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");

        let repo_root = tmp.path().join("demo-main");
        fs::create_dir_all(&repo_root).expect("mkdir");
        fs::write(repo_root.join("SKILL.md"), "# demo").expect("write skill");
        fs::write(repo_root.join("README.md"), "readme").expect("write readme");

        let out = tempfile::tempdir().expect("out");
        let zip_path = out.path().join("skill.zip");

        let f = fs::File::create(&zip_path).expect("zip create");
        let mut zip = zip::ZipWriter::new(f);
        let options = zip::write::SimpleFileOptions::default();

        zip.add_directory("demo-main/", options).expect("add dir");
        zip.start_file("demo-main/SKILL.md", options)
            .expect("start");
        zip.write_all(b"# demo").expect("write");
        zip.finish().expect("finish");

        let extract = tempfile::tempdir().expect("extract");
        let bytes = fs::read(&zip_path).expect("read zip");

        extract_zip_safely(&bytes, extract.path()).expect("extract");

        let root = resolve_skill_root(extract.path()).expect("root");

        assert!(root.join("SKILL.md").exists());
        assert!(!extract.path().join("SKILL.md").exists());
        assert_eq!(root.file_name().and_then(|s| s.to_str()), Some("demo-main"));
    }

    #[test]
    fn extract_zip_safely_rejects_parent_path() {
        let out = tempfile::tempdir().expect("out");
        let zip_path = out.path().join("bad.zip");

        let f = fs::File::create(&zip_path).expect("zip create");
        let mut zip = zip::ZipWriter::new(f);
        let options = zip::write::SimpleFileOptions::default();

        zip.start_file("../evil.txt", options).expect("start");
        zip.write_all(b"bad").expect("write");
        zip.finish().expect("finish");

        let extract = tempfile::tempdir().expect("extract");
        let bytes = fs::read(&zip_path).expect("read zip");

        assert!(extract_zip_safely(&bytes, extract.path()).is_err());
        assert!(!extract.path().join("evil.txt").exists());
    }
}
