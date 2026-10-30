//! The branch key's escrow and the player key backup (Story 9.2, AD-24).
//!
//! `POST /v1/kek/escrow`: on every tier, the Hub sends its branch key (KEK) wrapped for the cloud
//! escrow public key. The cloud stores the wrapped bytes in `gz_control.kek_escrow` and never
//! hands them back through any endpoint. The tenant, the branch and the Hub come from the device
//! token; the body holds nothing else.
//!
//! On paid tiers the Hub also PUTs `keys/player-keys.bin` (presign `kind: keys`): the player key
//! table sealed with the branch key.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The `kind` of a presign request for the player key backup.
pub const PRESIGN_KIND_KEYS: &str = "keys";

/// The player key backup's object key under the branch prefix: overwritten, never versioned.
pub const KEYS_OBJECT_KEY: &str = "keys/player-keys.bin";

/// The age X25519 public keys (`age1…`) the branch key is wrapped for. The private half exists only
/// as a SOPS-managed cloud secret. Empty until the release build is given the escrow key; a Hub
/// without one escrows nothing (and says so in its log).
pub const KEK_ESCROW_RECIPIENTS: &[&str] = &[];

/// A throwaway recipient for dev and test builds. Its private half was discarded, so nothing
/// escrowed to it can be opened; tests that open a wrapped key generate their own pair.
#[cfg(any(test, feature = "dev-keys"))]
pub const DEV_KEK_ESCROW_RECIPIENTS: &[&str] =
    &["age1v0w7nljdjmtc27xp7rt4zyyrmryhr67skkwxemc2slu9x09dsgjq7dr5j4"];

/// What an age file (binary format) starts with. The cloud checks it before storing a wrapped key,
/// so a bare key can never be stored by mistake.
pub const AGE_MAGIC: &[u8] = b"age-encryption.org/v1\n";

/// The longest wrapped key the cloud accepts, in bytes (a 32-byte key in an age file is about 200).
pub const KEK_ESCROW_MAX_BYTES: usize = 1024;

/// A Hub's escrow of its branch key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KekEscrowRequest {
    /// The branch key wrapped for the escrow public key: an age file, standard base64.
    pub wrapped_kek: String,
}

/// The cloud holds the wrapped key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "export-ts", ts(export))]
pub struct KekEscrowResponse {
    pub stored: bool,
}
