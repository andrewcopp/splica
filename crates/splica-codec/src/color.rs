//! Shared color parameter mapping for ITU-T codec standards.
//!
//! Maps numeric colour_primaries / transfer_characteristics / matrix_coefficients
//! values (shared across H.264 Table E-3, H.265 Table E.3, and AV1 §6.4.2) to
//! splica's typed color enums.

use splica_core::media::{ColorPrimaries, MatrixCoefficients, TransferCharacteristics};

pub(crate) fn map_color_primaries(val: u8) -> Option<ColorPrimaries> {
    match val {
        1 => Some(ColorPrimaries::Bt709),
        9 => Some(ColorPrimaries::Bt2020),
        12 => Some(ColorPrimaries::Smpte432),
        _ => None,
    }
}

pub(crate) fn map_transfer_characteristics(val: u8) -> Option<TransferCharacteristics> {
    match val {
        1 => Some(TransferCharacteristics::Bt709),
        16 => Some(TransferCharacteristics::Smpte2084),
        18 => Some(TransferCharacteristics::HybridLogGamma),
        _ => None,
    }
}

pub(crate) fn map_matrix_coefficients(val: u8) -> Option<MatrixCoefficients> {
    match val {
        0 => Some(MatrixCoefficients::Identity),
        1 => Some(MatrixCoefficients::Bt709),
        9 => Some(MatrixCoefficients::Bt2020NonConstant),
        10 => Some(MatrixCoefficients::Bt2020Constant),
        _ => None,
    }
}
