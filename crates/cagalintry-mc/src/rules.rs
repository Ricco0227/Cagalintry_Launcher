//! Evaluating Mojang's rule blocks against the machine we're running on.
//!
//! Rules gate both libraries and command-line arguments. Getting them wrong is
//! not subtle: include a library meant for another OS and the game fails to
//! start with a native-loading error; drop one that was needed and it fails the
//! same way. Every branch here is covered by a test.

use crate::meta::{Rule, RuleAction};

/// The values a rule is evaluated against. Parameterised rather than read from
/// globals so the tests can evaluate rules for platforms we aren't running on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Platform {
    /// Mojang's name for the OS: `windows`, `osx`, or `linux`.
    pub os: String,
    /// Mojang's name for the architecture: `x86`, `x86_64`, or `arm64`.
    pub arch: String,
    /// OS version, matched against patterns like `^10\.`.
    pub version: Option<String>,
}

impl Platform {
    /// The machine this launcher is running on.
    pub fn current() -> Self {
        Self {
            os: current_os().to_string(),
            arch: current_arch().to_string(),
            version: current_os_version(),
        }
    }
}

pub fn current_os() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        // Mojang has always called it "osx", never "macos".
        "macos" => "osx",
        other => other,
    }
}

pub fn current_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "x86" => "x86",
        "aarch64" => "arm64",
        other => other,
    }
}

/// Best-effort OS version for rule matching.
///
/// The only version pattern Mojang ships is `^10\.` on Windows, guarding a pair
/// of cosmetic `-Dos.name` / `-Dos.version` properties that work around old
/// LWJGL behaviour. Windows 10 and Windows 11 both report 10.x, so reporting
/// "10.0" is correct for every Windows version this launcher supports. No
/// other platform uses a version rule, so returning `None` there is accurate
/// rather than a fallback.
fn current_os_version() -> Option<String> {
    if cfg!(target_os = "windows") {
        Some("10.0".to_string())
    } else {
        None
    }
}

/// Whether a rule-gated item applies.
///
/// Mojang's algorithm: an empty rule list allows. Otherwise start from
/// disallowed and let each *matching* rule set the outcome, last match winning.
/// Non-matching rules are skipped entirely — which is why a `disallow` rule for
/// another OS doesn't block anything here.
pub fn rules_allow(rules: &[Rule], platform: &Platform) -> bool {
    if rules.is_empty() {
        return true;
    }

    let mut allowed = false;
    for rule in rules {
        if rule_matches(rule, platform) {
            allowed = rule.action == RuleAction::Allow;
        }
    }
    allowed
}

/// A rule matches when every condition it states is satisfied. A rule with no
/// conditions matches unconditionally.
fn rule_matches(rule: &Rule, platform: &Platform) -> bool {
    if let Some(os) = &rule.os {
        if let Some(name) = &os.name
            && name != &platform.os
        {
            return false;
        }
        if let Some(arch) = &os.arch
            && arch != &platform.arch
        {
            return false;
        }
        if let Some(pattern) = &os.version
            && !version_matches(pattern, platform.version.as_deref())
        {
            return false;
        }
    }

    if let Some(features) = &rule.features {
        // This launcher enables no optional features — no demo mode, no custom
        // resolution, no quick-play. A rule requiring one therefore does not
        // apply, and a rule requiring one to be off does.
        for expected in features.values() {
            if *expected {
                return false;
            }
        }
    }

    true
}

/// Matches the anchored-prefix patterns Mojang actually publishes (`^10\.`).
///
/// Deliberately not a general regex engine: supporting exactly the form in use
/// keeps this dependency-free, and an unrecognised pattern is treated as
/// non-matching rather than silently assumed to apply.
fn version_matches(pattern: &str, version: Option<&str>) -> bool {
    let Some(version) = version else {
        return false;
    };
    let Some(rest) = pattern.strip_prefix('^') else {
        return false;
    };
    let literal = rest.replace("\\.", ".");
    version.starts_with(&literal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta::OsRule;
    use std::collections::HashMap;

    fn platform(os: &str, arch: &str) -> Platform {
        Platform {
            os: os.to_string(),
            arch: arch.to_string(),
            version: if os == "windows" { Some("10.0".into()) } else { None },
        }
    }

    fn rule(action: RuleAction, os: Option<OsRule>) -> Rule {
        Rule { action, os, features: None }
    }

    fn os_rule(name: Option<&str>, arch: Option<&str>, version: Option<&str>) -> OsRule {
        OsRule {
            name: name.map(str::to_string),
            arch: arch.map(str::to_string),
            version: version.map(str::to_string),
        }
    }

    #[test]
    fn no_rules_means_always_included() {
        assert!(rules_allow(&[], &platform("windows", "x86_64")));
    }

    #[test]
    fn an_unconditional_allow_applies_everywhere() {
        let rules = vec![rule(RuleAction::Allow, None)];
        assert!(rules_allow(&rules, &platform("linux", "x86_64")));
    }

    #[test]
    fn os_specific_libraries_only_apply_to_their_os() {
        let rules = vec![rule(RuleAction::Allow, Some(os_rule(Some("windows"), None, None)))];
        assert!(rules_allow(&rules, &platform("windows", "x86_64")));
        assert!(!rules_allow(&rules, &platform("linux", "x86_64")));
        assert!(!rules_allow(&rules, &platform("osx", "arm64")));
    }

    #[test]
    fn allow_all_then_disallow_one_os_is_the_common_exclusion_pattern() {
        let rules = vec![
            rule(RuleAction::Allow, None),
            rule(RuleAction::Disallow, Some(os_rule(Some("osx"), None, None))),
        ];
        assert!(rules_allow(&rules, &platform("windows", "x86_64")));
        assert!(rules_allow(&rules, &platform("linux", "x86_64")));
        assert!(!rules_allow(&rules, &platform("osx", "x86_64")));
    }

    #[test]
    fn architecture_is_matched_alongside_the_os() {
        // Native libraries ship per architecture; Windows on ARM must not pick
        // up the x86_64 natives or the game dies loading them.
        let rules = vec![rule(
            RuleAction::Allow,
            Some(os_rule(Some("windows"), Some("x86_64"), None)),
        )];
        assert!(rules_allow(&rules, &platform("windows", "x86_64")));
        assert!(!rules_allow(&rules, &platform("windows", "arm64")));
    }

    #[test]
    fn all_conditions_in_one_rule_must_hold_together() {
        let rules = vec![rule(
            RuleAction::Allow,
            Some(os_rule(Some("osx"), Some("arm64"), None)),
        )];
        assert!(rules_allow(&rules, &platform("osx", "arm64")));
        assert!(!rules_allow(&rules, &platform("osx", "x86_64")));
        assert!(!rules_allow(&rules, &platform("linux", "arm64")));
    }

    #[test]
    fn windows_version_rules_match_windows_10_and_11() {
        let rules = vec![rule(
            RuleAction::Allow,
            Some(os_rule(Some("windows"), None, Some(r"^10\."))),
        )];
        assert!(rules_allow(&rules, &platform("windows", "x86_64")));

        let ancient = Platform { version: Some("6.1".into()), ..platform("windows", "x86_64") };
        assert!(!rules_allow(&rules, &ancient));
    }

    #[test]
    fn feature_gated_rules_do_not_apply_because_no_features_are_enabled() {
        // `--demo` and custom-resolution arguments are guarded this way; the
        // launcher must never pass them.
        let mut features = HashMap::new();
        features.insert("is_demo_user".to_string(), true);
        let rules = vec![Rule { action: RuleAction::Allow, os: None, features: Some(features) }];
        assert!(!rules_allow(&rules, &platform("windows", "x86_64")));
    }

    #[test]
    fn rules_requiring_a_feature_to_be_off_do_apply() {
        let mut features = HashMap::new();
        features.insert("has_custom_resolution".to_string(), false);
        let rules = vec![Rule { action: RuleAction::Allow, os: None, features: Some(features) }];
        assert!(rules_allow(&rules, &platform("windows", "x86_64")));
    }

    #[test]
    fn a_disallow_for_another_os_blocks_nothing_here() {
        // Non-matching rules are skipped, so this leaves the default of
        // disallowed rather than flipping anything.
        let rules = vec![rule(RuleAction::Disallow, Some(os_rule(Some("osx"), None, None)))];
        assert!(!rules_allow(&rules, &platform("windows", "x86_64")));
    }

    #[test]
    fn the_current_platform_reports_names_mojang_uses() {
        let current = Platform::current();
        assert!(["windows", "osx", "linux"].contains(&current.os.as_str()));
        assert!(["x86", "x86_64", "arm64"].contains(&current.arch.as_str()));
    }

    #[test]
    fn unrecognised_version_patterns_do_not_silently_match() {
        assert!(!version_matches("10.*", Some("10.0")));
        assert!(!version_matches(r"^10\.", None));
    }
}
