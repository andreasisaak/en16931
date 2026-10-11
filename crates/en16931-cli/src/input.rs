//! Getting bytes out of a file or standard input.
//!
//! Syntax and container detection is left to [`en16931_formats::read`] and
//! [`en16931_formats::receive`].

use std::path::{Path, PathBuf};

/// A document, read without validating it.
pub struct Loaded {
    /// Where it came from, for messages. `-` for standard input.
    pub source: PathBuf,
    /// What the reader made of it.
    pub read: en16931_formats::Read,
}

/// Read and parse the file at `path`, or standard input for `-`.
///
/// # Errors
/// A message naming the path.
pub fn load(path: &Path) -> Result<Loaded, String> {
    let bytes = bytes(path)?;
    let read = en16931_formats::read(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Loaded {
        source: path.to_owned(),
        read,
    })
}

/// The bytes of the file at `path`, or of standard input for `-`.
///
/// # Errors
/// A message naming the path.
pub fn bytes(path: &Path) -> Result<Vec<u8>, String> {
    if path == Path::new("-") {
        use std::io::Read as _;
        let mut buf = Vec::new();
        std::io::stdin()
            .read_to_end(&mut buf)
            .map_err(|e| format!("-: {e}"))?;
        return Ok(buf);
    }
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}
