//! Validation for anything that arrives from outside the process.
//!
//! A manifest is untrusted input even when it comes from your own sync server —
//! a compromised or buggy publisher must not be able to write a file outside the
//! pack directory or pull a jar from an arbitrary host. Both the launcher
//! and the server run these checks; neither trusts the other to have done it.

use thiserror::Error;

/// Hosts a manifest may point downloads at. Mirrors the allowlist Modrinth
/// enforces on `.mrpack` files, for the same reason: without it, "install this
/// pack" means "run whatever this URL serves".
pub const ALLOWED_DOWNLOAD_HOSTS: &[&str] = &[
    "cdn.modrinth.com",
    "github.com",
    "raw.githubusercontent.com",
    "gitlab.com",
];

/// Windows treats these as device names in every directory, with or without an
/// extension. A file called `CON.jar` is not creatable and the failure mode is
/// confusing, so reject them at the manifest boundary.
const WINDOWS_RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

const MAX_PATH_LEN: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ValidationError {
    #[error("manifest schema {found} is not supported (this launcher understands {supported})")]
    UnsupportedSchema { found: u32, supported: u32 },

    #[error("`{0}` must not be empty")]
    EmptyField(&'static str),

    #[error("path `{0}` is empty or too long")]
    PathLength(String),

    #[error("path `{0}` must be relative and use forward slashes")]
    PathNotRelative(String),

    #[error("path `{0}` escapes the pack directory")]
    PathTraversal(String),

    #[error("path `{0}` contains a component that is invalid on Windows")]
    PathInvalidOnWindows(String),

    #[error("path `{path}` must live under `{expected}/`")]
    PathOutsideKindDirectory { path: String, expected: &'static str },

    #[error("two entries both install to `{0}`")]
    DuplicatePath(String),

    #[error("`{0}` appears twice in this pack")]
    DuplicateProject(String),

    #[error("`{0}` has no download URLs")]
    NoDownloads(String),

    #[error("download URL `{0}` is not a valid URL")]
    MalformedUrl(String),

    #[error("download URL `{0}` must use https")]
    InsecureUrl(String),

    #[error("download host `{host}` is not allowed (permitted: {allowed})")]
    DisallowedHost { host: String, allowed: String },

    #[error("{field} must be {expected} hex characters")]
    BadHash { field: &'static str, expected: usize },
}

/// Accepts only paths that stay inside the pack directory.
///
/// Rejects absolute paths, drive letters, `..`, backslashes, control characters,
/// and the Windows-specific traps (reserved device names, trailing dots or
/// spaces, alternate data streams).
pub fn validate_relative_path(path: &str) -> Result<(), ValidationError> {
    if path.is_empty() || path.len() > MAX_PATH_LEN {
        return Err(ValidationError::PathLength(path.to_string()));
    }

    // Backslashes are a separator on Windows but a legal filename character on
    // Linux, so "a\..\b" would mean different things on different machines.
    // One canonical separator removes the ambiguity.
    if path.contains('\\') {
        return Err(ValidationError::PathNotRelative(path.to_string()));
    }
    if path.starts_with('/') {
        return Err(ValidationError::PathNotRelative(path.to_string()));
    }
    // `C:` prefix, and also any stray colon — on Windows `file.jar:stream`
    // addresses an alternate data stream.
    if path.contains(':') {
        return Err(ValidationError::PathNotRelative(path.to_string()));
    }
    if path.chars().any(|c| c.is_control()) {
        return Err(ValidationError::PathInvalidOnWindows(path.to_string()));
    }

    for component in path.split('/') {
        if component.is_empty() || component == "." {
            return Err(ValidationError::PathNotRelative(path.to_string()));
        }
        if component == ".." {
            return Err(ValidationError::PathTraversal(path.to_string()));
        }
        // Windows silently strips these, so `evil. ` and `evil` collide.
        if component.ends_with('.') || component.ends_with(' ') || component.starts_with(' ') {
            return Err(ValidationError::PathInvalidOnWindows(path.to_string()));
        }

        let stem = component.split('.').next().unwrap_or(component).to_ascii_lowercase();
        if WINDOWS_RESERVED.contains(&stem.as_str()) {
            return Err(ValidationError::PathInvalidOnWindows(path.to_string()));
        }
    }

    Ok(())
}

/// Accepts only `https` URLs pointing at [`ALLOWED_DOWNLOAD_HOSTS`].
pub fn validate_download_url(raw: &str) -> Result<(), ValidationError> {
    let url = url::Url::parse(raw).map_err(|_| ValidationError::MalformedUrl(raw.to_string()))?;

    if url.scheme() != "https" {
        return Err(ValidationError::InsecureUrl(raw.to_string()));
    }

    // `https://cdn.modrinth.com@evil.test/` parses with host `evil.test`; some
    // readers see the prefix and assume otherwise. Nothing legitimate needs
    // credentials in a CDN URL, so refuse them outright.
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ValidationError::MalformedUrl(raw.to_string()));
    }

    let host = url
        .host_str()
        .ok_or_else(|| ValidationError::MalformedUrl(raw.to_string()))?
        .to_ascii_lowercase();

    // Exact match only. Allowing subdomains would let anyone with a
    // user-content subdomain serve arbitrary jars.
    if !ALLOWED_DOWNLOAD_HOSTS.contains(&host.as_str()) {
        return Err(ValidationError::DisallowedHost {
            host,
            allowed: ALLOWED_DOWNLOAD_HOSTS.join(", "),
        });
    }

    Ok(())
}

pub fn validate_hex(value: &str, expected_len: usize, field: &'static str) -> Result<(), ValidationError> {
    if value.len() != expected_len || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ValidationError::BadHash { field, expected: expected_len });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ordinary_content_paths() {
        for path in [
            "mods/sodium-fabric-0.6.0.jar",
            "resourcepacks/faithful.zip",
            "shaderpacks/BSL_v8.2.zip",
            "config/sodium-options.json",
            "config/**/*keybind*",
        ] {
            validate_relative_path(path).unwrap_or_else(|e| panic!("{path} rejected: {e}"));
        }
    }

    #[test]
    fn rejects_directory_traversal() {
        for path in [
            "../evil.jar",
            "mods/../../evil.jar",
            "mods/../../../Windows/System32/evil.dll",
            "..",
        ] {
            assert!(
                matches!(validate_relative_path(path), Err(ValidationError::PathTraversal(_))),
                "{path} should have been rejected as traversal"
            );
        }
    }

    #[test]
    fn rejects_absolute_and_windows_style_paths() {
        for path in [
            "/etc/passwd",
            "C:/Windows/System32/evil.dll",
            "C:\\Windows\\evil.dll",
            "mods\\evil.jar",
            "mods//evil.jar",
            "mods/./evil.jar",
        ] {
            assert!(
                validate_relative_path(path).is_err(),
                "{path} should have been rejected"
            );
        }
    }

    #[test]
    fn rejects_windows_reserved_names_and_trailing_dots() {
        for path in ["mods/CON.jar", "mods/nul", "mods/aux.jar", "mods/evil.", "mods/evil "] {
            assert!(
                matches!(validate_relative_path(path), Err(ValidationError::PathInvalidOnWindows(_))),
                "{path} should have been rejected"
            );
        }
    }

    #[test]
    fn rejects_alternate_data_streams() {
        assert!(validate_relative_path("mods/ok.jar:hidden").is_err());
    }

    #[test]
    fn accepts_allowlisted_download_hosts() {
        for url in [
            "https://cdn.modrinth.com/data/AANobbMI/versions/x/sodium.jar",
            "https://github.com/org/repo/releases/download/v1/mod.jar",
            "https://raw.githubusercontent.com/org/repo/main/mod.jar",
            "https://gitlab.com/org/repo/-/raw/main/mod.jar",
        ] {
            validate_download_url(url).unwrap_or_else(|e| panic!("{url} rejected: {e}"));
        }
    }

    #[test]
    fn rejects_downloads_from_anywhere_else() {
        assert!(matches!(
            validate_download_url("https://evil.test/mod.jar"),
            Err(ValidationError::DisallowedHost { .. })
        ));
        // Subdomains of an allowed host are still not the allowed host.
        assert!(validate_download_url("https://evil.cdn.modrinth.com/x.jar").is_err());
        assert!(validate_download_url("https://cdn.modrinth.com.evil.test/x.jar").is_err());
    }

    #[test]
    fn rejects_plaintext_and_non_http_schemes() {
        assert!(matches!(
            validate_download_url("http://cdn.modrinth.com/x.jar"),
            Err(ValidationError::InsecureUrl(_))
        ));
        assert!(validate_download_url("file:///C:/evil.jar").is_err());
    }

    #[test]
    fn rejects_credentials_that_disguise_the_real_host() {
        assert!(validate_download_url("https://cdn.modrinth.com@evil.test/x.jar").is_err());
    }

    #[test]
    fn hash_validation_checks_length_and_alphabet() {
        validate_hex(&"a".repeat(40), 40, "sha1").unwrap();
        assert!(validate_hex(&"a".repeat(39), 40, "sha1").is_err());
        assert!(validate_hex(&"z".repeat(40), 40, "sha1").is_err());
    }
}
