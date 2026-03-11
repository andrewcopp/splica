//! WASM bindings for the splica media processing library.
//!
//! Re-exports wasm-bindgen annotated types from `splica-mp4` and `splica-webm`.
//!
//! # Usage (JavaScript)
//!
//! ```js
//! import init, { WasmMp4Demuxer } from './pkg/splica_wasm.js';
//!
//! await init();
//!
//! // Open MP4 and get WebCodecs-compatible config
//! const mp4Data = new Uint8Array(await (await fetch('video.mp4')).arrayBuffer());
//! const demuxer = WasmMp4Demuxer.fromBytes(mp4Data);
//! const config = demuxer.videoDecoderConfig();
//!
//! // Feed packets to WebCodecs VideoDecoder
//! while (true) {
//!     const packet = demuxer.readVideoPacket();
//!     if (!packet) break;
//!     // packet.data, packet.timestampUs, packet.isKeyframe
//! }
//! ```
//!
//! # Build
//!
//! ```sh
//! wasm-pack build --target web crates/splica-wasm
//! ```

pub use splica_core::wasm_types::{WasmVideoDecoderConfig, WasmVideoPacket};
pub use splica_mp4::wasm::WasmMp4Demuxer;
pub use splica_webm::wasm::WasmWebmDemuxer;
