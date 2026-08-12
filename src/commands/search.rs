use crate::{
    api::ApiClient,
    sources::{clawhub::ClawHubSource, SkillSource},
};
use anyhow::{anyhow, Result};
use comfy_table::{
    modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Cell, ContentArrangement, Table,
};
pub fn run(
    api: &ApiClient,
    claw: &ClawHubSource,
    source: &str,
    keyword: Option<String>,
    limit: u32,
    page: u32,
    json: bool,
) -> Result<()> {
    if keyword
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .is_empty()
    {
        return Err(anyhow!(
            "search requires a keyword. Usage: skillmux search <keyword>"
        ));
    }

    let rows = if source == "clawhub" {
        claw.search(keyword, limit, page)?
    } else {
        crate::sources::SkillSource::search(api, keyword, limit, page)?
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
    } else {
        let mut table = Table::new();
        table
            .load_preset(UTF8_FULL)
            .apply_modifier(UTF8_ROUND_CORNERS)
            .set_content_arrangement(ContentArrangement::Dynamic);
        table.set_header(vec!["Slug", "Version", "Description"]);
        for s in rows {
            let desc = s.description.unwrap_or_default();
            let clipped = if desc.chars().count() > 80 {
                format!("{}...", desc.chars().take(77).collect::<String>())
            } else {
                desc
            };
            table.add_row(vec![
                Cell::new(s.slug),
                Cell::new(s.version.unwrap_or_default()),
                Cell::new(clipped),
            ]);
        }
        println!("{table}");
    }
    Ok(())
}
