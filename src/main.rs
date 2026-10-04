mod bps;
mod combined_menu_poc;
mod combined_ui_poc;
mod demo_explanation;
mod effect_glyphs;
mod ending_graphics;
mod ending_text;
mod font_gen;
mod font_probe;
mod game_over_continue;
mod gameplay_pause;
mod gameplay_rensa;
mod gameplay_sousai;
mod gameplay_zenkeshi;
mod generated_lettering;
mod integrated_poc;
mod mouth_sign;
mod multi_ui_text;
mod opponent_prompt;
mod option_help;
mod ranking_graphics;
mod remix_caption_marker_graphics;
mod remix_course_continue_graphics;
mod remix_gameplay_text;
mod remix_layout_spec;
mod remix_menu_graphics;
mod remix_mode_labels;
mod remix_mode_prompt;
mod remix_production;
mod remix_ranking_ending_graphics;
mod remix_tokoton_play;
mod resource_inventory;
mod rom;
mod rule_editor;
mod sample_select_text;
mod snes_asm;
mod snes_lz;
mod story_codec;
mod story_font;
mod story_poc;
mod story_probe;
mod surface_audit;
#[cfg(test)]
mod test_input;
mod tokoton_difficulty;
mod translation_port;
mod two_player_rules;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(version, about = "SNES Super Puyo Puyo Tsuu Remix KR patch tooling")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// Inputs for the ranking-title and true-ending banner stage.
#[derive(Debug, Args)]
struct RankingEndingArgs {
    #[arg(long, default_value = "assets/fonts/maplestory_bold.ttf")]
    ending_exclamation_ttf: PathBuf,
    #[arg(long, default_value_t = 14.0)]
    ending_exclamation_ttf_size: f32,
    #[arg(long, default_value = "assets/ending_graphics/true_ending_shock.png")]
    ending_shock_asset: PathBuf,
    #[arg(long, default_value = "assets/fonts/galmuri11_bold.ttf")]
    ending_kaakun_ttf: PathBuf,
    #[arg(long, default_value_t = 12.0)]
    ending_kaakun_ttf_size: f32,
}

/// Inputs for the easy course sign and game-over continue stage.
#[derive(Debug, Args)]
struct CourseContinueArgs {
    #[arg(long, default_value = "assets/menu_graphics/easy_courses")]
    easy_course_assets_dir: PathBuf,
    #[arg(
        long,
        default_value = "out/evidence/menu/remix_easy_course_select_original"
    )]
    easy_course_dump: PathBuf,
    #[arg(long, default_value = "assets/menu_graphics/game_over/continue.png")]
    continue_asset: PathBuf,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print hashes and SNES header candidates for one ROM.
    Info {
        #[arg(long)]
        rom: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Compare the Remix ROM with a related ROM without treating either layout as authoritative.
    Compare {
        #[arg(long)]
        base: PathBuf,
        #[arg(long)]
        target: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Scan for blocks compatible with the known Tsuu story byte grammar.
    StoryScan {
        #[arg(long)]
        rom: PathBuf,
        /// Optional physical LoROM bank, such as 0x17. All banks are scanned by default.
        #[arg(long)]
        bank: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Compare story candidates in two same-engine ROMs without assuming fixed addresses.
    StoryCompare {
        #[arg(long)]
        base: PathBuf,
        #[arg(long)]
        target: PathBuf,
        #[arg(long, default_value = "0x17")]
        bank: String,
        #[arg(long)]
        json: bool,
    },
    /// Compare the two consecutive Tsuu-format story font streams.
    FontCompare {
        #[arg(long)]
        base: PathBuf,
        #[arg(long)]
        target: PathBuf,
        #[arg(long, default_value = "0x40136")]
        base_font_pc: String,
        #[arg(long)]
        json: bool,
    },
    /// Write the verified Tsuu-to-Remix story identity and address map.
    StoryPortMap {
        #[arg(long)]
        base: PathBuf,
        #[arg(long)]
        target: PathBuf,
        #[arg(long, default_value = "0x17")]
        bank: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Port the Tsuu Korean story work text onto logical Remix IDs for a PoC.
    StoryTranslationPort {
        #[arg(long, default_value = "assets/translations/story_port_map.json")]
        port_map: PathBuf,
        #[arg(long)]
        source_root: PathBuf,
        #[arg(long, default_value = "assets/translations/story_ko.json")]
        out: PathBuf,
    },
    /// Build the full 273-entry Korean story as a non-release Remix PoC.
    StoryKrPoc {
        #[arg(long)]
        rom: PathBuf,
        #[arg(long, default_value = "assets/translations/story_ko.json")]
        translation: PathBuf,
        #[arg(long, default_value = "assets/translations/story_port_map.json")]
        port_map: PathBuf,
        #[arg(long, default_value = "assets/translations/story_terms.tsv")]
        terms: PathBuf,
        #[arg(long, default_value = "assets/translations/story_style.md")]
        style: PathBuf,
        #[arg(long, default_value = "assets/fonts/galmuri14.ttf")]
        ttf: PathBuf,
        #[arg(long, default_value_t = 15.0)]
        ttf_size: f32,
        #[arg(long, default_value = "assets/fonts/bmjua.ttf")]
        opponent_prompt_ttf: PathBuf,
        #[arg(long, default_value_t = 15.0)]
        opponent_prompt_ttf_size: f32,
        #[arg(
            long,
            default_value = "out/builds/poc/puyopuyo2_remix_story_kr_poc.sfc"
        )]
        out: PathBuf,
        #[arg(long, default_value = "out/builds/poc/story_ko_encoding.tsv")]
        encoding_out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Audit the Remix-specific 4x16 option-help pointer population.
    OptionHelpAudit {
        #[arg(long)]
        rom: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Classify known Tsuu compressed UI/graphics surfaces in the Remix ROM.
    SurfaceAudit {
        #[arg(long)]
        base: PathBuf,
        #[arg(long)]
        target: PathBuf,
        /// Optional emucap dump captured on the Remix mode-selection screen.
        #[arg(long)]
        runtime_dump: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Inventory bank-local asset tables and classify Remix resource streams.
    ResourceInventory {
        #[arg(long)]
        base: PathBuf,
        #[arg(long)]
        target: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Render target-only (and optionally changed) resource streams as 4bpp atlases.
    ResourceAtlases {
        #[arg(long)]
        base: PathBuf,
        #[arg(long)]
        target: PathBuf,
        #[arg(long, default_value = "out/evidence/resources/atlases")]
        out_dir: PathBuf,
        #[arg(long)]
        include_changed: bool,
        #[arg(long, default_value_t = 2)]
        scale: usize,
        #[arg(long)]
        json: bool,
    },
    /// Match every table-linked target resource against a frozen SNES memory dump.
    ResourceRuntimeMatch {
        #[arg(long)]
        base: PathBuf,
        #[arg(long)]
        target: PathBuf,
        /// emucap dump directory containing vram.bin and wram.bin.
        #[arg(long)]
        runtime_dump: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Normalize the four generated Remix-only Korean menu labels.
    RemixMenuAssets {
        #[arg(long, default_value = "assets/menu_graphics/remix_labels")]
        source_dir: PathBuf,
        #[arg(long, default_value = "assets/menu_graphics/remix_labels")]
        out_dir: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Normalize a generated Remix-only 2-mode label to its 49x40 sign contract.
    RemixSoloTsuuAsset {
        #[arg(long)]
        source: PathBuf,
        #[arg(long, default_value = "out/evidence/menu/remix_solo_type")]
        runtime_dump: PathBuf,
        #[arg(long, default_value = "assets/menu_graphics/mode_labels/solo_tsuu.png")]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Normalize generated true-ending 쿠궁! artwork into the verified shock cells.
    RemixEndingShockAsset {
        #[arg(long)]
        source: PathBuf,
        #[arg(long, default_value = "assets/ending_graphics/true_ending_shock.png")]
        out: PathBuf,
        #[arg(
            long,
            default_value = "out/work/ending_graphics/true_ending_shock_preview.png"
        )]
        preview: PathBuf,
        #[arg(long, default_value_t = 0.35)]
        coverage_threshold: f32,
        #[arg(long)]
        json: bool,
    },
    /// Normalize one generated Remix-only Tokoton choice label.
    RemixTokotonPlayAsset {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        text: String,
        #[arg(
            long,
            default_value = "out/evidence/menu/remix_tokoton_play_select_original"
        )]
        runtime_dump: PathBuf,
        #[arg(long, default_value = "assets/menu_graphics/tokoton_play/play.png")]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Remap seven inherited labels plus the Remix-only 2-mode label.
    RemixModeLabelsKrPoc {
        #[arg(long)]
        rom: PathBuf,
        #[arg(long, default_value = "assets/menu_graphics/mode_labels")]
        assets_dir: PathBuf,
        #[arg(long, default_value = "out/evidence/menu/remix_mode_select")]
        mode_select_dump: PathBuf,
        #[arg(long, default_value = "out/evidence/menu/remix_solo_type")]
        solo_type_dump: PathBuf,
        #[arg(long, default_value = "out/evidence/menu/remix_tokoton_option")]
        tokoton_option_dump: PathBuf,
        /// Optional frozen mode-select dump captured from this patched PoC.
        #[arg(long)]
        patched_runtime_dump: Option<PathBuf>,
        #[arg(
            long,
            default_value = "out/builds/poc/puyopuyo2_remix_mode_labels_kr_poc.sfc"
        )]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Insert the generated Korean mode-selection prompt into its relocated CHR stream.
    RemixModePromptKrPoc {
        #[arg(long)]
        rom: PathBuf,
        #[arg(long, default_value = "out/evidence/menu/remix_mode_select")]
        runtime_dump: PathBuf,
        #[arg(long)]
        patched_runtime_dump: Option<PathBuf>,
        #[arg(
            long,
            default_value = "out/builds/poc/puyopuyo2_remix_mode_prompt_kr_poc.sfc"
        )]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Insert the prompt plus all twelve inherited and Remix-only Korean labels.
    RemixAllMenuLabelsKrPoc {
        #[arg(long)]
        rom: PathBuf,
        #[arg(long, default_value = "assets/menu_graphics/mode_labels")]
        mode_assets_dir: PathBuf,
        #[arg(long, default_value = "assets/menu_graphics/remix_labels")]
        remix_assets_dir: PathBuf,
        #[arg(long, default_value = "out/evidence/menu/remix_mode_select")]
        mode_select_dump: PathBuf,
        #[arg(long, default_value = "out/evidence/menu/remix_solo_type")]
        solo_type_dump: PathBuf,
        #[arg(long, default_value = "out/evidence/menu/remix_tokoton_option")]
        tokoton_option_dump: PathBuf,
        #[arg(long)]
        patched_mode_runtime_dump: Option<PathBuf>,
        #[arg(long)]
        patched_remix_runtime_dump: Option<PathBuf>,
        #[arg(
            long,
            default_value = "out/builds/poc/puyopuyo2_remix_all_menu_labels_kr_poc.sfc"
        )]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Insert Korean 연쇄, 상쇄, 싹쓸이!, and 휴식중 gameplay graphics.
    RemixGameplayTextKrPoc {
        #[arg(long)]
        rom: PathBuf,
        #[arg(
            long,
            default_value = "out/builds/poc/puyopuyo2_remix_gameplay_text_kr_poc.sfc"
        )]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Insert separate generated Korean 놀기 and 시범 labels into Tokoton choices.
    RemixTokotonPlayKrPoc {
        #[arg(long)]
        rom: PathBuf,
        #[arg(long, default_value = "assets/menu_graphics/tokoton_play/play.png")]
        play_asset: PathBuf,
        #[arg(
            long,
            default_value = "assets/menu_graphics/tokoton_play/demonstration.png"
        )]
        demonstration_asset: PathBuf,
        #[arg(
            long,
            default_value = "out/evidence/menu/remix_tokoton_play_select_original"
        )]
        runtime_dump: PathBuf,
        #[arg(long)]
        patched_runtime_dump: Option<PathBuf>,
        #[arg(
            long,
            default_value = "out/builds/poc/puyopuyo2_remix_tokoton_play_kr_poc.sfc"
        )]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Decompress one SNES LZ stream from any ROM (for example a PoC build) to a
    /// raw file for inspection.
    LzDecode {
        #[arg(long)]
        rom: PathBuf,
        /// PC offset of the stream, decimal or 0x-prefixed hex.
        #[arg(long)]
        pc: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Preview a mouth-sign stream: without assets erase the Japanese lettering
    /// only; with one 48x40 asset per slot composite the generated labels.
    RemixSignPreview {
        #[arg(long)]
        rom: PathBuf,
        /// `tokoton_choices` or `easy_courses`.
        #[arg(long)]
        surface: String,
        /// Generated label PNGs in slot order (repeat the flag).
        #[arg(long = "asset")]
        assets: Vec<PathBuf>,
        /// Defaults to the verified original runtime dump of the surface.
        #[arg(long)]
        runtime_dump: Option<PathBuf>,
        /// Defaults to `out/work/sign_preview/<surface>.png`.
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Render the original game-over continue line above the rebuilt one without writing a ROM.
    RemixContinuePreview {
        #[arg(long)]
        rom: PathBuf,
        #[arg(long, default_value = "assets/menu_graphics/game_over/continue.png")]
        asset: PathBuf,
        #[arg(long, default_value = "out/work/sign_preview/game_over_continue.png")]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Insert 3/4/5연쇄 and 끝내기 with the Super Puyo Puyo 2 shared-font path.
    RemixSampleSelectKrPoc {
        #[arg(long)]
        rom: PathBuf,
        #[arg(long, default_value = "assets/fonts/galmuri11.ttf")]
        ttf: PathBuf,
        #[arg(long, default_value_t = 12.0)]
        ttf_size: f32,
        #[arg(
            long,
            default_value = "out/builds/poc/puyopuyo2_remix_sample_select_kr_poc.sfc"
        )]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Insert 상대를 / 골라주세요 into the normal-mode opponent prompt.
    RemixOpponentPromptKrPoc {
        #[arg(long)]
        rom: PathBuf,
        #[arg(long, default_value = "assets/fonts/bmjua.ttf")]
        ttf: PathBuf,
        #[arg(long, default_value_t = 15.0)]
        ttf_size: f32,
        #[arg(
            long,
            default_value = "out/builds/poc/puyopuyo2_remix_opponent_prompt_kr_poc.sfc"
        )]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Insert every verified Korean menu and gameplay graphic into one PoC ROM.
    RemixAllUiKrPoc {
        #[arg(long)]
        rom: PathBuf,
        #[arg(long, default_value = "assets/menu_graphics/mode_labels")]
        mode_assets_dir: PathBuf,
        #[arg(long, default_value = "assets/menu_graphics/remix_labels")]
        remix_assets_dir: PathBuf,
        #[arg(long, default_value = "assets/fonts/bmjua.ttf")]
        prompt_ttf: PathBuf,
        #[arg(long, default_value_t = 15.0)]
        opponent_prompt_ttf_size: f32,
        #[arg(long, default_value = "out/evidence/menu/remix_mode_select")]
        mode_select_dump: PathBuf,
        #[arg(long, default_value = "out/evidence/menu/remix_solo_type")]
        solo_type_dump: PathBuf,
        #[arg(long, default_value = "out/evidence/menu/remix_tokoton_option")]
        tokoton_option_dump: PathBuf,
        #[arg(long)]
        patched_mode_runtime_dump: Option<PathBuf>,
        #[arg(long)]
        patched_remix_runtime_dump: Option<PathBuf>,
        #[arg(long, default_value = "assets/menu_graphics/tokoton_play/play.png")]
        tokoton_play_asset: PathBuf,
        #[arg(
            long,
            default_value = "assets/menu_graphics/tokoton_play/demonstration.png"
        )]
        tokoton_demonstration_asset: PathBuf,
        #[arg(
            long,
            default_value = "out/evidence/menu/remix_tokoton_play_select_original"
        )]
        tokoton_choice_dump: PathBuf,
        #[arg(long)]
        patched_tokoton_runtime_dump: Option<PathBuf>,
        #[arg(long, default_value = "assets/fonts/galmuri11.ttf")]
        rensa_ttf: PathBuf,
        #[arg(long, default_value_t = 12.0)]
        sample_ttf_size: f32,
        #[arg(
            long,
            default_value = "out/builds/poc/puyopuyo2_remix_all_ui_kr_poc.sfc"
        )]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Compose story, game explanation, option, menu, Tokoton, and gameplay into one PoC ROM.
    RemixIntegratedKrPoc {
        #[arg(long)]
        rom: PathBuf,
        #[arg(long, default_value = "assets/translations/story_ko.json")]
        translation: PathBuf,
        #[arg(long, default_value = "assets/translations/story_port_map.json")]
        port_map: PathBuf,
        #[arg(long, default_value = "assets/translations/story_terms.tsv")]
        terms: PathBuf,
        #[arg(long, default_value = "assets/translations/story_style.md")]
        style: PathBuf,
        #[arg(long, default_value = "assets/fonts/galmuri14.ttf")]
        story_ttf: PathBuf,
        #[arg(long, default_value_t = 15.0)]
        story_ttf_size: f32,
        #[arg(long, default_value = "assets/fonts/galmuri14.ttf")]
        demo_title_ttf: PathBuf,
        #[arg(long, default_value_t = 15.0)]
        demo_title_ttf_size: f32,
        #[arg(long, default_value = "assets/fonts/bmjua.ttf")]
        prompt_ttf: PathBuf,
        #[arg(long, default_value_t = 15.0)]
        opponent_prompt_ttf_size: f32,
        #[arg(long, default_value = "assets/menu_graphics/mode_labels")]
        mode_assets_dir: PathBuf,
        #[arg(long, default_value = "assets/menu_graphics/remix_labels")]
        remix_assets_dir: PathBuf,
        #[arg(long, default_value = "out/evidence/menu/remix_mode_select")]
        mode_select_dump: PathBuf,
        #[arg(long, default_value = "out/evidence/menu/remix_solo_type")]
        solo_type_dump: PathBuf,
        #[arg(long, default_value = "out/evidence/menu/remix_tokoton_option")]
        tokoton_option_dump: PathBuf,
        #[arg(long)]
        patched_mode_runtime_dump: Option<PathBuf>,
        #[arg(long)]
        patched_remix_runtime_dump: Option<PathBuf>,
        #[arg(long, default_value = "assets/menu_graphics/tokoton_play/play.png")]
        tokoton_play_asset: PathBuf,
        #[arg(
            long,
            default_value = "assets/menu_graphics/tokoton_play/demonstration.png"
        )]
        tokoton_demonstration_asset: PathBuf,
        #[arg(
            long,
            default_value = "out/evidence/menu/remix_tokoton_play_select_original"
        )]
        tokoton_choice_dump: PathBuf,
        #[arg(long)]
        patched_tokoton_runtime_dump: Option<PathBuf>,
        #[command(flatten)]
        ranking_ending: Box<RankingEndingArgs>,
        #[command(flatten)]
        course_continue: Box<CourseContinueArgs>,
        #[arg(
            long,
            default_value = "out/builds/poc/puyopuyo2_remix_integrated_kr_poc.sfc"
        )]
        out: PathBuf,
        #[arg(
            long,
            default_value = "out/builds/poc/integrated_story_ko_encoding.tsv"
        )]
        encoding_out: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Build the Korean ROM and BPS from the Remix JP ROM and committed assets only.
    RemixProductionBuild {
        #[arg(long)]
        rom: PathBuf,
        #[arg(long, value_enum)]
        policy: remix_production::Policy,
        #[arg(long, default_value = "out/release")]
        out_root: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Re-derive the committed menu BG1 layout constants from discovery dumps.
    RemixLayoutSpecCheck {
        #[arg(long)]
        rom: PathBuf,
        #[arg(long, default_value = "out/evidence/menu")]
        dump_root: PathBuf,
    },
    /// Insert the four Remix-only Korean menu labels into a non-release PoC ROM.
    RemixMenuKrPoc {
        #[arg(long)]
        rom: PathBuf,
        #[arg(long, default_value = "assets/menu_graphics/remix_labels")]
        assets_dir: PathBuf,
        /// Optional frozen emucap dump from the patched 3/4-player menu.
        #[arg(long)]
        runtime_dump: Option<PathBuf>,
        #[arg(long, default_value = "out/builds/poc/puyopuyo2_remix_menu_kr_poc.sfc")]
        out: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Info { rom: path, json } => {
            let data = rom::load(&path)?;
            let report = rom::inspect(&path, &data);
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("path: {}", report.path);
                println!("size: {} bytes", report.size);
                println!("copier_header: {}", report.copier_header);
                println!("crc32: {}", report.crc32);
                println!("md5: {}", report.md5);
                println!("sha1: {}", report.sha1);
                println!("sha256: {}", report.sha256);
                for header in report.headers {
                    println!();
                    println!("{} header @ {}", header.kind, header.pc_offset);
                    println!("  title: {}", header.title);
                    println!("  title_hex: {}", header.title_hex);
                    println!(
                        "  map_mode: {} ({})",
                        header.map_mode_hex, header.map_mode_name
                    );
                    println!("  rom_size_code: {}", header.rom_size_code);
                    println!(
                        "  checksum/complement: {}/{} valid_pair={}",
                        header.checksum_hex, header.complement_hex, header.checksum_pair_valid
                    );
                    println!("  plausible: {}", header.plausible);
                }
            }
        }
        Command::Compare { base, target, json } => {
            let base_data = rom::load(&base)?;
            let target_data = rom::load(&target)?;
            let report = rom::compare(&base, &base_data, &target, &target_data)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("base: {}", report.base_path);
                println!("target: {}", report.target_path);
                println!("size: {} bytes", report.size);
                println!("different bytes: {}", report.different_bytes);
                println!("changed ranges: {}", report.changed_ranges);
                println!("identical banks: {}", report.identical_banks.join(", "));
                for bank in report.banks {
                    println!(
                        "bank {} equal {}/{}",
                        bank.bank_hex, bank.equal_bytes, bank.size
                    );
                }
            }
        }
        Command::StoryScan {
            rom: path,
            bank,
            json,
        } => {
            let data = rom::load(&path)?;
            let bank = bank.as_deref().map(parse_u8).transpose()?;
            let report = story_probe::scan(&path, &data, bank)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("path: {}", report.path);
                for bank in report.banks {
                    println!(
                        "bank {} candidates={} accepted={} rejected={} long_pointer_refs={} unknown_controls={:?}",
                        bank.bank_hex,
                        bank.structural_candidates,
                        bank.accepted_blocks,
                        bank.rejected_candidates,
                        bank.long_pointer_references,
                        bank.unknown_controls
                    );
                }
            }
        }
        Command::StoryCompare {
            base,
            target,
            bank,
            json,
        } => {
            let base_data = rom::load(&base)?;
            let target_data = rom::load(&target)?;
            let bank = parse_u8(&bank)?;
            let report = story_probe::compare(&base, &base_data, &target, &target_data, bank)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("bank: {}", report.bank_hex);
                println!("base blocks: {}", report.base_blocks);
                println!("target blocks: {}", report.target_blocks);
                println!(
                    "same-index exact blocks: {}",
                    report.same_index_exact_blocks
                );
                println!(
                    "all blocks same order and bytes: {}",
                    report.all_blocks_same_order_and_bytes
                );
                println!(
                    "target raw matches: unique={} ambiguous={} missing={}",
                    report.target_blocks_with_unique_raw_match,
                    report.target_blocks_with_ambiguous_raw_match,
                    report.target_blocks_without_raw_match
                );
            }
        }
        Command::FontCompare {
            base,
            target,
            base_font_pc,
            json,
        } => {
            let base_data = rom::load(&base)?;
            let target_data = rom::load(&target)?;
            let base_font_pc = parse_usize(&base_font_pc)?;
            let report =
                font_probe::compare(&base, &base_data, &target, &target_data, base_font_pc)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("base font: {}", report.base_font.pc_hex);
                println!("target font: {}", report.target_font.pc_hex);
                println!(
                    "base font decoded differences: {}",
                    report.base_font_decoded_different_bytes
                );
                println!("base extended font: {}", report.base_extended_font.pc_hex);
                println!(
                    "target extended font: {}",
                    report.target_extended_font.pc_hex
                );
                println!(
                    "extended raw/decoded equal: {}/{}",
                    report.extended_raw_equal, report.extended_decoded_equal
                );
            }
        }
        Command::StoryPortMap {
            base,
            target,
            bank,
            out,
        } => {
            let base_data = rom::load(&base)?;
            let target_data = rom::load(&target)?;
            let report = story_probe::port_map(&base_data, &target_data, parse_u8(&bank)?)?;
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut json = serde_json::to_string_pretty(&report)?;
            json.push('\n');
            std::fs::write(&out, json)?;
            println!(
                "wrote {} verified story mappings -> {}",
                report.entries.len(),
                out.display()
            );
        }
        Command::StoryTranslationPort {
            port_map,
            source_root,
            out,
        } => {
            let report = translation_port::port(&port_map, &source_root)?;
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut json = serde_json::to_string_pretty(&report)?;
            json.push('\n');
            std::fs::write(&out, json)?;
            println!(
                "wrote {} ported Korean story entries -> {}",
                report.entries.len(),
                out.display()
            );
        }
        Command::StoryKrPoc {
            rom,
            translation,
            port_map,
            terms,
            style,
            ttf,
            ttf_size,
            opponent_prompt_ttf,
            opponent_prompt_ttf_size,
            out,
            encoding_out,
            json,
        } => {
            let source = rom::load(&rom)?;
            let ttf_data = std::fs::read(&ttf)?;
            let opponent_prompt_ttf_data = std::fs::read(&opponent_prompt_ttf)?;
            let (patched, report, encoding) = story_poc::build(&story_poc::StoryPocInputs {
                rom: &source,
                translation_path: &translation,
                port_map_path: &port_map,
                terms_path: &terms,
                style_path: &style,
                ttf_data: &ttf_data,
                ttf_size,
                opponent_prompt_ttf_path: &opponent_prompt_ttf.display().to_string(),
                opponent_prompt_ttf_data: &opponent_prompt_ttf_data,
                opponent_prompt_ttf_size,
            })?;
            for path in [&out, &encoding_out] {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
            }
            std::fs::write(&out, patched)?;
            std::fs::write(&encoding_out, encoding)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("wrote story PoC ROM -> {}", out.display());
                println!("wrote story encoding -> {}", encoding_out.display());
                println!("output SHA-256: {}", report.output_sha256);
            }
        }
        Command::OptionHelpAudit { rom, json } => {
            let source = rom::load(&rom)?;
            let report = option_help::audit(&source)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "option help: {} active / {} unique / {} placeholder slots",
                    report.active_slots, report.unique_active_targets, report.placeholder_slots
                );
            }
        }
        Command::SurfaceAudit {
            base,
            target,
            runtime_dump,
            json,
        } => {
            let base_data = rom::load(&base)?;
            let target_data = rom::load(&target)?;
            let report = surface_audit::audit(
                &base,
                &base_data,
                &target,
                &target_data,
                runtime_dump.as_deref(),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                for surface in report.surfaces {
                    println!(
                        "{}: {} (base {}, target {})",
                        surface.id, surface.classification, surface.base_pc, surface.target_pc
                    );
                }
            }
        }
        Command::ResourceInventory { base, target, json } => {
            let base_data = rom::load(&base)?;
            let target_data = rom::load(&target)?;
            let report = resource_inventory::compare(&base, &base_data, &target, &target_data)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "base tables/resources: {}/{}",
                    report.base_tables, report.base_resources
                );
                println!(
                    "target tables/resources: {}/{}",
                    report.target_tables, report.target_resources
                );
                println!("raw-exact inherited: {}", report.raw_exact_inherited);
                println!(
                    "decoded-exact inherited: {}",
                    report.decoded_exact_inherited
                );
                println!(
                    "changed logical counterparts: {}",
                    report.changed_logical_counterparts
                );
                println!("target-only candidates: {}", report.target_only_candidates);
                println!("base-only candidates: {}", report.base_only_candidates);
                println!(
                    "caption routes: {} (selector {}, display {} frames)",
                    report.caption_routing.routes.len(),
                    report.caption_routing.selector_lorom,
                    report.caption_routing.wait_frames
                );
                for route in report.caption_routing.routes {
                    println!(
                        "  {}: group {} {} + effect group {} {} ({})",
                        route.id,
                        route.caption_group,
                        route.caption_lorom,
                        route.effect_group,
                        route.effect_lorom,
                        route.condition
                    );
                }
            }
        }
        Command::ResourceAtlases {
            base,
            target,
            out_dir,
            include_changed,
            scale,
            json,
        } => {
            let base_data = rom::load(&base)?;
            let target_data = rom::load(&target)?;
            let report = resource_inventory::extract_atlases(
                &base,
                &base_data,
                &target,
                &target_data,
                &out_dir,
                include_changed,
                scale,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                let output_files = report
                    .atlases
                    .iter()
                    .map(|atlas| 2 + 2 * usize::from(atlas.base_output.is_some()))
                    .sum::<usize>();
                println!(
                    "wrote {} resource atlas entries ({} BMP/bin files) -> {}",
                    report.atlases.len(),
                    output_files,
                    out_dir.display()
                );
            }
        }
        Command::ResourceRuntimeMatch {
            base,
            target,
            runtime_dump,
            json,
        } => {
            let base_data = rom::load(&base)?;
            let target_data = rom::load(&target)?;
            let report = resource_inventory::match_runtime(
                &base,
                &base_data,
                &target,
                &target_data,
                &runtime_dump,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "runtime exact matches: {}/{} target resources",
                    report.matched_target_resources, report.target_resources
                );
                for resource in report.exact_matches {
                    let locations = resource
                        .locations
                        .iter()
                        .map(|location| format!("{}:{}", location.region, location.offset))
                        .collect::<Vec<_>>()
                        .join(", ");
                    println!(
                        "{} {} bytes -> {}",
                        resource.target_pc, resource.decoded_len, locations
                    );
                }
            }
        }
        Command::RemixMenuAssets {
            source_dir,
            out_dir,
            json,
        } => {
            let report = remix_menu_graphics::prepare_assets(&source_dir, &out_dir)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", report.verdict);
                for asset in report.assets {
                    println!("{} {} -> {}", asset.id, asset.dimensions, asset.output);
                }
            }
        }
        Command::RemixSoloTsuuAsset {
            source,
            runtime_dump,
            out,
            json,
        } => {
            let report = remix_mode_labels::prepare_solo_tsuu_asset(&source, &runtime_dump, &out)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", report.verdict);
                println!("wrote 2-mode label -> {}", out.display());
                println!("output SHA-256: {}", report.output_sha256);
            }
        }
        Command::RemixEndingShockAsset {
            source,
            out,
            preview,
            coverage_threshold,
            json,
        } => {
            let report = ending_graphics::prepare_shock_text_asset(
                &source,
                &out,
                &preview,
                coverage_threshold,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", report.verdict);
                println!("wrote 쿠궁! asset -> {}", out.display());
                println!("output SHA-256: {}", report.output_sha256);
            }
        }
        Command::RemixTokotonPlayAsset {
            source,
            text,
            runtime_dump,
            out,
            json,
        } => {
            let report = remix_tokoton_play::prepare_asset(&source, &text, &runtime_dump, &out)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", report.verdict);
                println!("wrote Tokoton play label -> {}", out.display());
                println!("output SHA-256: {}", report.output_sha256);
            }
        }
        Command::RemixModeLabelsKrPoc {
            rom,
            assets_dir,
            mode_select_dump,
            solo_type_dump,
            tokoton_option_dump,
            patched_runtime_dump,
            out,
            json,
        } => {
            let source = rom::load(&rom)?;
            let (patched, report) = remix_mode_labels::build_poc(
                &source,
                rom.display().to_string(),
                &assets_dir,
                &mode_select_dump,
                &solo_type_dump,
                &tokoton_option_dump,
                patched_runtime_dump.as_deref(),
                out.display().to_string(),
            )?;
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&out, patched)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", report.verdict);
                println!("wrote Remix mode-label PoC ROM -> {}", out.display());
                println!("output SHA-256: {}", report.output_sha256);
            }
        }
        Command::RemixAllMenuLabelsKrPoc {
            rom,
            mode_assets_dir,
            remix_assets_dir,
            mode_select_dump,
            solo_type_dump,
            tokoton_option_dump,
            patched_mode_runtime_dump,
            patched_remix_runtime_dump,
            out,
            json,
        } => {
            let source = rom::load(&rom)?;
            let (patched, report) = combined_menu_poc::build(
                &source,
                rom.display().to_string(),
                &mode_assets_dir,
                &remix_assets_dir,
                &mode_select_dump,
                &solo_type_dump,
                &tokoton_option_dump,
                patched_mode_runtime_dump.as_deref(),
                patched_remix_runtime_dump.as_deref(),
                out.display().to_string(),
            )?;
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&out, patched)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", report.verdict);
                println!("wrote combined Remix menu PoC ROM -> {}", out.display());
                println!("output SHA-256: {}", report.output_sha256);
            }
        }
        Command::RemixModePromptKrPoc {
            rom,
            runtime_dump,
            patched_runtime_dump,
            out,
            json,
        } => {
            let source = rom::load(&rom)?;
            let (patched, report) = remix_mode_prompt::build_poc(
                &source,
                rom.display().to_string(),
                &runtime_dump,
                patched_runtime_dump.as_deref(),
                out.display().to_string(),
            )?;
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&out, patched)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", report.verdict);
                println!("wrote Remix mode-prompt PoC ROM -> {}", out.display());
                println!("output SHA-256: {}", report.output_sha256);
            }
        }
        Command::RemixGameplayTextKrPoc { rom, out, json } => {
            let source = rom::load(&rom)?;
            let (patched, report) = remix_gameplay_text::build_poc(
                &source,
                rom.display().to_string(),
                out.display().to_string(),
            )?;
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&out, patched)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", report.verdict);
                println!("wrote Remix gameplay-text PoC ROM -> {}", out.display());
                println!("output SHA-256: {}", report.output_sha256);
            }
        }
        Command::RemixTokotonPlayKrPoc {
            rom,
            play_asset,
            demonstration_asset,
            runtime_dump,
            patched_runtime_dump,
            out,
            json,
        } => {
            let source = rom::load(&rom)?;
            let (patched, report) = remix_tokoton_play::build_poc(
                &source,
                rom.display().to_string(),
                &play_asset,
                &demonstration_asset,
                &runtime_dump,
                patched_runtime_dump.as_deref(),
                out.display().to_string(),
            )?;
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&out, patched)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", report.verdict);
                println!("wrote Tokoton play PoC ROM -> {}", out.display());
                println!("output SHA-256: {}", report.output_sha256);
            }
        }
        Command::LzDecode { rom, pc, out } => {
            let data = std::fs::read(&rom)?;
            let pc = match pc.strip_prefix("0x").or_else(|| pc.strip_prefix("0X")) {
                Some(hex) => usize::from_str_radix(hex, 16)?,
                None => pc.parse()?,
            };
            let block = snes_lz::decompress(&data, pc)?;
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&out, &block.bytes)?;
            println!(
                "0x{pc:06X}: {} compressed -> {} decoded bytes -> {}",
                block.compressed_len,
                block.bytes.len(),
                out.display()
            );
        }
        Command::RemixContinuePreview {
            rom,
            asset,
            out,
            json,
        } => {
            let source = rom::load(&rom)?;
            let report = game_over_continue::preview(
                &source,
                remix_course_continue_graphics::CONTINUE_TEXT,
                &asset,
                &out,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "continue line rebuilt into {} of {} line tiles",
                    report.report.used_line_tiles, report.report.available_line_tiles
                );
                println!("wrote preview -> {}", report.preview_path);
            }
        }
        Command::RemixSignPreview {
            rom,
            surface,
            assets,
            runtime_dump,
            out,
            json,
        } => {
            let source = rom::load(&rom)?;
            let runtime_dump = runtime_dump.unwrap_or_else(|| match surface.as_str() {
                "easy_courses" => {
                    PathBuf::from("out/evidence/menu/remix_easy_course_select_original")
                }
                _ => PathBuf::from("out/evidence/menu/remix_tokoton_play_select_original"),
            });
            let out = out
                .unwrap_or_else(|| PathBuf::from(format!("out/work/sign_preview/{surface}.png")));
            let report =
                remix_tokoton_play::sign_preview(&surface, &source, &runtime_dump, &assets, &out)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", report.verdict);
                println!("wrote side-by-side preview -> {}", out.display());
            }
        }
        Command::RemixSampleSelectKrPoc {
            rom,
            ttf,
            ttf_size,
            out,
            json,
        } => {
            let source = rom::load(&rom)?;
            let ttf_data = std::fs::read(&ttf)?;
            let (patched, report) = sample_select_text::build_poc(
                &source,
                rom.display().to_string(),
                ttf.display().to_string(),
                &ttf_data,
                ttf_size,
                out.display().to_string(),
            )?;
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&out, patched)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", report.verdict);
                println!("wrote sample-select PoC ROM -> {}", out.display());
                println!("output SHA-256: {}", report.output_sha256);
            }
        }
        Command::RemixOpponentPromptKrPoc {
            rom,
            ttf,
            ttf_size,
            out,
            json,
        } => {
            let source = rom::load(&rom)?;
            let ttf_data = std::fs::read(&ttf)?;
            let inputs = opponent_prompt::OpponentPromptKrPocInputs {
                rom: &source,
                source_path: rom.display().to_string(),
                ttf_path: ttf.display().to_string(),
                ttf_data: &ttf_data,
                font_px: ttf_size,
                runtime_vram: None,
                runtime_cram: None,
                output_path: out.display().to_string(),
            };
            let (patched, report) = opponent_prompt::build_opponent_prompt_kr_poc(&inputs)?;
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&out, patched)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", report.verdict);
                println!("wrote opponent-prompt PoC ROM -> {}", out.display());
                println!("output SHA-256: {}", report.output_sha256);
            }
        }
        Command::RemixAllUiKrPoc {
            rom,
            mode_assets_dir,
            remix_assets_dir,
            prompt_ttf,
            opponent_prompt_ttf_size,
            mode_select_dump,
            solo_type_dump,
            tokoton_option_dump,
            patched_mode_runtime_dump,
            patched_remix_runtime_dump,
            tokoton_play_asset,
            tokoton_demonstration_asset,
            tokoton_choice_dump,
            patched_tokoton_runtime_dump,
            rensa_ttf,
            sample_ttf_size,
            out,
            json,
        } => {
            let source = rom::load(&rom)?;
            let prompt_ttf_data = std::fs::read(&prompt_ttf)?;
            let rensa_ttf_data = std::fs::read(&rensa_ttf)?;
            let (patched, report) = combined_ui_poc::build(
                &source,
                rom.display().to_string(),
                &mode_assets_dir,
                &remix_assets_dir,
                prompt_ttf.display().to_string(),
                &prompt_ttf_data,
                opponent_prompt_ttf_size,
                &mode_select_dump,
                &solo_type_dump,
                &tokoton_option_dump,
                patched_mode_runtime_dump.as_deref(),
                patched_remix_runtime_dump.as_deref(),
                &tokoton_play_asset,
                &tokoton_demonstration_asset,
                &tokoton_choice_dump,
                patched_tokoton_runtime_dump.as_deref(),
                rensa_ttf.display().to_string(),
                &rensa_ttf_data,
                sample_ttf_size,
                out.display().to_string(),
            )?;
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&out, patched)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", report.verdict);
                println!("wrote combined Remix UI PoC ROM -> {}", out.display());
                println!("output SHA-256: {}", report.output_sha256);
            }
        }
        Command::RemixIntegratedKrPoc {
            rom,
            translation,
            port_map,
            terms,
            style,
            story_ttf,
            story_ttf_size,
            demo_title_ttf,
            demo_title_ttf_size,
            prompt_ttf,
            opponent_prompt_ttf_size,
            mode_assets_dir,
            remix_assets_dir,
            mode_select_dump,
            solo_type_dump,
            tokoton_option_dump,
            patched_mode_runtime_dump,
            patched_remix_runtime_dump,
            tokoton_play_asset,
            tokoton_demonstration_asset,
            tokoton_choice_dump,
            patched_tokoton_runtime_dump,
            ranking_ending,
            course_continue,
            out,
            encoding_out,
            json,
        } => {
            let source = rom::load(&rom)?;
            let story_ttf_data = std::fs::read(&story_ttf)?;
            // Easy-course captions keep Galmuri11 to match the original
            // caption stroke rule; the shared story font is Galmuri14.
            let caption_ttf_data = std::fs::read("assets/fonts/galmuri11.ttf")?;
            let demo_title_ttf_data = std::fs::read(&demo_title_ttf)?;
            let prompt_ttf_data = std::fs::read(&prompt_ttf)?;
            let RankingEndingArgs {
                ending_exclamation_ttf,
                ending_exclamation_ttf_size,
                ending_shock_asset,
                ending_kaakun_ttf,
                ending_kaakun_ttf_size,
            } = *ranking_ending;
            let ending_exclamation_ttf_data = std::fs::read(&ending_exclamation_ttf)?;
            let ending_shock_asset_data = std::fs::read(&ending_shock_asset)?;
            let ending_kaakun_ttf_data = std::fs::read(&ending_kaakun_ttf)?;
            let CourseContinueArgs {
                easy_course_assets_dir,
                easy_course_dump,
                continue_asset,
            } = *course_continue;
            let prompt_ttf_path = prompt_ttf.display().to_string();
            let (patched, report, encoding) =
                integrated_poc::build(&integrated_poc::IntegratedPocInputs {
                    source_path: rom.display().to_string(),
                    story: story_poc::StoryPocInputs {
                        rom: &source,
                        translation_path: &translation,
                        port_map_path: &port_map,
                        terms_path: &terms,
                        style_path: &style,
                        ttf_data: &story_ttf_data,
                        ttf_size: story_ttf_size,
                        opponent_prompt_ttf_path: &prompt_ttf_path,
                        opponent_prompt_ttf_data: &prompt_ttf_data,
                        opponent_prompt_ttf_size,
                    },
                    demo_title_ttf_path: demo_title_ttf.display().to_string(),
                    demo_title_ttf_data: &demo_title_ttf_data,
                    demo_title_font_px: demo_title_ttf_size,
                    mode_assets_dir: &mode_assets_dir,
                    remix_assets_dir: &remix_assets_dir,
                    mode_select_dump: &mode_select_dump,
                    solo_type_dump: &solo_type_dump,
                    tokoton_option_dump: &tokoton_option_dump,
                    patched_mode_runtime_dump: patched_mode_runtime_dump.as_deref(),
                    patched_remix_runtime_dump: patched_remix_runtime_dump.as_deref(),
                    tokoton_play_asset: &tokoton_play_asset,
                    tokoton_demonstration_asset: &tokoton_demonstration_asset,
                    tokoton_choice_dump: &tokoton_choice_dump,
                    patched_tokoton_runtime_dump: patched_tokoton_runtime_dump.as_deref(),
                    ranking_ending: remix_ranking_ending_graphics::Inputs {
                        exclamation_ttf_path: ending_exclamation_ttf.display().to_string(),
                        exclamation_ttf_data: &ending_exclamation_ttf_data,
                        exclamation_font_px: ending_exclamation_ttf_size,
                        shock_asset_path: ending_shock_asset.display().to_string(),
                        shock_asset_data: &ending_shock_asset_data,
                        kaakun_ttf_path: ending_kaakun_ttf.display().to_string(),
                        kaakun_ttf_data: &ending_kaakun_ttf_data,
                        kaakun_font_px: ending_kaakun_ttf_size,
                    },
                    course_continue: remix_course_continue_graphics::Inputs {
                        easy_course_assets: [
                            &easy_course_assets_dir.join("beginner.png"),
                            &easy_course_assets_dir.join("practiced.png"),
                            &easy_course_assets_dir.join("graduation.png"),
                        ],
                        easy_course_layout: &easy_course_dump,
                        continue_asset: &continue_asset,
                    },
                    caption_marker: remix_caption_marker_graphics::Inputs {
                        galmuri_ttf_data: &caption_ttf_data,
                        galmuri_bold_ttf_data: &ending_kaakun_ttf_data,
                    },
                    output_path: out.display().to_string(),
                })?;
            for path in [&out, &encoding_out] {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
            }
            std::fs::write(&out, patched)?;
            std::fs::write(&encoding_out, encoding)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", report.verdict);
                println!("wrote integrated Remix PoC ROM -> {}", out.display());
                println!("wrote story encoding -> {}", encoding_out.display());
                println!("output SHA-256: {}", report.output_sha256);
            }
        }
        Command::RemixProductionBuild {
            rom,
            policy,
            out_root,
            json,
        } => {
            let source = rom::load(&rom)?;
            let report = remix_production::build(&remix_production::Inputs {
                rom_path: &rom,
                rom: &source,
                policy,
                out_root: &out_root,
            })?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("policy: {}", report.policy);
                println!(
                    "wrote ROM -> {} ({})",
                    report.output_rom, report.output_sha256
                );
                println!("wrote BPS -> {} ({})", report.bps, report.bps_sha256);
                for blocker in &report.scope.release_blockers {
                    println!("release blocker: {blocker}");
                }
            }
        }
        Command::RemixLayoutSpecCheck { rom, dump_root } => {
            let source = rom::load(&rom)?;
            for screen in remix_layout_spec::SCREENS {
                let dump = dump_root.join(
                    screen
                        .discovery_dump
                        .rsplit('/')
                        .next()
                        .expect("discovery dump path has a final component"),
                );
                screen.check_against_dump(&dump, &source)?;
                println!("{}: layout constants match {}", screen.name, dump.display());
            }
        }
        Command::RemixMenuKrPoc {
            rom,
            assets_dir,
            runtime_dump,
            out,
            json,
        } => {
            let source = rom::load(&rom)?;
            let (patched, report) = remix_menu_graphics::build_poc(
                &source,
                rom.display().to_string(),
                &assets_dir,
                runtime_dump.as_deref(),
                out.display().to_string(),
            )?;
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&out, patched)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", report.verdict);
                println!("wrote Remix menu PoC ROM -> {}", out.display());
                println!("output SHA-256: {}", report.output_sha256);
            }
        }
    }
    Ok(())
}

fn parse_u8(value: &str) -> Result<u8> {
    let value = value.trim();
    let parsed = if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u8::from_str_radix(hex, 16)?
    } else {
        value.parse()?
    };
    Ok(parsed)
}

fn parse_usize(value: &str) -> Result<usize> {
    let value = value.trim();
    let parsed = if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        usize::from_str_radix(hex, 16)?
    } else {
        value.parse()?
    };
    Ok(parsed)
}
