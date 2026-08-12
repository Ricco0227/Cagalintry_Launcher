//! Maven coordinates, as used to name libraries.
//!
//! Vanilla version JSON normally carries an explicit `path` for every library,
//! but loader profiles routinely omit it and expect the launcher to derive the
//! layout from the coordinate alone — so this has to be exactly right or the
//! classpath points at files that were never written.

/// `group:artifact:version[:classifier][@extension]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MavenCoord {
    pub group: String,
    pub artifact: String,
    pub version: String,
    pub classifier: Option<String>,
    pub extension: String,
}

impl MavenCoord {
    pub fn parse(coord: &str) -> Option<Self> {
        // The extension suffix binds to the whole coordinate, not the last part.
        let (body, extension) = match coord.split_once('@') {
            Some((body, ext)) => (body, ext.to_string()),
            None => (coord, "jar".to_string()),
        };

        let mut parts = body.split(':');
        let group = parts.next()?.to_string();
        let artifact = parts.next()?.to_string();
        let version = parts.next()?.to_string();
        let classifier = parts.next().map(str::to_string);

        if group.is_empty() || artifact.is_empty() || version.is_empty() {
            return None;
        }

        Some(Self { group, artifact, version, classifier, extension })
    }

    /// Repository-relative path, always `/`-separated.
    pub fn path(&self) -> String {
        let group = self.group.replace('.', "/");
        let classifier = match &self.classifier {
            Some(c) => format!("-{c}"),
            None => String::new(),
        };
        format!(
            "{group}/{artifact}/{version}/{artifact}-{version}{classifier}.{ext}",
            artifact = self.artifact,
            version = self.version,
            ext = self.extension,
        )
    }

    /// Native libraries are ordinary artifacts distinguished only by a
    /// `natives-*` classifier. They belong in the natives directory, extracted,
    /// rather than on the classpath.
    pub fn is_native(&self) -> bool {
        self.classifier.as_deref().is_some_and(|c| c.starts_with("natives-"))
    }

    /// For a native artifact, the CPU architecture it targets.
    ///
    /// Mojang's rules for native libraries gate on the operating system only —
    /// `natives-windows`, `natives-windows-x86` and `natives-windows-arm64` all
    /// carry the identical rule `{"os": {"name": "windows"}}`. The architecture
    /// lives in the classifier suffix and nowhere else, so a launcher that
    /// trusts the rules alone unpacks all three over each other and whichever
    /// lands last wins. On anything but plain x64 that is the wrong one, and
    /// the game dies with `Failed to locate library: lwjgl.dll`.
    ///
    /// An unsuffixed classifier means x86_64, following LWJGL's convention.
    pub fn native_arch(&self) -> Option<String> {
        let classifier = self.classifier.as_deref()?;
        let rest = classifier.strip_prefix("natives-")?;

        for os in ["windows", "macos", "linux", "osx"] {
            if let Some(suffix) = rest.strip_prefix(os) {
                return Some(match suffix.trim_start_matches('-') {
                    "" => "x86_64".to_string(),
                    arch => arch.to_string(),
                });
            }
        }
        None
    }

    /// Everything but the version — two entries sharing this are the same
    /// library, and only the newest should reach the classpath.
    pub fn versionless_key(&self) -> String {
        match &self.classifier {
            Some(c) => format!("{}:{}:{c}", self.group, self.artifact),
            None => format!("{}:{}", self.group, self.artifact),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_plain_coordinate() {
        let c = MavenCoord::parse("org.lwjgl:lwjgl:3.3.3").unwrap();
        assert_eq!(c.group, "org.lwjgl");
        assert_eq!(c.artifact, "lwjgl");
        assert_eq!(c.version, "3.3.3");
        assert_eq!(c.classifier, None);
        assert_eq!(c.extension, "jar");
        assert_eq!(c.path(), "org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3.jar");
    }

    #[test]
    fn parses_a_classified_coordinate() {
        let c = MavenCoord::parse("org.lwjgl:lwjgl:3.3.3:natives-windows").unwrap();
        assert_eq!(c.classifier.as_deref(), Some("natives-windows"));
        assert_eq!(
            c.path(),
            "org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-windows.jar"
        );
        assert!(c.is_native());
    }

    #[test]
    fn parses_an_explicit_extension() {
        // Loader profiles use this for the non-jar artifacts they reference.
        let c = MavenCoord::parse("net.minecraftforge:forge:1.20.1-47.2.0:installer@zip").unwrap();
        assert_eq!(c.extension, "zip");
        assert!(c.path().ends_with("forge-1.20.1-47.2.0-installer.zip"));
    }

    #[test]
    fn group_dots_become_directories() {
        let c = MavenCoord::parse("net.fabricmc:fabric-loader:0.16.10").unwrap();
        assert_eq!(
            c.path(),
            "net/fabricmc/fabric-loader/0.16.10/fabric-loader-0.16.10.jar"
        );
    }

    #[test]
    fn rejects_incomplete_coordinates() {
        assert!(MavenCoord::parse("org.lwjgl").is_none());
        assert!(MavenCoord::parse("org.lwjgl:lwjgl").is_none());
        assert!(MavenCoord::parse(":lwjgl:3.3.3").is_none());
        assert!(MavenCoord::parse("org.lwjgl::3.3.3").is_none());
    }

    #[test]
    fn a_plain_artifact_is_not_a_native() {
        assert!(!MavenCoord::parse("org.lwjgl:lwjgl:3.3.3").unwrap().is_native());
        // A classifier that isn't a natives one stays on the classpath.
        assert!(!MavenCoord::parse("com.example:lib:1.0:sources").unwrap().is_native());
    }

    #[test]
    fn native_classifiers_carry_the_architecture() {
        let arch = |coord: &str| MavenCoord::parse(coord).unwrap().native_arch();

        // Unsuffixed means 64-bit x86, by LWJGL convention.
        assert_eq!(arch("org.lwjgl:lwjgl:3.4.1:natives-windows").as_deref(), Some("x86_64"));
        assert_eq!(arch("org.lwjgl:lwjgl:3.4.1:natives-windows-x86").as_deref(), Some("x86"));
        assert_eq!(arch("org.lwjgl:lwjgl:3.4.1:natives-windows-arm64").as_deref(), Some("arm64"));
        assert_eq!(arch("org.lwjgl:lwjgl:3.4.1:natives-linux").as_deref(), Some("x86_64"));
        assert_eq!(arch("org.lwjgl:lwjgl:3.4.1:natives-linux-arm64").as_deref(), Some("arm64"));
        assert_eq!(arch("org.lwjgl:lwjgl:3.4.1:natives-macos").as_deref(), Some("x86_64"));
        assert_eq!(arch("org.lwjgl:lwjgl:3.4.1:natives-macos-arm64").as_deref(), Some("arm64"));
    }

    #[test]
    fn non_native_artifacts_have_no_architecture() {
        let arch = |coord: &str| MavenCoord::parse(coord).unwrap().native_arch();
        assert_eq!(arch("org.lwjgl:lwjgl:3.4.1"), None);
        assert_eq!(arch("org.lwjgl:lwjgl:3.4.1:unsafe"), None);
        assert_eq!(arch("com.example:lib:1.0:sources"), None);
    }

    #[test]
    fn versionless_keys_identify_duplicate_libraries() {
        // Loaders ship newer copies of libraries vanilla also pulls in; the
        // classpath must carry exactly one of each.
        let old = MavenCoord::parse("com.google.guava:guava:21.0").unwrap();
        let new = MavenCoord::parse("com.google.guava:guava:32.1.2").unwrap();
        assert_eq!(old.versionless_key(), new.versionless_key());

        // Natives for different platforms are genuinely different artifacts.
        let win = MavenCoord::parse("org.lwjgl:lwjgl:3.3.3:natives-windows").unwrap();
        let linux = MavenCoord::parse("org.lwjgl:lwjgl:3.3.3:natives-linux").unwrap();
        assert_ne!(win.versionless_key(), linux.versionless_key());
    }
}
