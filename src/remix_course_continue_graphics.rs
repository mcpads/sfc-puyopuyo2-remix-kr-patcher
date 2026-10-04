//! Two Japanese graphics every playthrough meets: the easy-mode course signs
//! (`はじめて/なれた/そつぎょう`) and the game-over continue line. Both are
//! rebuilt from committed generated labels on an already verified derivative.

use std::{ops::Range, path::Path};

use anyhow::Result;
use serde::Serialize;

pub const EASY_COURSE_TEXTS: [&str; 3] = ["입문", "숙련", "졸업"];
pub const CONTINUE_TEXT: &str = "컨티뉴 할래?";
const EASY_COURSE_STREAM: Range<usize> = 0x0C_0669..0x0C_1C16;

pub struct Inputs<'a> {
    /// Generated 48x40 labels in slot order (입문, 숙련, 졸업).
    pub easy_course_assets: [&'a Path; 3],
    /// BG1 layout of the easy course select screen (runtime dump or the
    /// production layout materialized from `remix_layout_spec::EASY_COURSE`).
    pub easy_course_layout: &'a Path,
    /// 160x16 line: white stroke, black outline, transparent background.
    pub continue_asset: &'a Path,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub easy_courses: crate::remix_tokoton_play::SignStreamReport,
    pub continue_line: crate::game_over_continue::Report,
}

/// The caller owns the checksum.
pub fn build_after_verified_patch(source: &[u8], inputs: &Inputs<'_>) -> Result<(Vec<u8>, Report)> {
    let (course_rom, easy_courses) = crate::remix_tokoton_play::patch_sign_stream(
        source,
        &crate::remix_tokoton_play::EASY_COURSES,
        &EASY_COURSE_TEXTS,
        &inputs.easy_course_assets,
        inputs.easy_course_layout,
    )?;
    let (patched, continue_line) =
        crate::game_over_continue::patch(&course_rom, CONTINUE_TEXT, inputs.continue_asset)?;
    Ok((
        patched,
        Report {
            easy_courses,
            continue_line,
        },
    ))
}

pub fn registered_write(offset: usize) -> Option<&'static str> {
    if EASY_COURSE_STREAM.contains(&offset) {
        Some("easy course sign stream")
    } else {
        crate::game_over_continue::registered_write(offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easy_course_range_matches_the_sign_stream_contract() {
        let stream = crate::remix_tokoton_play::EASY_COURSES;
        assert_eq!(EASY_COURSE_STREAM.start, stream.stream_pc);
        assert_eq!(EASY_COURSE_STREAM.len(), stream.compressed_len);
        assert_eq!(EASY_COURSE_TEXTS.len(), stream.slots.len());
    }
}
