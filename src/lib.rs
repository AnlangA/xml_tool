//! `xml_tool` is an `eframe`/`egui` desktop XML and EXI viewer/editor.
//!
//! The library exposes the application shell plus the core XML, EXI, export,
//! caching, and UI modules used by the desktop binary.
//!
//! Current behavior and limits:
//! - XML editing is structured around the in-memory tree model.
//! - Qualified names are preserved, but namespace-aware editing is not yet implemented.
//! - Processing instructions and doctypes are tolerated on parse, but not modeled for editing.
//! - JSON export preserves mixed-content order with an `@content` array.
//!
pub mod app;
pub mod cache;
pub mod core;
pub mod exi;
pub mod export;
pub mod fixtures;
pub mod services;
pub mod ui;
pub mod utils;
pub mod xml;

pub use app::XmlToolApp;
