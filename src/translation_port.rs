use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PortedStoryTranslation {
    pub schema_version: u32,
    pub table_id: String,
    pub target_rom_sha256: String,
    pub source_rom_sha256: String,
    pub source_terms_sha256: String,
    pub source_style_sha256: String,
    pub build_eligibility: String,
    pub entries: Vec<PortedStoryEntry>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PortedStoryEntry {
    pub entry_id: usize,
    pub logical_id: String,
    pub source_stable_id: String,
    pub target_stable_id: String,
    pub target_file_offset: String,
    pub target_lorom_address: String,
    pub raw_len: usize,
    pub raw_sha256: String,
    pub batch_id: String,
    pub scene_id: String,
    pub context: Value,
    pub ko: String,
    pub status: String,
    pub notes: Value,
}

pub fn port(port_map_path: &Path, source_root: &Path) -> Result<PortedStoryTranslation> {
    let port_map: crate::story_probe::StoryPortMap = read_json(port_map_path)?;
    if port_map.schema_version != 1
        || port_map.table_id != "story_bank_17"
        || port_map.entries.len() != 273
    {
        bail!("story port map does not match the supported schema and population");
    }
    let map_by_source = port_map
        .entries
        .iter()
        .map(|entry| (entry.base_stable_id.as_str(), entry))
        .collect::<BTreeMap<_, _>>();

    let manifest: Value = read_json(&source_root.join("story_batches.json"))?;
    let batches = manifest
        .get("batches")
        .and_then(Value::as_array)
        .context("source story manifest has no batches array")?;
    let mut seen = BTreeSet::new();
    let mut entries = Vec::new();
    for batch in batches {
        let batch_id = string_field(batch, "batch_id")?;
        let work_file = string_field(batch, "work_file")?;
        let scene_id = batch
            .get("scene")
            .and_then(|scene| scene.get("scene_id"))
            .and_then(Value::as_str)
            .context("source manifest batch has no scene_id")?;
        let work: Value = read_json(&source_root.join(work_file))?;
        if string_field(&work, "batch_id")? != batch_id {
            bail!("source work file batch ID differs from its manifest entry");
        }
        let work_entries = work
            .get("entries")
            .and_then(Value::as_array)
            .context("source work file has no entries array")?;
        for work_entry in work_entries {
            let source = work_entry
                .get("source")
                .context("source work entry has no protected source object")?;
            let source_stable_id = string_field(source, "stable_id")?;
            let mapping = map_by_source
                .get(source_stable_id)
                .with_context(|| format!("no Remix mapping for {source_stable_id}"))?;
            if !seen.insert(mapping.entry_id) {
                bail!("duplicate translated entry {}", mapping.logical_id);
            }
            let raw = decode_hex(string_field(source, "raw_hex")?)?;
            let raw_sha256 = format!("{:x}", Sha256::digest(&raw));
            if raw.len() != mapping.raw_len || raw_sha256 != mapping.raw_sha256 {
                bail!("protected raw bytes differ for {source_stable_id}");
            }
            let ko = string_field(work_entry, "ko")?;
            let status = string_field(work_entry, "status")?;
            if ko.is_empty() || status != "needs_review" {
                bail!("ported PoC requires non-empty needs_review text for {source_stable_id}");
            }
            let mut context = work_entry
                .get("context")
                .cloned()
                .context("source work entry has no context object")?;
            if let Some(object) = context.as_object_mut() {
                object.remove("stable_id");
            }
            entries.push(PortedStoryEntry {
                entry_id: mapping.entry_id,
                logical_id: mapping.logical_id.clone(),
                source_stable_id: mapping.base_stable_id.clone(),
                target_stable_id: mapping.target_stable_id.clone(),
                target_file_offset: mapping.target_file_offset.clone(),
                target_lorom_address: mapping.target_lorom_address.clone(),
                raw_len: mapping.raw_len,
                raw_sha256,
                batch_id: batch_id.to_owned(),
                scene_id: scene_id.to_owned(),
                context,
                ko: ko.to_owned(),
                status: status.to_owned(),
                notes: work_entry.get("notes").cloned().unwrap_or(Value::Null),
            });
        }
    }
    entries.sort_by_key(|entry| entry.entry_id);
    if entries.len() != port_map.entries.len()
        || entries
            .iter()
            .enumerate()
            .any(|(expected, entry)| entry.entry_id != expected)
    {
        bail!("ported translation does not cover every logical story entry exactly once");
    }

    Ok(PortedStoryTranslation {
        schema_version: 1,
        table_id: port_map.table_id,
        target_rom_sha256: port_map.target_rom_sha256,
        source_rom_sha256: port_map.base_rom_sha256,
        source_terms_sha256: file_sha256(&source_root.join("story_terms.tsv"))?,
        source_style_sha256: file_sha256(&source_root.join("story_style.md"))?,
        build_eligibility: "poc_only_needs_review".to_owned(),
        entries,
    })
}

pub fn read_ported(path: &Path) -> Result<PortedStoryTranslation> {
    read_json(path)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("invalid JSON in {}", path.display()))
}

fn string_field<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .with_context(|| format!("missing string field {field}"))
}

fn decode_hex(value: &str) -> Result<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        bail!("raw hex has odd length");
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).map_err(Into::into))
        .collect()
}

fn file_sha256(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
