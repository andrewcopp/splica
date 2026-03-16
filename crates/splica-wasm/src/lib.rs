//! WASM bindings for the splica media processing library.
//!
//! Re-exports wasm-bindgen annotated types from `splica-mp4` and `splica-webm`.
//!
//! # Usage (JavaScript)
//!
//! ```js
//! import init, { WasmMp4Demuxer, detectContainer } from './pkg/splica_wasm.js';
//!
//! await init();
//!
//! const fileData = new Uint8Array(await (await fetch('video.mp4')).arrayBuffer());
//!
//! // Auto-detect the container format from the first bytes
//! const format = detectContainer(fileData.slice(0, 64));
//! // format is "mp4", "webm", or "mkv" (or throws on unrecognized)
//!
//! // Then instantiate the right demuxer
//! let demuxer;
//! if (format === "mp4") {
//!     demuxer = WasmMp4Demuxer.fromBytes(fileData);
//! } else if (format === "webm") {
//!     demuxer = WasmWebmDemuxer.fromBytes(fileData);
//! } else {
//!     demuxer = WasmMkvDemuxer.fromBytes(fileData);
//! }
//! ```
//!
//! # Build
//!
//! ```sh
//! wasm-pack build --target web crates/splica-wasm
//! ```

use wasm_bindgen::prelude::*;

use splica_core::container_detect;
use splica_core::media::ContainerFormat;

pub use splica_core::wasm_types::{
    WasmAudioDecoderConfig, WasmAudioPacket, WasmVideoDecoderConfig, WasmVideoPacket,
};
pub use splica_mkv::wasm::WasmMkvDemuxer;
pub use splica_mp4::wasm::WasmMp4Demuxer;
pub use splica_webm::wasm::WasmWebmDemuxer;

/// Detects the container format from the first bytes of a media file.
///
/// Pass at least 64 bytes from the start of the file. Returns `"mp4"`,
/// `"webm"`, or `"mkv"`. Throws if the format is unrecognized.
///
/// # Example (JS)
///
/// ```js
/// const header = fileData.slice(0, 64);
/// const format = detectContainer(header); // "mp4" | "webm" | "mkv"
/// ```
#[wasm_bindgen(js_name = "detectContainer")]
pub fn detect_container(header: &[u8]) -> Result<String, JsError> {
    let format = container_detect::detect_container(header).ok_or_else(|| {
        JsError::new("unrecognized container format — expected MP4, WebM, or MKV")
    })?;

    Ok(match format {
        ContainerFormat::Mp4 => "mp4".to_string(),
        ContainerFormat::WebM => "webm".to_string(),
        ContainerFormat::Mkv => "mkv".to_string(),
    })
}
