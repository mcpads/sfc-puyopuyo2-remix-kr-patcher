use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

const BANK_SIZE: usize = 0x8000;
const BASE_SHA256: &str = "5b7ba076d62b0221df270e3a78e2f73ef0efa4dca54354d5207c38283a9dd45c";
const TARGET_SHA256: &str = "19f61feaa61c39254243a28d75ad921a39e4ad3de86c2888c7347691e9fb92fa";
const CAPTION_SELECTOR_PC: usize = 0x03177D;
const CAPTION_DISPLAY_PC: usize = 0x031813;
const CAPTION_WAIT_PC: usize = 0x03189E;
const CAPTION_WAIT_FRAMES: usize = 0x0258;

#[derive(Debug, Serialize)]
pub struct ResourceInventoryReport {
    pub base_path: String,
    pub target_path: String,
    pub base_sha256: String,
    pub target_sha256: String,
    pub base_tables: usize,
    pub target_tables: usize,
    pub base_logical_records: usize,
    pub target_logical_records: usize,
    pub base_resources: usize,
    pub target_resources: usize,
    pub raw_exact_inherited: usize,
    pub decoded_exact_inherited: usize,
    pub changed_logical_counterparts: usize,
    pub target_only_candidates: usize,
    pub base_only_candidates: usize,
    pub tables: Vec<TableComparison>,
    pub resources: Vec<ResourceComparison>,
    pub base_only_resources: Vec<UnmatchedResource>,
    pub caption_routing: CaptionRoutingReport,
}

#[derive(Debug, Serialize)]
pub struct CaptionRoutingReport {
    pub selector_pc: String,
    pub selector_lorom: String,
    pub display_pc: String,
    pub display_lorom: String,
    pub wait_pc: String,
    pub wait_lorom: String,
    pub wait_frames: usize,
    pub routes: Vec<CaptionRoute>,
}

#[derive(Debug, Serialize)]
pub struct CaptionRoute {
    pub id: String,
    pub condition: String,
    pub script_pc: String,
    pub script_lorom: String,
    pub caption_group: usize,
    pub caption_pc: String,
    pub caption_lorom: String,
    pub effect_group: usize,
    pub effect_pc: String,
    pub effect_lorom: String,
}

#[derive(Debug, Serialize)]
pub struct TableComparison {
    pub bank: String,
    pub base_present: bool,
    pub target_present: bool,
    pub base_groups: Option<usize>,
    pub target_groups: Option<usize>,
    pub base_logical_records: Option<usize>,
    pub target_logical_records: Option<usize>,
    pub topology_equal: bool,
}

#[derive(Debug, Serialize)]
pub struct ResourceComparison {
    pub target_pc: String,
    pub target_lorom: String,
    pub target_compressed_len: usize,
    pub target_decoded_len: usize,
    pub target_raw_sha256: String,
    pub target_decoded_sha256: String,
    pub target_consumers: Vec<String>,
    pub inheritance: String,
    pub location: Option<String>,
    pub base_pc: Option<String>,
    pub base_lorom: Option<String>,
    pub base_compressed_len: Option<usize>,
    pub base_decoded_len: Option<usize>,
    pub base_consumers: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct UnmatchedResource {
    pub base_pc: String,
    pub base_lorom: String,
    pub base_compressed_len: usize,
    pub base_decoded_len: usize,
    pub base_raw_sha256: String,
    pub base_decoded_sha256: String,
    pub base_consumers: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AtlasExportReport {
    pub base_path: String,
    pub target_path: String,
    pub out_dir: String,
    pub include_changed: bool,
    pub scale: usize,
    pub atlases: Vec<AtlasEntry>,
}

#[derive(Debug, Serialize)]
pub struct AtlasEntry {
    pub target_pc: String,
    pub target_lorom: String,
    pub inheritance: String,
    pub decoded_len: usize,
    pub tile_count: usize,
    pub columns: usize,
    pub rows: usize,
    pub target_output: String,
    pub target_output_sha256: String,
    pub target_decoded_output: String,
    pub base_pc: Option<String>,
    pub base_decoded_len: Option<usize>,
    pub base_tile_count: Option<usize>,
    pub base_output: Option<String>,
    pub base_output_sha256: Option<String>,
    pub base_decoded_output: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RuntimeMatchReport {
    pub target_path: String,
    pub target_sha256: String,
    pub runtime_dump: String,
    pub vram_sha256: String,
    pub wram_sha256: String,
    pub target_resources: usize,
    pub matched_target_resources: usize,
    pub exact_matches: Vec<RuntimeResourceMatch>,
}

#[derive(Debug, Serialize)]
pub struct RuntimeResourceMatch {
    pub target_pc: String,
    pub target_lorom: String,
    pub inheritance: String,
    pub decoded_len: usize,
    pub target_consumers: Vec<String>,
    pub locations: Vec<RuntimeLocation>,
}

#[derive(Debug, Serialize)]
pub struct RuntimeLocation {
    pub region: String,
    pub offset: String,
}

struct Inventory {
    tables: BTreeMap<u8, AssetTable>,
    resources: BTreeMap<usize, Resource>,
    records_by_logical_id: BTreeMap<String, usize>,
}

struct AssetTable {
    groups: usize,
    group_record_counts: Vec<usize>,
    logical_records: usize,
}

struct Resource {
    pc: usize,
    compressed_len: usize,
    decoded_len: usize,
    raw_sha256: String,
    decoded_sha256: String,
    consumers: BTreeSet<String>,
}

pub fn compare(
    base_path: &Path,
    base: &[u8],
    target_path: &Path,
    target: &[u8],
) -> Result<ResourceInventoryReport> {
    let base_sha256 = sha256(base);
    let target_sha256 = sha256(target);
    if base_sha256 != BASE_SHA256 {
        bail!("Tsuu base ROM SHA-256 mismatch: expected {BASE_SHA256}, got {base_sha256}");
    }
    if target_sha256 != TARGET_SHA256 {
        bail!("Remix target ROM SHA-256 mismatch: expected {TARGET_SHA256}, got {target_sha256}");
    }
    let base_inventory = inventory(base)?;
    let target_inventory = inventory(target)?;
    let caption_routing = audit_caption_routing(target, &target_inventory)?;

    let all_banks = base_inventory
        .tables
        .keys()
        .chain(target_inventory.tables.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    let tables = all_banks
        .into_iter()
        .map(|bank| {
            let base_table = base_inventory.tables.get(&bank);
            let target_table = target_inventory.tables.get(&bank);
            TableComparison {
                bank: format!("${bank:02X}"),
                base_present: base_table.is_some(),
                target_present: target_table.is_some(),
                base_groups: base_table.map(|table| table.groups),
                target_groups: target_table.map(|table| table.groups),
                base_logical_records: base_table.map(|table| table.logical_records),
                target_logical_records: target_table.map(|table| table.logical_records),
                topology_equal: base_table.zip(target_table).is_some_and(|(base, target)| {
                    base.group_record_counts == target.group_record_counts
                }),
            }
        })
        .collect::<Vec<_>>();

    let base_by_raw = index_hash(&base_inventory.resources, |resource| &resource.raw_sha256);
    let base_by_decoded = index_hash(&base_inventory.resources, |resource| {
        &resource.decoded_sha256
    });
    let mut matched_base_pcs = BTreeSet::new();
    let mut resources = Vec::new();
    for target_resource in target_inventory.resources.values() {
        let (inheritance, base_resource) = if let Some(base_pc) = unique_match(
            base_by_raw.get(&target_resource.raw_sha256),
            target_resource.pc,
        ) {
            (
                "raw_exact_inherited",
                base_inventory.resources.get(&base_pc),
            )
        } else if let Some(base_pc) = unique_match(
            base_by_decoded.get(&target_resource.decoded_sha256),
            target_resource.pc,
        ) {
            (
                "decoded_exact_inherited",
                base_inventory.resources.get(&base_pc),
            )
        } else if let Some(base_pc) =
            logical_counterpart(target_resource, &target_inventory, &base_inventory)
        {
            (
                "changed_logical_counterpart",
                base_inventory.resources.get(&base_pc),
            )
        } else {
            ("target_only_candidate", None)
        };
        if let Some(base_resource) = base_resource {
            matched_base_pcs.insert(base_resource.pc);
        }
        resources.push(resource_comparison(
            target_resource,
            inheritance,
            base_resource,
        ));
    }
    let base_only_resources = base_inventory
        .resources
        .values()
        .filter(|resource| !matched_base_pcs.contains(&resource.pc))
        .map(unmatched_resource)
        .collect::<Vec<_>>();

    Ok(ResourceInventoryReport {
        base_path: base_path.display().to_string(),
        target_path: target_path.display().to_string(),
        base_sha256,
        target_sha256,
        base_tables: base_inventory.tables.len(),
        target_tables: target_inventory.tables.len(),
        base_logical_records: base_inventory.records_by_logical_id.len(),
        target_logical_records: target_inventory.records_by_logical_id.len(),
        base_resources: base_inventory.resources.len(),
        target_resources: target_inventory.resources.len(),
        raw_exact_inherited: count_inheritance(&resources, "raw_exact_inherited"),
        decoded_exact_inherited: count_inheritance(&resources, "decoded_exact_inherited"),
        changed_logical_counterparts: count_inheritance(&resources, "changed_logical_counterpart"),
        target_only_candidates: count_inheritance(&resources, "target_only_candidate"),
        base_only_candidates: base_only_resources.len(),
        tables,
        resources,
        base_only_resources,
        caption_routing,
    })
}

fn audit_caption_routing(rom: &[u8], inventory: &Inventory) -> Result<CaptionRoutingReport> {
    const SELECTOR: &[u8] = &[
        0xA0, 0x05, 0x98, 0xAD, 0x0B, 0x03, 0xD0, 0x0B, 0xA0, 0xF7, 0x97, 0xAD, 0x17, 0x03, 0xD0,
        0x03, 0xA0, 0xE9, 0x97, 0x84, 0x12, 0x22, 0xDD, 0xB4, 0x80,
    ];
    const DISPLAY_PREFIX: &[u8] = &[
        0xA0, 0x00, 0x10, 0xAD, 0x0B, 0x03, 0xC9, 0x02, 0x90, 0x03, 0xA0, 0x00, 0x14,
    ];
    const WAIT: &[u8] = &[0xC2, 0x20, 0xA9, 0x58, 0x02, 0x22, 0x2D, 0x81, 0x81];
    verify_bytes(rom, CAPTION_SELECTOR_PC, SELECTOR, "caption selector")?;
    verify_bytes(
        rom,
        CAPTION_DISPLAY_PC,
        DISPLAY_PREFIX,
        "caption display prefix",
    )?;
    verify_bytes(rom, CAPTION_WAIT_PC, WAIT, "caption 600-frame wait")?;

    let specs = [
        (
            "state0_course0",
            "$030B == 0 and $0317 == 0",
            0x0317E9,
            1usize,
            5usize,
        ),
        (
            "state0_other_course",
            "$030B == 0 and $0317 != 0",
            0x0317F7,
            2usize,
            6usize,
        ),
        ("nonzero_state", "$030B != 0", 0x031805, 3usize, 7usize),
    ];
    let mut routes = Vec::with_capacity(specs.len());
    for (id, condition, script_pc, caption_group, effect_group) in specs {
        let script = caption_script(caption_group, effect_group);
        verify_bytes(rom, script_pc, &script, id)?;
        let caption_pc = logical_resource_pc(inventory, 0x27, caption_group)?;
        let effect_pc = logical_resource_pc(inventory, 0x27, effect_group)?;
        routes.push(CaptionRoute {
            id: id.to_owned(),
            condition: condition.to_owned(),
            script_pc: format_pc(script_pc),
            script_lorom: format_lorom(script_pc),
            caption_group,
            caption_pc: format_pc(caption_pc),
            caption_lorom: format_lorom(caption_pc),
            effect_group,
            effect_pc: format_pc(effect_pc),
            effect_lorom: format_lorom(effect_pc),
        });
    }

    Ok(CaptionRoutingReport {
        selector_pc: format_pc(CAPTION_SELECTOR_PC),
        selector_lorom: format_lorom(CAPTION_SELECTOR_PC),
        display_pc: format_pc(CAPTION_DISPLAY_PC),
        display_lorom: format_lorom(CAPTION_DISPLAY_PC),
        wait_pc: format_pc(CAPTION_WAIT_PC),
        wait_lorom: format_lorom(CAPTION_WAIT_PC),
        wait_frames: CAPTION_WAIT_FRAMES,
        routes,
    })
}

fn caption_script(caption_group: usize, effect_group: usize) -> [u8; 14] {
    [
        0x00,
        0x21,
        0x00,
        0x10,
        0xA7,
        caption_group as u8,
        0x7E,
        0x31,
        0x00,
        0x30,
        0xA7,
        effect_group as u8,
        0xFF,
        0xFF,
    ]
}

fn logical_resource_pc(inventory: &Inventory, bank: u8, group: usize) -> Result<usize> {
    let logical_id = format!("bank_{bank:02X}_group_{group:03}_record_00");
    inventory
        .records_by_logical_id
        .get(&logical_id)
        .copied()
        .with_context(|| format!("caption route references missing resource {logical_id}"))
}

fn verify_bytes(rom: &[u8], pc: usize, expected: &[u8], label: &str) -> Result<()> {
    let actual = rom
        .get(pc..pc + expected.len())
        .with_context(|| format!("{label} at {} is outside ROM", format_pc(pc)))?;
    if actual != expected {
        bail!("{label} bytes drifted at {}", format_pc(pc));
    }
    Ok(())
}

pub fn extract_atlases(
    base_path: &Path,
    base: &[u8],
    target_path: &Path,
    target: &[u8],
    out_dir: &Path,
    include_changed: bool,
    scale: usize,
) -> Result<AtlasExportReport> {
    if scale == 0 || scale > 16 {
        bail!("atlas scale must be between 1 and 16");
    }
    let comparison = compare(base_path, base, target_path, target)?;
    fs::create_dir_all(out_dir)
        .with_context(|| format!("create atlas directory {}", out_dir.display()))?;
    let mut atlases = Vec::new();
    for resource in &comparison.resources {
        let selected = resource.inheritance == "target_only_candidate"
            || (include_changed && resource.inheritance == "changed_logical_counterpart");
        if !selected || !resource.target_decoded_len.is_multiple_of(32) {
            continue;
        }
        let pc = parse_pc(&resource.target_pc)?;
        let block = crate::snes_lz::decompress(target, pc)?;
        if block.bytes.len() != resource.target_decoded_len {
            bail!("atlas resource length drift at {}", resource.target_pc);
        }
        let columns = 16usize.min(block.bytes.len() / 32).max(1);
        let tile_count = block.bytes.len() / 32;
        let rows = tile_count.div_ceil(columns);
        let bmp = render_4bpp_atlas(&block.bytes, columns, scale)?;
        let filename = format!(
            "{}_{}_target_{}.bmp",
            resource
                .target_pc
                .trim_start_matches("0x")
                .to_ascii_lowercase(),
            resource.inheritance,
            tile_count
        );
        let target_output = out_dir.join(filename);
        fs::write(&target_output, &bmp)
            .with_context(|| format!("write atlas {}", target_output.display()))?;
        let target_decoded_output = target_output.with_extension("4bpp.bin");
        fs::write(&target_decoded_output, &block.bytes).with_context(|| {
            format!("write decoded resource {}", target_decoded_output.display())
        })?;

        let (base_tile_count, base_output, base_output_sha256, base_decoded_output) =
            if let Some(base_pc) = resource
                .base_pc
                .as_deref()
                .filter(|_| resource.inheritance == "changed_logical_counterpart")
            {
                let base_pc = parse_pc(base_pc)?;
                let base_block = crate::snes_lz::decompress(base, base_pc)?;
                if !base_block.bytes.len().is_multiple_of(32) {
                    (None, None, None, None)
                } else {
                    let base_tile_count = base_block.bytes.len() / 32;
                    let base_columns = 16usize.min(base_tile_count).max(1);
                    let base_bmp = render_4bpp_atlas(&base_block.bytes, base_columns, scale)?;
                    let base_filename = format!(
                        "{}_{}_base_{}.bmp",
                        format_pc(base_pc)
                            .trim_start_matches("0x")
                            .to_ascii_lowercase(),
                        resource.inheritance,
                        base_tile_count
                    );
                    let base_output = out_dir.join(base_filename);
                    fs::write(&base_output, &base_bmp)
                        .with_context(|| format!("write atlas {}", base_output.display()))?;
                    let base_decoded_output = base_output.with_extension("4bpp.bin");
                    fs::write(&base_decoded_output, &base_block.bytes).with_context(|| {
                        format!("write decoded resource {}", base_decoded_output.display())
                    })?;
                    (
                        Some(base_tile_count),
                        Some(base_output.display().to_string()),
                        Some(sha256(&base_bmp)),
                        Some(base_decoded_output.display().to_string()),
                    )
                }
            } else {
                (None, None, None, None)
            };
        atlases.push(AtlasEntry {
            target_pc: resource.target_pc.clone(),
            target_lorom: resource.target_lorom.clone(),
            inheritance: resource.inheritance.clone(),
            decoded_len: block.bytes.len(),
            tile_count,
            columns,
            rows,
            target_output: target_output.display().to_string(),
            target_output_sha256: sha256(&bmp),
            target_decoded_output: target_decoded_output.display().to_string(),
            base_pc: resource.base_pc.clone(),
            base_decoded_len: resource.base_decoded_len,
            base_tile_count,
            base_output,
            base_output_sha256,
            base_decoded_output,
        });
    }
    let report = AtlasExportReport {
        base_path: base_path.display().to_string(),
        target_path: target_path.display().to_string(),
        out_dir: out_dir.display().to_string(),
        include_changed,
        scale,
        atlases,
    };
    let mut manifest = serde_json::to_string_pretty(&report)?;
    manifest.push('\n');
    fs::write(out_dir.join("manifest.json"), manifest)?;
    Ok(report)
}

pub fn match_runtime(
    base_path: &Path,
    base: &[u8],
    target_path: &Path,
    target: &[u8],
    runtime_dump: &Path,
) -> Result<RuntimeMatchReport> {
    let comparison = compare(base_path, base, target_path, target)?;
    let vram_path = runtime_dump.join("vram.bin");
    let wram_path = runtime_dump.join("wram.bin");
    let vram = fs::read(&vram_path)
        .with_context(|| format!("read runtime VRAM {}", vram_path.display()))?;
    let wram = fs::read(&wram_path)
        .with_context(|| format!("read runtime WRAM {}", wram_path.display()))?;
    if vram.len() != 0x10000 || wram.len() != 0x20000 {
        bail!(
            "runtime dump sizes differ from SNES VRAM/WRAM: {} / {}",
            vram.len(),
            wram.len()
        );
    }

    let mut exact_matches = Vec::new();
    for resource in &comparison.resources {
        let pc = parse_pc(&resource.target_pc)?;
        let block = crate::snes_lz::decompress(target, pc)?;
        let mut locations = find_all(&vram, &block.bytes)
            .into_iter()
            .map(|offset| RuntimeLocation {
                region: "vram".to_owned(),
                offset: format!("0x{offset:04X}"),
            })
            .collect::<Vec<_>>();
        locations.extend(
            find_all(&wram, &block.bytes)
                .into_iter()
                .map(|offset| RuntimeLocation {
                    region: "wram".to_owned(),
                    offset: format!("0x{offset:05X}"),
                }),
        );
        if locations.is_empty() {
            continue;
        }
        exact_matches.push(RuntimeResourceMatch {
            target_pc: resource.target_pc.clone(),
            target_lorom: resource.target_lorom.clone(),
            inheritance: resource.inheritance.clone(),
            decoded_len: block.bytes.len(),
            target_consumers: resource.target_consumers.clone(),
            locations,
        });
    }

    Ok(RuntimeMatchReport {
        target_path: target_path.display().to_string(),
        target_sha256: comparison.target_sha256,
        runtime_dump: runtime_dump.display().to_string(),
        vram_sha256: sha256(&vram),
        wram_sha256: sha256(&wram),
        target_resources: comparison.target_resources,
        matched_target_resources: exact_matches.len(),
        exact_matches,
    })
}

fn inventory(rom: &[u8]) -> Result<Inventory> {
    if !rom.len().is_multiple_of(BANK_SIZE) {
        bail!("ROM length is not a whole number of LoROM banks");
    }
    let mut tables = BTreeMap::new();
    let mut resources = BTreeMap::<usize, Resource>::new();
    let mut records_by_logical_id = BTreeMap::new();
    for bank in 0..rom.len() / BANK_SIZE {
        let Some(table) = parse_table(rom, bank as u8)? else {
            continue;
        };
        for record in &table.records {
            records_by_logical_id.insert(record.logical_id.clone(), record.source_pc);
            let resource = resources
                .entry(record.source_pc)
                .or_insert_with(|| Resource {
                    pc: record.source_pc,
                    compressed_len: record.compressed_len,
                    decoded_len: record.decoded_len,
                    raw_sha256: record.raw_sha256.clone(),
                    decoded_sha256: record.decoded_sha256.clone(),
                    consumers: BTreeSet::new(),
                });
            if resource.compressed_len != record.compressed_len
                || resource.decoded_len != record.decoded_len
                || resource.raw_sha256 != record.raw_sha256
                || resource.decoded_sha256 != record.decoded_sha256
            {
                bail!("resource identity drift at PC 0x{:06X}", record.source_pc);
            }
            resource.consumers.insert(record.logical_id.clone());
        }
        tables.insert(
            bank as u8,
            AssetTable {
                groups: table.group_record_counts.len(),
                logical_records: table.records.len(),
                group_record_counts: table.group_record_counts,
            },
        );
    }
    Ok(Inventory {
        tables,
        resources,
        records_by_logical_id,
    })
}

struct ParsedTable {
    group_record_counts: Vec<usize>,
    records: Vec<ParsedRecord>,
}

struct ParsedRecord {
    logical_id: String,
    source_pc: usize,
    compressed_len: usize,
    decoded_len: usize,
    raw_sha256: String,
    decoded_sha256: String,
}

fn parse_table(rom: &[u8], bank: u8) -> Result<Option<ParsedTable>> {
    let bank_start = usize::from(bank) * BANK_SIZE;
    let bank_data = &rom[bank_start..bank_start + BANK_SIZE];
    let first_list = read_word(bank_data, 0)?;
    if !(0x8002..=0x8800).contains(&first_list) || !(first_list - 0x8000).is_multiple_of(2) {
        return Ok(None);
    }
    let group_count = usize::from((first_list - 0x8000) / 2);
    if group_count == 0 || group_count > 1024 {
        return Ok(None);
    }
    let pointers = (0..group_count)
        .map(|index| read_word(bank_data, index * 2))
        .collect::<Result<Vec<_>>>()?;
    if pointers.iter().any(|pointer| *pointer < first_list)
        || pointers.windows(2).any(|pair| pair[0] > pair[1])
    {
        return Ok(None);
    }

    let mut group_record_counts = Vec::with_capacity(group_count);
    let mut records = Vec::new();
    let mut descriptor_end_pc = bank_start;
    for (group_index, pointer) in pointers.iter().copied().enumerate() {
        let mut cursor = usize::from(pointer - 0x8000);
        let mut record_index = 0usize;
        loop {
            let tag = read_word(bank_data, cursor)?;
            if tag == 0xFFFF {
                break;
            }
            if cursor + 8 > BANK_SIZE || record_index >= 64 {
                return Ok(None);
            }
            let source = read_word(bank_data, cursor + 6)?;
            if source < 0x8000 {
                return Ok(None);
            }
            let source_pc = bank_start + usize::from(source - 0x8000);
            let Ok(block) = crate::snes_lz::decompress(rom, source_pc) else {
                return Ok(None);
            };
            if source_pc + block.compressed_len > bank_start + BANK_SIZE {
                return Ok(None);
            }
            let raw = &rom[source_pc..source_pc + block.compressed_len];
            records.push(ParsedRecord {
                logical_id: format!(
                    "bank_{bank:02X}_group_{group_index:03}_record_{record_index:02}"
                ),
                source_pc,
                compressed_len: block.compressed_len,
                decoded_len: block.bytes.len(),
                raw_sha256: sha256(raw),
                decoded_sha256: sha256(&block.bytes),
            });
            cursor += 8;
            record_index += 1;
        }
        descriptor_end_pc = descriptor_end_pc.max(bank_start + cursor + 2);
        group_record_counts.push(record_index);
    }
    if records.is_empty() {
        return Ok(None);
    }
    let first_source = records
        .iter()
        .map(|record| record.source_pc)
        .min()
        .context("asset table has no source")?;
    if descriptor_end_pc > first_source {
        return Ok(None);
    }
    Ok(Some(ParsedTable {
        group_record_counts,
        records,
    }))
}

fn index_hash<'a, F>(
    resources: &'a BTreeMap<usize, Resource>,
    select: F,
) -> BTreeMap<String, Vec<usize>>
where
    F: Fn(&'a Resource) -> &'a str,
{
    let mut index = BTreeMap::<String, Vec<usize>>::new();
    for resource in resources.values() {
        index
            .entry(select(resource).to_owned())
            .or_default()
            .push(resource.pc);
    }
    index
}

fn unique_match(candidates: Option<&Vec<usize>>, preferred_pc: usize) -> Option<usize> {
    let candidates = candidates?;
    candidates
        .iter()
        .copied()
        .find(|pc| *pc == preferred_pc)
        .or_else(|| (candidates.len() == 1).then_some(candidates[0]))
}

fn logical_counterpart(
    target_resource: &Resource,
    target_inventory: &Inventory,
    base_inventory: &Inventory,
) -> Option<usize> {
    let candidates = target_resource
        .consumers
        .iter()
        .filter_map(|logical_id| {
            target_inventory
                .records_by_logical_id
                .get(logical_id)
                .filter(|pc| **pc == target_resource.pc)?;
            base_inventory
                .records_by_logical_id
                .get(logical_id)
                .copied()
        })
        .collect::<BTreeSet<_>>();
    (candidates.len() == 1).then(|| *candidates.first().expect("one logical counterpart"))
}

fn resource_comparison(
    target: &Resource,
    inheritance: &str,
    base: Option<&Resource>,
) -> ResourceComparison {
    ResourceComparison {
        target_pc: format_pc(target.pc),
        target_lorom: format_lorom(target.pc),
        target_compressed_len: target.compressed_len,
        target_decoded_len: target.decoded_len,
        target_raw_sha256: target.raw_sha256.clone(),
        target_decoded_sha256: target.decoded_sha256.clone(),
        target_consumers: target.consumers.iter().cloned().collect(),
        inheritance: inheritance.to_owned(),
        location: base.map(|base| {
            if base.pc == target.pc {
                "same_pc".to_owned()
            } else {
                "relocated".to_owned()
            }
        }),
        base_pc: base.map(|base| format_pc(base.pc)),
        base_lorom: base.map(|base| format_lorom(base.pc)),
        base_compressed_len: base.map(|base| base.compressed_len),
        base_decoded_len: base.map(|base| base.decoded_len),
        base_consumers: base
            .map(|base| base.consumers.iter().cloned().collect())
            .unwrap_or_default(),
    }
}

fn unmatched_resource(resource: &Resource) -> UnmatchedResource {
    UnmatchedResource {
        base_pc: format_pc(resource.pc),
        base_lorom: format_lorom(resource.pc),
        base_compressed_len: resource.compressed_len,
        base_decoded_len: resource.decoded_len,
        base_raw_sha256: resource.raw_sha256.clone(),
        base_decoded_sha256: resource.decoded_sha256.clone(),
        base_consumers: resource.consumers.iter().cloned().collect(),
    }
}

fn count_inheritance(resources: &[ResourceComparison], inheritance: &str) -> usize {
    resources
        .iter()
        .filter(|resource| resource.inheritance == inheritance)
        .count()
}

fn find_all(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }
    haystack
        .windows(needle.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == needle).then_some(offset))
        .collect()
}

fn render_4bpp_atlas(decoded: &[u8], columns: usize, scale: usize) -> Result<Vec<u8>> {
    if decoded.is_empty() || !decoded.len().is_multiple_of(32) || columns == 0 || scale == 0 {
        bail!("4bpp atlas input must contain whole SNES tiles");
    }
    let tile_count = decoded.len() / 32;
    let rows = tile_count.div_ceil(columns);
    let width = columns * 8 * scale;
    let height = rows * 8 * scale;
    let row_stride = (width * 3 + 3) & !3;
    let pixel_bytes = row_stride
        .checked_mul(height)
        .context("atlas pixel size overflow")?;
    let file_size = 54usize
        .checked_add(pixel_bytes)
        .context("atlas file size overflow")?;
    let mut bmp = vec![0u8; file_size];
    bmp[0..2].copy_from_slice(b"BM");
    bmp[2..6].copy_from_slice(&(file_size as u32).to_le_bytes());
    bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
    bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
    bmp[18..22].copy_from_slice(&(width as i32).to_le_bytes());
    bmp[22..26].copy_from_slice(&(height as i32).to_le_bytes());
    bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
    bmp[28..30].copy_from_slice(&24u16.to_le_bytes());
    bmp[34..38].copy_from_slice(&(pixel_bytes as u32).to_le_bytes());

    for tile_index in 0..tile_count {
        let tile = &decoded[tile_index * 32..tile_index * 32 + 32];
        let tile_x = tile_index % columns;
        let tile_y = tile_index / columns;
        for row in 0..8 {
            for column in 0..8 {
                let bit = 7 - column;
                let palette_index = ((tile[row * 2] >> bit) & 1)
                    | (((tile[row * 2 + 1] >> bit) & 1) << 1)
                    | (((tile[16 + row * 2] >> bit) & 1) << 2)
                    | (((tile[16 + row * 2 + 1] >> bit) & 1) << 3);
                let value = palette_index * 17;
                for dy in 0..scale {
                    for dx in 0..scale {
                        let x = (tile_x * 8 + column) * scale + dx;
                        let y = (tile_y * 8 + row) * scale + dy;
                        let bmp_y = height - 1 - y;
                        let offset = 54 + bmp_y * row_stride + x * 3;
                        bmp[offset..offset + 3].copy_from_slice(&[value, value, value]);
                    }
                }
            }
        }
    }
    Ok(bmp)
}

fn read_word(data: &[u8], offset: usize) -> Result<u16> {
    let bytes = data
        .get(offset..offset + 2)
        .with_context(|| format!("word at offset 0x{offset:04X} is outside bank"))?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn format_pc(pc: usize) -> String {
    format!("0x{pc:06X}")
}

fn parse_pc(value: &str) -> Result<usize> {
    usize::from_str_radix(
        value
            .strip_prefix("0x")
            .with_context(|| format!("PC offset is not 0x-prefixed: {value}"))?,
        16,
    )
    .with_context(|| format!("invalid PC offset: {value}"))
}

fn format_lorom(pc: usize) -> String {
    let (bank, address) = crate::rom::pc_to_lorom(pc);
    format!("${bank:02X}:${address:04X}")
}

fn sha256(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_consumer_backed_bank_local_resource_table() {
        let mut rom = vec![0xFF; BANK_SIZE];
        rom[0..2].copy_from_slice(&0x8002u16.to_le_bytes());
        rom[2..4].copy_from_slice(&0x0088u16.to_le_bytes());
        rom[4..6].copy_from_slice(&0x0000u16.to_le_bytes());
        rom[6..8].copy_from_slice(&0x0000u16.to_le_bytes());
        rom[8..10].copy_from_slice(&0x8010u16.to_le_bytes());
        rom[10..12].copy_from_slice(&0xFFFFu16.to_le_bytes());
        let compressed = crate::snes_lz::compress(&[0xA5; 32]);
        rom[0x10..0x10 + compressed.len()].copy_from_slice(&compressed);

        let table = parse_table(&rom, 0).unwrap().unwrap();
        assert_eq!(table.group_record_counts, vec![1]);
        assert_eq!(table.records.len(), 1);
        assert_eq!(table.records[0].logical_id, "bank_00_group_000_record_00");
        assert_eq!(table.records[0].source_pc, 0x10);
        assert_eq!(table.records[0].decoded_len, 32);
    }

    #[test]
    fn atlas_renders_snes_bitplanes_and_bmp_bottom_up_rows() {
        let mut tile = [0u8; 32];
        tile[0] = 0x80;
        tile[1] = 0x80;
        tile[16] = 0x80;
        tile[17] = 0x80;

        let bmp = render_4bpp_atlas(&tile, 1, 1).unwrap();
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(u32::from_le_bytes(bmp[18..22].try_into().unwrap()), 8);
        assert_eq!(u32::from_le_bytes(bmp[22..26].try_into().unwrap()), 8);

        let row_stride = 24;
        let top_left = 54 + 7 * row_stride;
        assert_eq!(&bmp[top_left..top_left + 3], &[0xFF, 0xFF, 0xFF]);
        assert_eq!(&bmp[top_left + 3..top_left + 6], &[0, 0, 0]);
        assert_eq!(&bmp[54..57], &[0, 0, 0]);
    }

    #[test]
    fn atlas_rejects_partial_tiles_and_zero_scale() {
        assert!(render_4bpp_atlas(&[0; 31], 1, 1).is_err());
        assert!(render_4bpp_atlas(&[0; 32], 1, 0).is_err());
    }

    #[test]
    fn finds_all_exact_runtime_locations() {
        assert_eq!(find_all(b"PUYOPUYOPUYO", b"PUYO"), vec![0, 4, 8]);
        assert!(find_all(b"PUYO", b"").is_empty());
        assert!(find_all(b"PUYO", b"PUYOPUYO").is_empty());
    }

    #[test]
    fn audits_three_caption_routes_and_their_effect_pairs() {
        let mut rom = vec![0; CAPTION_WAIT_PC + 9];
        rom[CAPTION_SELECTOR_PC..CAPTION_SELECTOR_PC + 25].copy_from_slice(&[
            0xA0, 0x05, 0x98, 0xAD, 0x0B, 0x03, 0xD0, 0x0B, 0xA0, 0xF7, 0x97, 0xAD, 0x17, 0x03,
            0xD0, 0x03, 0xA0, 0xE9, 0x97, 0x84, 0x12, 0x22, 0xDD, 0xB4, 0x80,
        ]);
        rom[CAPTION_DISPLAY_PC..CAPTION_DISPLAY_PC + 13].copy_from_slice(&[
            0xA0, 0x00, 0x10, 0xAD, 0x0B, 0x03, 0xC9, 0x02, 0x90, 0x03, 0xA0, 0x00, 0x14,
        ]);
        rom[CAPTION_WAIT_PC..CAPTION_WAIT_PC + 9]
            .copy_from_slice(&[0xC2, 0x20, 0xA9, 0x58, 0x02, 0x22, 0x2D, 0x81, 0x81]);
        for (script_pc, caption_group, effect_group) in
            [(0x0317E9, 1, 5), (0x0317F7, 2, 6), (0x031805, 3, 7)]
        {
            rom[script_pc..script_pc + 14]
                .copy_from_slice(&caption_script(caption_group, effect_group));
        }

        let records_by_logical_id = [
            ("bank_27_group_001_record_00", 0x138B0B),
            ("bank_27_group_002_record_00", 0x13A0AB),
            ("bank_27_group_003_record_00", 0x13B44F),
            ("bank_27_group_005_record_00", 0x13CCFB),
            ("bank_27_group_006_record_00", 0x13CF16),
            ("bank_27_group_007_record_00", 0x13D0FD),
        ]
        .into_iter()
        .map(|(id, pc)| (id.to_owned(), pc))
        .collect();
        let inventory = Inventory {
            tables: BTreeMap::new(),
            resources: BTreeMap::new(),
            records_by_logical_id,
        };

        let report = audit_caption_routing(&rom, &inventory).unwrap();
        assert_eq!(report.selector_lorom, "$06:$977D");
        assert_eq!(report.wait_frames, 600);
        assert_eq!(report.routes.len(), 3);
        assert_eq!(report.routes[0].caption_pc, "0x138B0B");
        assert_eq!(report.routes[0].effect_pc, "0x13CCFB");
        assert_eq!(report.routes[2].caption_pc, "0x13B44F");
        assert_eq!(report.routes[2].effect_pc, "0x13D0FD");
    }
}
