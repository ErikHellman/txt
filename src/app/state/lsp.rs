use std::path::PathBuf;

/// Why the user is being prompted to approve an LSP binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LspApprovalReason {
    /// We've never seen this binary before.
    FirstLaunch,
    /// We have an entry for this path, but the hash on disk has changed.
    BinaryChanged { previous_hash: String },
}

/// State for the LSP-binary approval overlay.
#[derive(Debug, Clone)]
pub struct PendingLspApproval {
    /// Server identifier from the active LSP config (e.g. `"rust-analyzer"`).
    pub server_name: String,
    /// Raw command from the config (may be a bare name or a path).
    pub command: String,
    /// Command-line args from the config.
    pub args: Vec<String>,
    /// Path returned by `which` (or the absolute path the user gave).
    pub display_path: PathBuf,
    /// Canonicalized path that gets hashed and recorded in the trust store.
    pub canonical_path: PathBuf,
    /// SHA-256 of the binary contents, lowercase hex.
    pub hash: String,
    /// What triggered the prompt.
    pub reason: LspApprovalReason,
}
