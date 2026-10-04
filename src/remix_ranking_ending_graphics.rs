use std::ops::Range;

use anyhow::{Result, bail};
use serde::Serialize;

// Every stream below is byte-exact between the base game and Remix at the same
// PC, so each builder verifies its own stream hash instead of a whole-ROM hash.
const RANKING_CHR_STREAM: Range<usize> = 0x13_0F31..0x13_2FC2;
const RANKING_TILEMAP_STREAM: Range<usize> = 0x12_FB16..0x12_FC58;
const GIANT_EXCLAMATION_MAP_STREAM: Range<usize> = 0x12_1EBC..0x12_1F56;
const SHOCK_CHR_STREAM: Range<usize> = 0x12_1F56..0x12_36C9;
const GIANT_KAAKUN_MAP_STREAM: Range<usize> = 0x12_5669..0x12_5705;

pub struct Inputs<'a> {
    pub exclamation_ttf_path: String,
    pub exclamation_ttf_data: &'a [u8],
    pub exclamation_font_px: f32,
    pub shock_asset_path: String,
    pub shock_asset_data: &'a [u8],
    pub kaakun_ttf_path: String,
    pub kaakun_ttf_data: &'a [u8],
    pub kaakun_font_px: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub verdict: String,
    pub ranking: crate::ranking_graphics::RankingGraphicsKrPocReport,
    pub giant_exclamation: crate::ending_graphics::GiantExclamationBuildReport,
    pub shock_text: crate::ending_graphics::ShockTextBuildReport,
    pub giant_kaakun: crate::ending_graphics::GiantKaakunBuildReport,
    pub diff_confined_to_registered_writes: bool,
}

/// Applies the ranking titles and the true-ending 앗!, 쿠궁!, 카~ 군~! banners
/// to an already verified derivative. The caller recomputes the checksum.
pub fn build_after_verified_patch(source: &[u8], inputs: &Inputs<'_>) -> Result<(Vec<u8>, Report)> {
    let intermediate = "in-process Remix ranking/ending intermediate; not written".to_owned();
    let (ranking_rom, ranking) = crate::ranking_graphics::build_ranking_graphics_kr_poc(
        source,
        "verified in-process Remix derivative".to_owned(),
        None,
        intermediate.clone(),
    )?;
    let (exclamation_rom, giant_exclamation) = crate::ending_graphics::build_giant_exclamation_kr(
        &ranking_rom,
        "verified in-process Remix ranking derivative".to_owned(),
        inputs.exclamation_ttf_path.clone(),
        inputs.exclamation_ttf_data,
        inputs.exclamation_font_px,
        None,
        None,
        intermediate.clone(),
    )?;
    let (shock_rom, shock_text) = crate::ending_graphics::build_shock_text_kr(
        &exclamation_rom,
        "verified in-process Remix giant-exclamation derivative".to_owned(),
        inputs.shock_asset_path.clone(),
        inputs.shock_asset_data,
        intermediate.clone(),
    )?;
    let (patched, giant_kaakun) = crate::ending_graphics::build_giant_kaakun_kr(
        &shock_rom,
        "verified in-process Remix shock-text derivative".to_owned(),
        inputs.kaakun_ttf_path.clone(),
        inputs.kaakun_ttf_data,
        inputs.kaakun_font_px,
        None,
        intermediate,
    )?;

    let diff_confined_to_registered_writes = source
        .iter()
        .zip(&patched)
        .enumerate()
        .all(|(offset, (before, after))| before == after || registered_write(offset).is_some());
    if !diff_confined_to_registered_writes {
        bail!("Remix ranking/ending graphics diff escaped its registered stream slots");
    }
    Ok((
        patched,
        Report {
            verdict: "ranking titles and true-ending 앗!, 쿠궁!, 카~ 군~! rebuilt in their verified stream slots"
                .to_owned(),
            ranking,
            giant_exclamation,
            shock_text,
            giant_kaakun,
            diff_confined_to_registered_writes,
        },
    ))
}

pub fn registered_write(offset: usize) -> Option<&'static str> {
    if RANKING_CHR_STREAM.contains(&offset) {
        Some("ranking title CHR stream")
    } else if RANKING_TILEMAP_STREAM.contains(&offset) {
        Some("ranking title tilemap stream")
    } else if GIANT_EXCLAMATION_MAP_STREAM.contains(&offset) {
        Some("true-ending 앗! tilemap stream")
    } else if SHOCK_CHR_STREAM.contains(&offset) {
        Some("true-ending 쿠궁! CHR stream")
    } else if GIANT_KAAKUN_MAP_STREAM.contains(&offset) {
        Some("true-ending 카~ 군~! tilemap stream")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_streams_do_not_overlap() {
        let ranges = [
            RANKING_CHR_STREAM,
            RANKING_TILEMAP_STREAM,
            GIANT_EXCLAMATION_MAP_STREAM,
            SHOCK_CHR_STREAM,
            GIANT_KAAKUN_MAP_STREAM,
        ];
        for (index, left) in ranges.iter().enumerate() {
            for right in &ranges[index + 1..] {
                assert!(left.end <= right.start || right.end <= left.start);
            }
        }
    }

    #[test]
    #[ignore = "requires the Remix JP ROM, maplestory_bold.ttf, galmuri11_bold.ttf and true_ending_shock.png under assets/"]
    fn remix_source_builds_all_four_surfaces() {
        let rom = crate::test_input::read("roms/Super Puyo Puyo Tsuu Remix (Japan).sfc");
        let maple = crate::test_input::read("assets/fonts/maplestory_bold.ttf");
        let shock = crate::test_input::read("assets/ending_graphics/true_ending_shock.png");
        let galmuri_bold = crate::test_input::read("assets/fonts/galmuri11_bold.ttf");
        let (patched, report) = build_after_verified_patch(
            &rom,
            &Inputs {
                exclamation_ttf_path: "assets/fonts/maplestory_bold.ttf".to_owned(),
                exclamation_ttf_data: &maple,
                exclamation_font_px: 14.0,
                shock_asset_path: "assets/ending_graphics/true_ending_shock.png".to_owned(),
                shock_asset_data: &shock,
                kaakun_ttf_path: "assets/fonts/galmuri11_bold.ttf".to_owned(),
                kaakun_ttf_data: &galmuri_bold,
                kaakun_font_px: 12.0,
            },
        )
        .unwrap();
        assert_eq!(patched.len(), rom.len());
        assert!(report.diff_confined_to_registered_writes);
        assert_ne!(patched, rom);
    }
}
