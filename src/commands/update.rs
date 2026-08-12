use crate::{
    api::ApiClient,
    commands::install::{self, InstallReport, InstallStatus},
    config::{target_skill_dir, Config},
    sources::clawhub::ClawHubSource,
    storage::InstalledSkill,
    storage::SkillStorage,
};
use anyhow::{anyhow, Result};
use comfy_table::{
    modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Cell, ContentArrangement, Table,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
struct UpdateCandidate {
    name: String,
    source_type: String,
    source_skill: String,
    match_names: BTreeSet<String>,
}

pub fn run(
    api: &ApiClient,
    claw: &ClawHubSource,
    cfg: &Config,
    skill: Option<String>,
    all: bool,
    ref_name: Option<String>,
) -> Result<()> {
    let candidates = collect_candidates(cfg, ref_name.as_deref())?;

    if candidates.is_empty() {
        println!("No installed skills found in configured targets.");
        return Ok(());
    }

    let requested = skill
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let update_all = all || requested.is_none();

    let selected: Vec<_> = if update_all {
        candidates.clone()
    } else {
        let requested = requested.as_deref().unwrap_or_default();

        candidates
            .iter()
            .filter(|candidate| {
                candidate
                    .match_names
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(requested))
            })
            .cloned()
            .collect()
    };

    if selected.is_empty() {
        let available = candidates
            .iter()
            .map(|candidate| candidate.name.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(", ");

        return Err(anyhow!(
            "skill not installed: {}. Use `skillmux update --all` or choose one of: {}",
            requested.unwrap_or_default(),
            available
        ));
    }

    let mut reports = Vec::new();

    for candidate in selected {
        let report = install::run_silent(
            api,
            claw,
            &candidate.source_type,
            cfg,
            &candidate.source_skill,
            None,
            ref_name.clone(),
            None,
            None,
            true,
            false,
        )?;

        reports.push((candidate, report));
    }

    print_update_summary(&reports);

    Ok(())
}

fn collect_candidates(cfg: &Config, ref_override: Option<&str>) -> Result<Vec<UpdateCandidate>> {
    let mut by_key: BTreeMap<String, UpdateCandidate> = BTreeMap::new();

    for target in &cfg.install.targets {
        let storage = SkillStorage::new(target_skill_dir(target)?);

        for folder_name in storage.list()? {
            let Some(info) = storage.load_info(&folder_name) else {
                continue;
            };

            let source_type = source_type(&info);
            let slug = info.slug.clone().unwrap_or_else(|| folder_name.clone());
            let (key, source_skill) = source_skill(&source_type, &slug, &info, ref_override);
            let mut match_names = BTreeSet::new();

            add_match_name(&mut match_names, &folder_name);
            add_match_name(&mut match_names, &info.name);
            add_match_name(&mut match_names, &slug);

            if let Some(display_name) = &info.display_name {
                add_match_name(&mut match_names, display_name);
            }

            by_key
                .entry(key)
                .and_modify(|candidate| candidate.match_names.extend(match_names.clone()))
                .or_insert_with(|| UpdateCandidate {
                    name: slug,
                    source_type,
                    source_skill,
                    match_names,
                });
        }
    }

    Ok(by_key.into_values().collect())
}

fn source_type(info: &InstalledSkill) -> String {
    info.source
        .get("type")
        .and_then(|v| v.as_str())
        .or_else(|| info.source.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("kingdee")
        .to_string()
}

fn source_skill(
    source_type: &str,
    slug: &str,
    info: &InstalledSkill,
    ref_override: Option<&str>,
) -> (String, String) {
    if source_type == "github" {
        let owner = info.source.get("owner").and_then(|v| v.as_str());
        let repo = info.source.get("repo").and_then(|v| v.as_str());
        let rf = ref_override
            .or_else(|| info.source.get("ref").and_then(|v| v.as_str()))
            .unwrap_or("HEAD");

        if let (Some(owner), Some(repo)) = (owner, repo) {
            let key = format!("github:{owner}/{repo}@{rf}");

            return (key.clone(), key);
        }
    }

    let key = format!("{source_type}:{slug}");
    (key.clone(), key)
}

fn add_match_name(names: &mut BTreeSet<String>, name: &str) {
    let name = name.trim();

    if !name.is_empty() {
        names.insert(name.to_string());
    }
}

fn print_update_summary(reports: &[(UpdateCandidate, InstallReport)]) {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec!["Skill", "Source", "Status", "Version", "Targets"]);

    for (candidate, report) in reports {
        table.add_row(vec![
            Cell::new(&candidate.name),
            Cell::new(&report.source),
            Cell::new(report_status(report)),
            Cell::new(report_version(report)),
            Cell::new(target_statuses(report)),
        ]);
    }

    println!("{table}");
}

fn report_status(report: &InstallReport) -> String {
    let statuses = report
        .targets
        .iter()
        .map(|target| target.status)
        .collect::<BTreeSet<_>>();

    if statuses.len() == 1 {
        statuses
            .into_iter()
            .next()
            .map(InstallStatus::label)
            .unwrap_or("unchanged")
            .to_string()
    } else {
        "mixed".to_string()
    }
}

fn report_version(report: &InstallReport) -> String {
    report
        .targets
        .iter()
        .find(|target| target.status == InstallStatus::Updated)
        .and_then(|target| {
            target
                .previous_version
                .as_ref()
                .map(|previous| format!("{previous} -> {}", target.version))
        })
        .unwrap_or_else(|| report.version.clone())
}

fn target_statuses(report: &InstallReport) -> String {
    report
        .targets
        .iter()
        .map(|target| {
            let mut status = format!("{}: {}", target.target, target.status.label());

            if !target.legacy_aliases_removed.is_empty() {
                status.push_str(&format!(
                    " (removed alias: {})",
                    target.legacy_aliases_removed.join(", ")
                ));
            }

            status
        })
        .collect::<Vec<_>>()
        .join(", ")
}
