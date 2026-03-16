//! Color space types: primaries, transfer characteristics, matrix coefficients, and range.

// ---------------------------------------------------------------------------
// Color space
// ---------------------------------------------------------------------------

/// Color primaries (defines the RGB gamut).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorPrimaries {
    Bt709,
    Bt2020,
    Smpte432,
}

/// Transfer characteristics (OETF/EOTF — gamma curve).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransferCharacteristics {
    Bt709,
    Smpte2084,
    HybridLogGamma,
}

/// Matrix coefficients for YCbCr ↔ RGB conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MatrixCoefficients {
    Bt709,
    Bt2020NonConstant,
    Bt2020Constant,
    Identity,
}

/// Color range: limited (broadcast/studio) vs full (PC/JPEG).
///
/// Getting this wrong causes crushed blacks or blown highlights in every
/// downstream decoder that respects the flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorRange {
    /// Limited range (16–235 luma, 16–240 chroma for 8-bit). Broadcast standard.
    Limited,
    /// Full range (0–255 for 8-bit). Common in JPEG, screen capture, PC content.
    Full,
}

/// Full color space description for a video frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ColorSpace {
    pub primaries: ColorPrimaries,
    pub transfer: TransferCharacteristics,
    pub matrix: MatrixCoefficients,
    pub range: ColorRange,
}

impl ColorSpace {
    /// Standard BT.709 color space (SDR, HD, limited range).
    pub const BT709: Self = Self {
        primaries: ColorPrimaries::Bt709,
        transfer: TransferCharacteristics::Bt709,
        matrix: MatrixCoefficients::Bt709,
        range: ColorRange::Limited,
    };

    /// BT.2020 with PQ transfer (HDR10, limited range).
    pub const BT2020_PQ: Self = Self {
        primaries: ColorPrimaries::Bt2020,
        transfer: TransferCharacteristics::Smpte2084,
        matrix: MatrixCoefficients::Bt2020NonConstant,
        range: ColorRange::Limited,
    };

    /// BT.2020 with HLG transfer (limited range).
    pub const BT2020_HLG: Self = Self {
        primaries: ColorPrimaries::Bt2020,
        transfer: TransferCharacteristics::HybridLogGamma,
        matrix: MatrixCoefficients::Bt2020NonConstant,
        range: ColorRange::Limited,
    };
}
