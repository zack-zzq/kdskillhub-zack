use crate::{
    cli::{ConfigCmd, TargetsCmd},
    config::{install_check_path, parse_targets_args, Config, ALL_TARGETS},
};
use anyhow::{anyhow, Result};
use comfy_table::{presets::UTF8_FULL, Attribute, Cell, Table};
pub fn run(cfg: &mut Config, cmd: ConfigCmd) -> Result<()> {
    match cmd {
        ConfigCmd::List => {
            let mut table = Table::new();
            table.load_preset(UTF8_FULL);
            table.set_header(vec!["Key", "Value"]);
            table.add_row(vec![
                Cell::new("api.endpoint"),
                Cell::new(cfg.api.endpoint.clone()),
            ]);
            table.add_row(vec![
                Cell::new("api.timeout"),
                Cell::new(cfg.api.timeout.to_string()),
            ]);
            table.add_row(vec![
                Cell::new("api.token"),
                Cell::new(secret_status(cfg.api.token.as_deref())),
            ]);
            table.add_row(vec![
                Cell::new("source.default"),
                Cell::new(cfg.source.default.clone()),
            ]);
            table.add_row(vec![
                Cell::new("install.targets"),
                Cell::new(cfg.install.targets.join(", ")),
            ]);
            println!("{table}");
        }
        ConfigCmd::Get { key } => {
            let value = cfg
                .get(&key)
                .ok_or_else(|| anyhow!("unknown config key: {key}"))?;
            println!("{value}");
        }
        ConfigCmd::Set { key, value } => {
            cfg.set(&key, &value)?;
            cfg.save()?;
        }
        ConfigCmd::Targets { action, targets } => match action {
            Some(TargetsCmd::List) => print_targets(cfg)?,
            Some(TargetsCmd::Set { targets }) => set_targets(cfg, &targets)?,
            Some(TargetsCmd::Add { targets }) => add_targets(cfg, &targets)?,
            Some(TargetsCmd::Remove { targets }) => remove_targets(cfg, &targets)?,
            None if targets.is_empty() => print_targets(cfg)?,
            None => set_targets(cfg, &targets)?,
        },
    }
    Ok(())
}

fn secret_status(value: Option<&str>) -> &'static str {
    match value.map(str::trim) {
        Some(v) if !v.is_empty() => "<set>",
        _ => "",
    }
}

fn print_targets(cfg: &Config) -> Result<()> {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec![
        Cell::new("Target").add_attribute(Attribute::Bold),
        Cell::new("Selected").add_attribute(Attribute::Bold),
        Cell::new("Install Path").add_attribute(Attribute::Bold),
    ]);
    for t in ALL_TARGETS {
        table.add_row(vec![
            Cell::new(*t),
            Cell::new(if cfg.install.targets.iter().any(|x| x == t) {
                "yes"
            } else {
                "no"
            }),
            Cell::new(install_check_path(t)?.display().to_string()),
        ]);
    }
    println!("{table}");
    Ok(())
}

fn set_targets(cfg: &mut Config, targets: &[String]) -> Result<()> {
    cfg.install.targets = parse_targets_args(targets)?;
    cfg.save()?;
    println!(
        "Updated install targets: {}",
        cfg.install.targets.join(", ")
    );
    Ok(())
}

fn add_targets(cfg: &mut Config, targets: &[String]) -> Result<()> {
    let targets = parse_targets_args(targets)?;
    let mut added = Vec::new();

    for target in targets {
        if !cfg.install.targets.contains(&target) {
            cfg.install.targets.push(target.clone());
            added.push(target);
        }
    }

    if added.is_empty() {
        println!(
            "Install targets unchanged: {}",
            cfg.install.targets.join(", ")
        );
    } else {
        cfg.save()?;
        println!("Added install targets: {}", added.join(", "));
        println!(
            "Current install targets: {}",
            cfg.install.targets.join(", ")
        );
    }

    Ok(())
}

fn remove_targets(cfg: &mut Config, targets: &[String]) -> Result<()> {
    let targets = parse_targets_args(targets)?;
    let before = cfg.install.targets.clone();

    cfg.install
        .targets
        .retain(|target| !targets.contains(target));

    let removed: Vec<_> = before
        .into_iter()
        .filter(|target| !cfg.install.targets.contains(target))
        .collect();

    if removed.is_empty() {
        println!(
            "Install targets unchanged: {}",
            cfg.install.targets.join(", ")
        );
    } else {
        cfg.save()?;
        println!("Removed install targets: {}", removed.join(", "));
        println!(
            "Current install targets: {}",
            cfg.install.targets.join(", ")
        );
    }

    Ok(())
}
