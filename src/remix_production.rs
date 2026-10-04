//! Production build for the Remix Korean patch.
//!
//! Inputs are the supported Remix JP ROM and committed repository assets only.
//! Menu label importers receive BG1 layouts materialized from
//! `remix_layout_spec`; runtime dumps are never read. The build composes the
//! same stages as the integrated PoC, applies the selected input policy, writes
//! an in-ROM policy marker for non-release builds, audits the complete diff and
//! emits a BPS that is replayed in process before it is written.

use std::{
    collections::BTreeMap,
    fmt,
    ops::Range,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Inert candidate free space: the last 64 bytes of Bank `$2C`, `0xFF` in the
/// supported source and outside every integrated-PoC writer.
pub const MARKER_RANGE: Range<usize> = 0x16_7FC0..0x16_8000;
const MARKER_PREFIX: &str = "KR-PATCH ";
const CHECKSUM_RANGE: Range<usize> = 0x7FDC..0x7FE0;

const REVIEW_STATE_PATH: &str = "assets/translations/review_state.json";
const STORY_TRANSLATION_PATH: &str = "assets/translations/story_ko.json";
const STORY_REVIEW_PATH: &str = "assets/translations/story_review.json";

/// Every unit the integrated composition writes, plus declared scope that is
/// not yet localized. The registry must list exactly these IDs.
pub const DECLARED_UNITS: [&str; 22] = [
    "story_bank_17",
    "ending_text",
    "multi_ui_text",
    "rule_editor",
    "option_help",
    "demo_explanation_body",
    "sample_select_text",
    "opponent_prompt",
    "two_player_rules",
    "demo_explanation_titles",
    "mode_prompt",
    "mode_labels",
    "remix_menu_labels",
    "tokoton_choices",
    "tokoton_difficulty",
    "gameplay_effects",
    "ranking_titles",
    "true_ending_graphics",
    "easy_course_signs",
    "game_over_continue",
    "easy_ending_captions",
    "nomi_marker",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Policy {
    /// Development build; any buildable review state; non-distribution marker.
    Poc,
    /// Identified-tester build; unapproved text allowed, marker plus declared
    /// scope and unresolved list.
    PreRelease,
    /// Owner-approved public preview; no marker, versioned output, open
    /// review items carried in the report instead of blocking.
    Preview,
    /// Distribution build; every unit approved and runtime-QA passed.
    Release,
}

impl Policy {
    fn tag(self) -> &'static str {
        match self {
            Self::Poc => "POC",
            Self::PreRelease => "PRE-RELEASE",
            Self::Preview => "PREVIEW",
            Self::Release => "RELEASE",
        }
    }

    fn dir_name(self) -> &'static str {
        match self {
            Self::Poc => "poc",
            Self::PreRelease => "pre-release",
            Self::Preview => "preview",
            Self::Release => "release",
        }
    }
}

impl fmt::Display for Policy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.dir_name())
    }
}

/// The project owner's recorded decision to publish a preview before every
/// unit is human-approved. The preview policy refuses to build without it.
pub const PREVIEW_DECISION_PATH: &str = "assets/release/preview.json";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PreviewDecision {
    pub version: String,
    pub approved_by: String,
    pub approved_on: String,
    pub decision: String,
    pub known_limitations: Vec<String>,
}

fn read_preview_decision() -> Result<PreviewDecision> {
    let decision: PreviewDecision =
        serde_json::from_slice(&std::fs::read(PREVIEW_DECISION_PATH).with_context(|| {
            format!("preview policy needs the owner decision {PREVIEW_DECISION_PATH}")
        })?)?;
    validate_preview_decision(&decision)?;
    Ok(decision)
}

fn validate_preview_decision(decision: &PreviewDecision) -> Result<()> {
    let parts: Vec<&str> = decision.version.split('.').collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()))
    {
        bail!(
            "preview version '{}' is not MAJOR.MINOR.PATCH",
            decision.version
        );
    }
    if decision.approved_by.trim().is_empty()
        || decision.approved_on.trim().is_empty()
        || decision.decision.trim().is_empty()
    {
        bail!("preview decision must name the approving owner, date and decision");
    }
    if decision.known_limitations.is_empty() {
        bail!("preview decision must list its known limitations");
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct ReviewRegistry {
    schema_version: u32,
    declared_scope: String,
    state_values: Vec<String>,
    runtime_qa_values: Vec<String>,
    units: Vec<ReviewUnit>,
    release_exceptions: Vec<ReleaseException>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReviewUnit {
    pub unit_id: String,
    pub kind: String,
    pub sources: Vec<String>,
    pub review_state: String,
    pub runtime_qa: String,
    pub approved_by: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReleaseException {
    pub content: String,
    pub reason: String,
    pub approved_by: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StoryStates {
    build_eligibility: String,
    entries: Vec<StoryStateEntry>,
}

#[derive(Debug, Deserialize)]
struct StoryStateEntry {
    logical_id: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct StoryReview {
    entries: Vec<StoryReviewEntry>,
}

#[derive(Debug, Deserialize)]
struct StoryReviewEntry {
    logical_id: String,
    decision: String,
}

#[derive(Debug, Serialize)]
pub struct ScopeAssessment {
    pub declared_scope: String,
    pub units: Vec<ReviewUnit>,
    pub story_status_counts: BTreeMap<String, usize>,
    pub story_pending_human_decisions: Vec<String>,
    pub release_exceptions: Vec<ReleaseException>,
    /// Every reason the current inputs cannot become a release build.
    pub release_blockers: Vec<String>,
}

pub struct Inputs<'a> {
    pub rom_path: &'a Path,
    pub rom: &'a [u8],
    pub policy: Policy,
    pub out_root: &'a Path,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub policy: Policy,
    pub source_path: String,
    pub source_sha256: String,
    pub output_rom: String,
    pub output_sha256: String,
    pub output_crc32: String,
    pub bps: String,
    pub bps_sha256: String,
    pub bps_bytes: usize,
    pub bps_source_crc32: String,
    pub bps_target_crc32: String,
    pub bps_replay_matches_output: bool,
    pub marker: Option<String>,
    pub marker_verified: bool,
    pub input_digest: String,
    pub changed_bytes: usize,
    pub diff_confined: bool,
    pub integrated_stage_sha256: String,
    pub layout_screens: Vec<String>,
    pub preview_decision: Option<PreviewDecision>,
    pub scope: ScopeAssessment,
}

pub fn build(inputs: &Inputs<'_>) -> Result<Report> {
    let source_sha256 = sha256(inputs.rom);
    if source_sha256 != crate::story_poc::TARGET_SHA256 {
        bail!("production input is not the supported Remix JP ROM: SHA-256 {source_sha256}");
    }
    let scope = assess_scope()?;
    if inputs.policy == Policy::Release && !scope.release_blockers.is_empty() {
        bail!(
            "release policy rejected {} blocker(s):\n- {}",
            scope.release_blockers.len(),
            scope.release_blockers.join("\n- ")
        );
    }
    let preview_decision = match inputs.policy {
        Policy::Preview => Some(read_preview_decision()?),
        _ => None,
    };
    let input_digest = input_digest()?;

    let (out_dir, stem) = match &preview_decision {
        Some(decision) => (
            inputs.out_root.join(format!("v{}", decision.version)),
            format!("puyopuyo2_remix_kr_v{}", decision.version),
        ),
        None => (
            inputs.out_root.join(inputs.policy.dir_name()),
            format!("puyopuyo2_remix_kr_{}", inputs.policy.dir_name()),
        ),
    };
    let layout_dir = out_dir.join("layout");
    let mut layout_screens = Vec::new();
    for screen in crate::remix_layout_spec::SCREENS {
        screen.materialize(inputs.rom, &layout_dir.join(screen.name))?;
        layout_screens.push(screen.name.to_owned());
    }

    let (integrated, _report, _encoding) = compose(inputs, &layout_dir)?;
    let integrated_stage_sha256 = sha256(&integrated);

    let mut output = integrated.clone();
    let marker = match inputs.policy {
        Policy::Release | Policy::Preview => None,
        policy => {
            let text = marker_text(policy, &input_digest);
            write_marker(inputs.rom, &integrated, &mut output, &text)?;
            Some(text)
        }
    };
    crate::rom::fix_checksum(&mut output)?;
    let marker_verified = verify_marker(&output, marker.as_deref())?;

    let diff_confined = inputs
        .rom
        .iter()
        .zip(&output)
        .zip(&integrated)
        .enumerate()
        .all(|(offset, ((source, out), stage))| {
            source == out
                || source != stage
                || MARKER_RANGE.contains(&offset)
                || CHECKSUM_RANGE.contains(&offset)
        });
    if !diff_confined {
        bail!("production diff escaped the integrated writers, marker and checksum");
    }
    let changed_bytes = inputs
        .rom
        .iter()
        .zip(&output)
        .filter(|(before, after)| before != after)
        .count();

    let bps = crate::bps::generate_verified(inputs.rom, &output)
        .map_err(|error| anyhow::anyhow!(error))?;
    let replay = crate::bps::apply(inputs.rom, &bps).map_err(|error| anyhow::anyhow!(error))?;
    let bps_replay_matches_output = replay == output;
    if !bps_replay_matches_output {
        bail!("BPS replay does not reproduce the production ROM");
    }
    let footer = bps.len() - 12;
    let bps_source_crc32 = u32::from_le_bytes(bps[footer..footer + 4].try_into()?);
    let bps_target_crc32 = u32::from_le_bytes(bps[footer + 4..footer + 8].try_into()?);

    std::fs::create_dir_all(&out_dir)?;
    let rom_path = out_dir.join(format!("{stem}.sfc"));
    let bps_path = out_dir.join(format!("{stem}.bps"));
    std::fs::write(&rom_path, &output)?;
    std::fs::write(&bps_path, &bps)?;

    let report = Report {
        policy: inputs.policy,
        source_path: inputs.rom_path.display().to_string(),
        source_sha256,
        output_rom: rom_path.display().to_string(),
        output_sha256: sha256(&output),
        output_crc32: format!("{:08x}", crc32fast::hash(&output)),
        bps: bps_path.display().to_string(),
        bps_sha256: sha256(&bps),
        bps_bytes: bps.len(),
        bps_source_crc32: format!("{bps_source_crc32:08x}"),
        bps_target_crc32: format!("{bps_target_crc32:08x}"),
        bps_replay_matches_output,
        marker,
        marker_verified,
        input_digest,
        changed_bytes,
        diff_confined,
        integrated_stage_sha256,
        layout_screens,
        preview_decision,
        scope,
    };
    std::fs::write(
        out_dir.join(format!("{stem}_report.json")),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(report)
}

fn compose(
    inputs: &Inputs<'_>,
    layout_dir: &Path,
) -> Result<(Vec<u8>, crate::integrated_poc::IntegratedPocReport, String)> {
    let read = |path: &str| {
        std::fs::read(path).with_context(|| format!("read committed production input {path}"))
    };
    let galmuri = read("assets/fonts/galmuri11.ttf")?;
    let story_font = read(crate::story_poc::STORY_TTF_PATH)?;
    let galmuri_bold = read("assets/fonts/galmuri11_bold.ttf")?;
    let bmjua = read("assets/fonts/bmjua.ttf")?;
    let maplestory_bold = read("assets/fonts/maplestory_bold.ttf")?;
    let shock = read("assets/ending_graphics/true_ending_shock.png")?;
    let translation = PathBuf::from(STORY_TRANSLATION_PATH);
    let port_map = PathBuf::from("assets/translations/story_port_map.json");
    let terms = PathBuf::from("assets/translations/story_terms.tsv");
    let style = PathBuf::from("assets/translations/story_style.md");
    let mode_assets = PathBuf::from("assets/menu_graphics/mode_labels");
    let remix_assets = PathBuf::from("assets/menu_graphics/remix_labels");
    let tokoton_play = PathBuf::from("assets/menu_graphics/tokoton_play/play.png");
    let tokoton_demo = PathBuf::from("assets/menu_graphics/tokoton_play/demonstration.png");
    let screen = |layout: &crate::remix_layout_spec::ScreenLayout| layout_dir.join(layout.name);
    let mode_select = screen(&crate::remix_layout_spec::MODE_SELECT);
    let solo_type = screen(&crate::remix_layout_spec::SOLO_TYPE);
    let tokoton_option = screen(&crate::remix_layout_spec::TOKOTON_OPTION);
    let tokoton_choice = screen(&crate::remix_layout_spec::TOKOTON_CHOICE);
    let easy_course = screen(&crate::remix_layout_spec::EASY_COURSE);
    let easy_course_assets = [
        PathBuf::from("assets/menu_graphics/easy_courses/beginner.png"),
        PathBuf::from("assets/menu_graphics/easy_courses/practiced.png"),
        PathBuf::from("assets/menu_graphics/easy_courses/graduation.png"),
    ];
    let continue_asset = PathBuf::from("assets/menu_graphics/game_over/continue.png");
    let bmjua_path = "assets/fonts/bmjua.ttf".to_owned();

    crate::integrated_poc::build(&crate::integrated_poc::IntegratedPocInputs {
        source_path: inputs.rom_path.display().to_string(),
        story: crate::story_poc::StoryPocInputs {
            rom: inputs.rom,
            translation_path: &translation,
            port_map_path: &port_map,
            terms_path: &terms,
            style_path: &style,
            ttf_data: &story_font,
            ttf_size: crate::story_poc::STORY_TTF_PX,
            opponent_prompt_ttf_path: &bmjua_path,
            opponent_prompt_ttf_data: &bmjua,
            opponent_prompt_ttf_size: 15.0,
        },
        demo_title_ttf_path: crate::story_poc::STORY_TTF_PATH.to_owned(),
        demo_title_ttf_data: &story_font,
        demo_title_font_px: crate::story_poc::STORY_TTF_PX,
        mode_assets_dir: &mode_assets,
        remix_assets_dir: &remix_assets,
        mode_select_dump: &mode_select,
        solo_type_dump: &solo_type,
        tokoton_option_dump: &tokoton_option,
        patched_mode_runtime_dump: None,
        patched_remix_runtime_dump: None,
        tokoton_play_asset: &tokoton_play,
        tokoton_demonstration_asset: &tokoton_demo,
        tokoton_choice_dump: &tokoton_choice,
        patched_tokoton_runtime_dump: None,
        ranking_ending: crate::remix_ranking_ending_graphics::Inputs {
            exclamation_ttf_path: "assets/fonts/maplestory_bold.ttf".to_owned(),
            exclamation_ttf_data: &maplestory_bold,
            exclamation_font_px: 14.0,
            shock_asset_path: "assets/ending_graphics/true_ending_shock.png".to_owned(),
            shock_asset_data: &shock,
            kaakun_ttf_path: "assets/fonts/galmuri11_bold.ttf".to_owned(),
            kaakun_ttf_data: &galmuri_bold,
            kaakun_font_px: 12.0,
        },
        course_continue: crate::remix_course_continue_graphics::Inputs {
            easy_course_assets: [
                &easy_course_assets[0],
                &easy_course_assets[1],
                &easy_course_assets[2],
            ],
            easy_course_layout: &easy_course,
            continue_asset: &continue_asset,
        },
        caption_marker: crate::remix_caption_marker_graphics::Inputs {
            galmuri_ttf_data: &galmuri,
            galmuri_bold_ttf_data: &galmuri_bold,
        },
        output_path: format!("production:{}", inputs.policy),
    })
}

pub fn assess_scope() -> Result<ScopeAssessment> {
    let registry: ReviewRegistry = serde_json::from_slice(
        &std::fs::read(REVIEW_STATE_PATH).with_context(|| format!("read {REVIEW_STATE_PATH}"))?,
    )
    .with_context(|| format!("parse {REVIEW_STATE_PATH}"))?;
    let story: StoryStates = serde_json::from_slice(&std::fs::read(STORY_TRANSLATION_PATH)?)?;
    let review: StoryReview = serde_json::from_slice(&std::fs::read(STORY_REVIEW_PATH)?)?;
    assess(&registry, &story, &review)
}

fn assess(
    registry: &ReviewRegistry,
    story: &StoryStates,
    review: &StoryReview,
) -> Result<ScopeAssessment> {
    if registry.schema_version != 1 {
        bail!(
            "unsupported review_state schema {}",
            registry.schema_version
        );
    }
    let ids = registry
        .units
        .iter()
        .map(|unit| unit.unit_id.as_str())
        .collect::<Vec<_>>();
    if ids != DECLARED_UNITS {
        bail!("review_state units must list exactly the declared production units in order");
    }
    for unit in &registry.units {
        let state_ok = registry.state_values.contains(&unit.review_state)
            || (unit.unit_id == "story_bank_17" && unit.review_state == "entry_level");
        if !state_ok || !registry.runtime_qa_values.contains(&unit.runtime_qa) {
            bail!(
                "unit {} has an unknown review or runtime-QA state",
                unit.unit_id
            );
        }
    }
    if story.entries.len() != 273
        || review.entries.len() != story.entries.len()
        || story
            .entries
            .iter()
            .zip(&review.entries)
            .any(|(entry, review)| entry.logical_id != review.logical_id)
    {
        bail!("story translation and review record do not cover the same 273 entries");
    }

    let mut blockers = Vec::new();
    let mut story_status_counts = BTreeMap::new();
    for entry in &story.entries {
        *story_status_counts.entry(entry.status.clone()).or_insert(0) += 1;
    }
    let not_eligible = story
        .entries
        .iter()
        .filter(|entry| entry.status != "distribution_eligible")
        .count();
    if not_eligible > 0 {
        blockers.push(format!(
            "story_bank_17: {not_eligible}/273 entries are not distribution_eligible"
        ));
    }
    if story.build_eligibility != "reviewed_candidate" {
        blockers.push(format!(
            "story_bank_17: file build_eligibility is {}",
            story.build_eligibility
        ));
    }
    let story_pending_human_decisions = review
        .entries
        .iter()
        .filter(|entry| entry.decision == "pending_human")
        .map(|entry| entry.logical_id.clone())
        .collect::<Vec<_>>();
    if !story_pending_human_decisions.is_empty() {
        blockers.push(format!(
            "story_bank_17: {} human review decisions are pending",
            story_pending_human_decisions.len()
        ));
    }
    for unit in &registry.units {
        if unit.unit_id != "story_bank_17" && unit.review_state != "distribution_eligible" {
            blockers.push(format!(
                "{}: review_state is {}",
                unit.unit_id, unit.review_state
            ));
        }
        if unit.runtime_qa != "passed" {
            blockers.push(format!(
                "{}: runtime_qa is {}",
                unit.unit_id, unit.runtime_qa
            ));
        }
        if unit.approved_by.as_deref().is_none_or(str::is_empty) {
            blockers.push(format!("{}: no approving human recorded", unit.unit_id));
        }
    }
    for exception in &registry.release_exceptions {
        if exception.approved_by.as_deref().is_none_or(str::is_empty) {
            blockers.push(format!(
                "release exception '{}' has no approving human",
                exception.content
            ));
        }
    }
    Ok(ScopeAssessment {
        declared_scope: registry.declared_scope.clone(),
        units: registry.units.clone(),
        story_status_counts,
        story_pending_human_decisions,
        release_exceptions: registry.release_exceptions.clone(),
        release_blockers: blockers,
    })
}

/// Digest of the review-state inputs that decide build eligibility.
fn input_digest() -> Result<String> {
    let mut hasher = Sha256::new();
    for path in [REVIEW_STATE_PATH, STORY_TRANSLATION_PATH, STORY_REVIEW_PATH] {
        hasher.update(std::fs::read(path).with_context(|| format!("read {path}"))?);
    }
    if let Ok(decision) = std::fs::read(PREVIEW_DECISION_PATH) {
        hasher.update(decision);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn marker_text(policy: Policy, input_digest: &str) -> String {
    format!(
        "{MARKER_PREFIX}{} NOT-FOR-DISTRIBUTION {}",
        policy.tag(),
        &input_digest[..16]
    )
}

fn write_marker(source: &[u8], stage: &[u8], output: &mut [u8], text: &str) -> Result<()> {
    if text.len() > MARKER_RANGE.len() || !text.is_ascii() {
        bail!("production marker does not fit its ASCII range");
    }
    if source[MARKER_RANGE].iter().any(|&byte| byte != 0xFF)
        || stage[MARKER_RANGE].iter().any(|&byte| byte != 0xFF)
    {
        bail!("production marker range is not untouched 0xFF in the source and composed stage");
    }
    let mut bytes = vec![b' '; MARKER_RANGE.len()];
    bytes[..text.len()].copy_from_slice(text.as_bytes());
    output[MARKER_RANGE].copy_from_slice(&bytes);
    Ok(())
}

/// Read the marker range back from the final ROM.
fn verify_marker(output: &[u8], expected: Option<&str>) -> Result<bool> {
    let range = &output[MARKER_RANGE];
    match expected {
        None => {
            if range.iter().any(|&byte| byte != 0xFF) {
                bail!("release ROM carries data in the marker range");
            }
        }
        Some(text) => {
            let stored = std::str::from_utf8(range)
                .context("marker range is not ASCII")?
                .trim_end_matches(' ');
            if stored != text || !stored.contains("NOT-FOR-DISTRIBUTION") {
                bail!("final ROM marker differs from the policy marker");
            }
        }
    }
    Ok(true)
}

fn sha256(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(id: &str, state: &str, qa: &str, approver: Option<&str>) -> ReviewUnit {
        ReviewUnit {
            unit_id: id.to_owned(),
            kind: "text".to_owned(),
            sources: Vec::new(),
            review_state: state.to_owned(),
            runtime_qa: qa.to_owned(),
            approved_by: approver.map(str::to_owned),
            notes: None,
        }
    }

    fn registry(state: &str, qa: &str, approver: Option<&str>) -> ReviewRegistry {
        ReviewRegistry {
            schema_version: 1,
            declared_scope: "test".to_owned(),
            state_values: [
                "untranslated",
                "in_progress",
                "needs_review",
                "needs_human_review",
                "distribution_eligible",
            ]
            .map(str::to_owned)
            .to_vec(),
            runtime_qa_values: ["pending", "partial", "passed"].map(str::to_owned).to_vec(),
            units: DECLARED_UNITS
                .iter()
                .map(|id| {
                    if *id == "story_bank_17" {
                        unit(id, "entry_level", qa, approver)
                    } else {
                        unit(id, state, qa, approver)
                    }
                })
                .collect(),
            release_exceptions: Vec::new(),
        }
    }

    fn story(status: &str, eligibility: &str) -> (StoryStates, StoryReview) {
        let ids = (0..273)
            .map(|index| format!("story_{index:03}"))
            .collect::<Vec<_>>();
        (
            StoryStates {
                build_eligibility: eligibility.to_owned(),
                entries: ids
                    .iter()
                    .map(|id| StoryStateEntry {
                        logical_id: id.clone(),
                        status: status.to_owned(),
                    })
                    .collect(),
            },
            StoryReview {
                entries: ids
                    .iter()
                    .map(|id| StoryReviewEntry {
                        logical_id: id.clone(),
                        decision: "ok".to_owned(),
                    })
                    .collect(),
            },
        )
    }

    #[test]
    fn unapproved_units_block_release_with_reasons() {
        let (states, review) = story("needs_review", "poc_only_needs_review");
        let scope = assess(&registry("needs_review", "partial", None), &states, &review).unwrap();
        assert!(
            scope
                .release_blockers
                .iter()
                .any(|reason| reason.contains("273/273"))
        );
        assert!(
            scope
                .release_blockers
                .iter()
                .any(|reason| reason.starts_with("ending_text: review_state"))
        );
        assert!(
            scope
                .release_blockers
                .iter()
                .any(|reason| reason.contains("no approving human"))
        );
    }

    #[test]
    fn fully_approved_inputs_have_no_release_blockers() {
        let (states, review) = story("distribution_eligible", "reviewed_candidate");
        let scope = assess(
            &registry("distribution_eligible", "passed", Some("maintainer")),
            &states,
            &review,
        )
        .unwrap();
        assert!(
            scope.release_blockers.is_empty(),
            "{:?}",
            scope.release_blockers
        );
    }

    #[test]
    fn pending_human_decision_and_unapproved_exception_block_release() {
        let (states, mut review) = story("distribution_eligible", "reviewed_candidate");
        review.entries[39].decision = "pending_human".to_owned();
        let mut registry = registry("distribution_eligible", "passed", Some("maintainer"));
        registry.release_exceptions.push(ReleaseException {
            content: "COMPILE logo".to_owned(),
            reason: "branding".to_owned(),
            approved_by: None,
        });
        let scope = assess(&registry, &states, &review).unwrap();
        assert_eq!(scope.release_blockers.len(), 2);
    }

    #[test]
    fn registry_must_list_exactly_the_declared_units() {
        let (states, review) = story("needs_review", "poc_only_needs_review");
        let mut registry = registry("needs_review", "partial", None);
        registry.units.pop();
        assert!(assess(&registry, &states, &review).is_err());
    }

    #[test]
    fn preview_decision_needs_semver_owner_and_limitations() {
        let ok = PreviewDecision {
            version: "0.1.0".to_owned(),
            approved_by: "owner".to_owned(),
            approved_on: "2026-10-01".to_owned(),
            decision: "publish preview".to_owned(),
            known_limitations: vec!["unreviewed text".to_owned()],
        };
        assert!(validate_preview_decision(&ok).is_ok());
        let mut bad = ok.clone();
        bad.version = "0.1".to_owned();
        assert!(validate_preview_decision(&bad).is_err());
        let mut bad = ok.clone();
        bad.approved_by = " ".to_owned();
        assert!(validate_preview_decision(&bad).is_err());
        let mut bad = ok;
        bad.known_limitations.clear();
        assert!(validate_preview_decision(&bad).is_err());
    }

    #[test]
    fn marker_round_trips_and_release_range_must_stay_blank() {
        let source = vec![0xFFu8; 0x20_0000];
        let stage = source.clone();
        let mut output = source.clone();
        let text = marker_text(Policy::PreRelease, &"ab".repeat(32));
        write_marker(&source, &stage, &mut output, &text).unwrap();
        assert!(verify_marker(&output, Some(&text)).unwrap());
        assert!(verify_marker(&output, None).is_err());
        assert!(verify_marker(&source, None).unwrap());
        let mut dirty = source.clone();
        dirty[MARKER_RANGE.start] = 0;
        assert!(write_marker(&dirty, &stage, &mut output.clone(), &text).is_err());
    }

    #[test]
    #[ignore = "requires the Remix JP ROM and every build input under assets/"]
    fn pre_release_build_replays_bps_and_release_is_rejected() {
        let rom_path = Path::new("roms/Super Puyo Puyo Tsuu Remix (Japan).sfc");
        let rom = std::fs::read(rom_path).unwrap();
        let out_root = Path::new("out/release-test");
        let report = build(&Inputs {
            rom_path,
            rom: &rom,
            policy: Policy::PreRelease,
            out_root,
        })
        .unwrap();
        assert!(report.bps_replay_matches_output && report.marker_verified);
        assert!(report.diff_confined);
        let error = build(&Inputs {
            rom_path,
            rom: &rom,
            policy: Policy::Release,
            out_root,
        })
        .unwrap_err();
        assert!(error.to_string().contains("release policy rejected"));
    }
}
