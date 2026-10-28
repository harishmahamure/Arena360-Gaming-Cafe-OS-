//! The global game catalogue as a Hub reads it (Story 6.1): the manifest answered by
//! `GET /v1/catalog/manifest?since=<version>`. A game is a live entry, or a tombstone for one
//! that was unpublished or deleted. Media are content-addressed objects behind the CDN.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The manifest path, under `/v1`.
pub const CATALOG_MANIFEST_PATH: &str = "/catalog/manifest";

/// The manifest calls one tenant may make per minute.
pub const CATALOG_MANIFEST_PER_MIN: u64 = 30;

/// An executable that identifies a game on a PC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CatalogSignature {
    pub exe_name: String,
    pub product_name: String,
    pub publisher: String,
    /// SHA-256 of the executable, lowercase hex, when the catalogue knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

/// A cover or a flash-screened video.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CatalogMedia {
    /// `cover` or `video`.
    pub kind: String,
    /// SHA-256 of the file, lowercase hex.
    pub sha256: String,
    #[ts(type = "number")]
    pub bytes: u64,
    /// Where to fetch it: `<CDN base>/catalog/<sha256>.<ext>`.
    pub url: String,
}

/// One game in a manifest. A tombstone has `deleted` set and nothing else but `id` and `rev`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CatalogGame {
    pub id: String,
    #[ts(type = "number")]
    pub rev: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deleted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launcher: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub age_rating: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signatures: Vec<CatalogSignature>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<CatalogMedia>,
}

/// The games published or withdrawn since the Hub's version, and the version to ask from next.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct CatalogManifest {
    #[ts(type = "number")]
    pub catalog_version: u64,
    pub games: Vec<CatalogGame>,
}
