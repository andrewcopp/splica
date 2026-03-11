//! WASM bindings for the splica media processing library.
//!
//! Re-exports wasm-bindgen annotated types from `splica-mp4` and `splica-webm`.
//!
//! # Usage (JavaScript)
//!
//! ```js
//! import init, { WasmMp4Demuxer, WasmWebmDemuxer } from './pkg/splica_wasm.js';
//!
//! await init();
//!
//! // MP4
//! const mp4Data = new Uint8Array(await (await fetch('video.mp4')).arrayBuffer());
//! const mp4 = WasmMp4Demuxer.fromBytes(mp4Data);
//! console.log('Tracks:', mp4.trackCount());
//! console.log('Video:', mp4.videoTrackInfo());
//!
//! // WebM
//! const webmData = new Uint8Array(await (await fetch('video.webm')).arrayBuffer());
//! const webm = WasmWebmDemuxer.fromBytes(webmData);
//! console.log('Tracks:', webm.trackCount());
//! ```
//!
//! # Build
//!
//! ```sh
//! wasm-pack build --target web crates/splica-wasm
//! ```

pub use splica_mp4::wasm::WasmMp4Demuxer;
pub use splica_webm::wasm::WasmWebmDemuxer;
