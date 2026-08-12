//! Building the command line and running the game.
//!
//! Argument construction is a pure function of the version metadata and the
//! session, deliberately separated from spawning. Nearly every "the game won't
//! start" report comes down to one wrong argument, and a pure builder is
//! something tests can pin down exactly.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt as _, BufReader};
use tokio::sync::mpsc;

use crate::install::ResolvedVersion;
use crate::java::JavaRuntime;
use crate::meta::{Argument, VersionKind};
use crate::rules::{Platform, rules_allow};

/// Reported to the game as the launcher brand, and visible in crash reports.
pub const LAUNCHER_NAME: &str = "cagalintry";
pub const LAUNCHER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("starting the game: {0}")]
    Spawn(#[source] std::io::Error),

    #[error("creating {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// The authenticated player. Every field comes from the Microsoft login chain;
/// there is no offline path that fabricates them.
#[derive(Debug, Clone)]
pub struct LaunchSession {
    pub player_name: String,
    /// Canonical account UUID. Dashes are stripped when passed to the game,
    /// which is the form the vanilla launcher uses.
    pub uuid: String,
    pub access_token: String,
    pub xuid: String,
    pub client_id: String,
}

impl LaunchSession {
    fn undashed_uuid(&self) -> String {
        self.uuid.replace('-', "")
    }
}

#[derive(Debug, Clone)]
pub struct LaunchOptions {
    pub session: LaunchSession,
    pub game_dir: PathBuf,
    /// Maximum heap, in mebibytes.
    pub max_memory_mb: u32,
    pub min_memory_mb: u32,
    /// Appended after the version's own JVM arguments, so a player override
    /// wins over the default.
    pub extra_jvm_args: Vec<String>,
    pub extra_game_args: Vec<String>,
}

impl LaunchOptions {
    pub fn new(session: LaunchSession, game_dir: impl Into<PathBuf>) -> Self {
        Self {
            session,
            game_dir: game_dir.into(),
            // Comfortable for a modded pack without inviting the GC pauses
            // that come from handing the JVM most of the machine.
            max_memory_mb: 8192,
            min_memory_mb: 512,
            extra_jvm_args: Vec::new(),
            extra_game_args: Vec::new(),
        }
    }
}

/// A fully built command line: the JVM to run, and everything to pass it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub working_dir: PathBuf,
}

impl LaunchCommand {
    /// Rendering for logs, with the access token redacted. The token is a live
    /// credential and log files get pasted into chats.
    pub fn to_redacted_string(&self) -> String {
        let args: Vec<String> = self
            .args
            .iter()
            .map(|arg| {
                if arg.len() > 40 && arg.contains('.') && arg.starts_with("ey") {
                    "<access-token>".to_string()
                } else {
                    arg.clone()
                }
            })
            .collect();
        format!("{} {}", self.program.display(), args.join(" "))
    }
}

/// Build the command line for a resolved version.
pub fn build_command(
    resolved: &ResolvedVersion,
    java: &JavaRuntime,
    options: &LaunchOptions,
    assets_root: &Path,
    platform: &Platform,
) -> LaunchCommand {
    let classpath_separator = if platform.os == "windows" { ";" } else { ":" };
    let classpath = resolved
        .classpath
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(classpath_separator);

    let assets_index = resolved.asset_index.id.clone();
    let vars: HashMap<&str, String> = HashMap::from([
        ("auth_player_name", options.session.player_name.clone()),
        ("auth_uuid", options.session.undashed_uuid()),
        ("auth_access_token", options.session.access_token.clone()),
        ("auth_xuid", options.session.xuid.clone()),
        ("clientid", options.session.client_id.clone()),
        ("auth_session", format!("token:{}", options.session.access_token)),
        ("user_type", "msa".to_string()),
        ("user_properties", "{}".to_string()),
        ("version_name", resolved.detail.id.clone()),
        ("version_type", version_type(resolved).to_string()),
        ("game_directory", options.game_dir.display().to_string()),
        ("assets_root", assets_root.display().to_string()),
        ("game_assets", assets_root.join("virtual").join(&assets_index).display().to_string()),
        ("assets_index_name", assets_index),
        ("natives_directory", resolved.natives_dir.display().to_string()),
        ("launcher_name", LAUNCHER_NAME.to_string()),
        ("launcher_version", LAUNCHER_VERSION.to_string()),
        ("classpath", classpath.clone()),
        ("classpath_separator", classpath_separator.to_string()),
        ("library_directory", library_root(resolved).display().to_string()),
    ]);

    let mut args = Vec::new();

    // Heap settings come first so a version's own JVM arguments, and then the
    // player's overrides, can still contradict them if they really mean to.
    args.push(format!("-Xmx{}M", options.max_memory_mb));
    args.push(format!("-Xms{}M", options.min_memory_mb));

    match resolved.detail.arguments.as_ref() {
        Some(arguments) => {
            args.extend(expand_arguments(&arguments.jvm, &vars, platform));
        }
        None => {
            // Pre-1.13 versions describe no JVM arguments at all, so the
            // essentials have to be supplied here or the game cannot find its
            // natives or its own classes.
            args.push(format!("-Djava.library.path={}", resolved.natives_dir.display()));
            args.push("-cp".to_string());
            args.push(classpath.clone());
        }
    }

    args.extend(options.extra_jvm_args.iter().cloned());
    args.push(resolved.main_class.clone());

    match resolved.detail.arguments.as_ref() {
        Some(arguments) => {
            args.extend(expand_arguments(&arguments.game, &vars, platform));
        }
        None => {
            if let Some(legacy) = &resolved.detail.minecraft_arguments {
                args.extend(legacy.split_whitespace().map(|token| substitute(token, &vars)));
            }
        }
    }

    args.extend(options.extra_game_args.iter().cloned());

    LaunchCommand {
        program: java.executable.clone(),
        args,
        working_dir: options.game_dir.clone(),
    }
}

fn version_type(resolved: &ResolvedVersion) -> &'static str {
    match resolved.detail.kind {
        Some(VersionKind::Release) | None => "release",
        Some(VersionKind::Snapshot) => "snapshot",
        Some(VersionKind::OldBeta) => "old_beta",
        Some(VersionKind::OldAlpha) => "old_alpha",
    }
}

/// The shared library root, inferred from any classpath entry. Only Forge-style
/// argument templates reference it.
fn library_root(resolved: &ResolvedVersion) -> PathBuf {
    resolved
        .classpath
        .first()
        .and_then(|p| p.ancestors().find(|a| a.ends_with("libraries")))
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

fn expand_arguments(
    arguments: &[Argument],
    vars: &HashMap<&str, String>,
    platform: &Platform,
) -> Vec<String> {
    let mut out = Vec::new();
    for argument in arguments {
        match argument {
            Argument::Literal(value) => out.push(substitute(value, vars)),
            Argument::Conditional { rules, value } => {
                if rules_allow(rules, platform) {
                    out.extend(value.as_slice().iter().map(|v| substitute(v, vars)));
                }
            }
        }
    }
    out
}

/// Replace every `${name}` for which we have a value.
///
/// Unknown placeholders are left as-is rather than blanked: an argument that
/// still visibly reads `${some_new_field}` in a crash report is far easier to
/// diagnose than one that silently became empty.
fn substitute(template: &str, vars: &HashMap<&str, String>) -> String {
    if !template.contains("${") {
        return template.to_string();
    }

    let mut out = String::with_capacity(template.len());
    let mut rest = template;

    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];

        match after.find('}') {
            Some(end) => {
                let name = &after[..end];
                match vars.get(name) {
                    Some(value) => out.push_str(value),
                    None => {
                        out.push_str("${");
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                // Unterminated — emit the remainder verbatim.
                out.push_str(&rest[start..]);
                return out;
            }
        }
    }

    out.push_str(rest);
    out
}

/// A line of output from the running game.
#[derive(Debug, Clone)]
pub struct GameOutput {
    pub line: String,
    pub is_stderr: bool,
}

/// Start the game.
///
/// Output is piped rather than inherited so the launcher can show logs and
/// explain crashes. That is also why the console `java` binary is used instead
/// of `javaw` — `javaw` writes nowhere.
pub async fn spawn(
    command: &LaunchCommand,
    output: Option<mpsc::UnboundedSender<GameOutput>>,
) -> Result<tokio::process::Child, LaunchError> {
    tokio::fs::create_dir_all(&command.working_dir)
        .await
        .map_err(|source| LaunchError::Io {
            path: command.working_dir.display().to_string(),
            source,
        })?;

    let mut builder = tokio::process::Command::new(&command.program);
    builder
        .args(&command.args)
        .current_dir(&command.working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());

    // Without this the piped console binary still flashes a window on Windows.
    // tokio's Command exposes creation_flags directly, so no extension trait.
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        builder.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = builder.spawn().map_err(LaunchError::Spawn)?;

    if let Some(sender) = output {
        if let Some(stdout) = child.stdout.take() {
            pump(stdout, sender.clone(), false);
        }
        if let Some(stderr) = child.stderr.take() {
            pump(stderr, sender, true);
        }
    }

    Ok(child)
}

/// Drain one of the child's pipes into the output channel.
///
/// This must keep running even if nobody is listening: a full pipe buffer
/// blocks the game itself, which looks exactly like a freeze.
fn pump<R>(reader: R, sender: mpsc::UnboundedSender<GameOutput>, is_stderr: bool)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if sender.send(GameOutput { line, is_stderr }).is_err() {
                // Receiver dropped; keep draining so the game never blocks.
                continue;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::ResolvedVersion;
    use crate::java::{JavaRuntime, JavaSource};
    use crate::meta::{
        Argument, ArgumentValue, AssetIndexRef, Arguments, OsRule, Rule, RuleAction, VersionDetail,
    };

    fn platform() -> Platform {
        Platform { os: "windows".into(), arch: "x86_64".into(), version: Some("10.0".into()) }
    }

    fn session() -> LaunchSession {
        LaunchSession {
            player_name: "Ricco".into(),
            uuid: "069a79f4-44e9-4726-a5be-fca90e38aaf5".into(),
            access_token: "eyJhbGciOiJ.token.signature-that-is-long-enough-to-redact".into(),
            xuid: "2535".into(),
            client_id: "client".into(),
        }
    }

    fn resolved(arguments: Option<Arguments>, legacy: Option<&str>) -> ResolvedVersion {
        let detail = VersionDetail {
            id: "1.21.4".into(),
            inherits_from: None,
            main_class: Some("net.minecraft.client.main.Main".into()),
            asset_index: Some(AssetIndexRef {
                id: "17".into(),
                sha1: "b".repeat(40),
                size: 1,
                url: "https://example.test/17.json".into(),
                total_size: None,
            }),
            assets: Some("17".into()),
            downloads: None,
            java_version: None,
            libraries: Vec::new(),
            arguments,
            minecraft_arguments: legacy.map(str::to_string),
            kind: None,
        };

        ResolvedVersion {
            classpath: vec![
                PathBuf::from("C:/data/libraries/a/b/1/b-1.jar"),
                PathBuf::from("C:/data/meta/versions/1.21.4/1.21.4.jar"),
            ],
            natives: Vec::new(),
            natives_dir: PathBuf::from("C:/data/natives/1.21.4"),
            natives_extract_dir: PathBuf::from("C:/data/natives/1.21.4"),
            client_jar: PathBuf::from("C:/data/meta/versions/1.21.4/1.21.4.jar"),
            asset_index: detail.asset_index.clone().unwrap(),
            main_class: "net.minecraft.client.main.Main".into(),
            detail,
            downloads: Vec::new(),
        }
    }

    fn java() -> JavaRuntime {
        JavaRuntime {
            executable: PathBuf::from("C:/data/java/java-runtime-delta/bin/java.exe"),
            major_version: 21,
            source: JavaSource::Mojang,
        }
    }

    fn build(resolved: &ResolvedVersion) -> LaunchCommand {
        build_command(
            resolved,
            &java(),
            &LaunchOptions::new(session(), "C:/data/packs/x/minecraft"),
            Path::new("C:/data/assets"),
            &platform(),
        )
    }

    #[test]
    fn substitutes_known_placeholders() {
        let vars = HashMap::from([("name", "Ricco".to_string())]);
        assert_eq!(substitute("--user ${name}", &vars), "--user Ricco");
        assert_eq!(substitute("${name}${name}", &vars), "RiccoRicco");
        assert_eq!(substitute("no placeholders", &vars), "no placeholders");
    }

    #[test]
    fn leaves_unknown_placeholders_visible() {
        // Blanking them would turn "we don't support this yet" into a silently
        // malformed command line.
        let vars = HashMap::from([("known", "yes".to_string())]);
        assert_eq!(substitute("${unknown}", &vars), "${unknown}");
        assert_eq!(substitute("${known}/${unknown}", &vars), "yes/${unknown}");
        assert_eq!(substitute("${unterminated", &vars), "${unterminated");
    }

    #[test]
    fn the_command_is_java_then_jvm_args_then_main_class_then_game_args() {
        let arguments = Arguments {
            jvm: vec![
                Argument::Literal("-Djava.library.path=${natives_directory}".into()),
                Argument::Literal("-cp".into()),
                Argument::Literal("${classpath}".into()),
            ],
            game: vec![
                Argument::Literal("--username".into()),
                Argument::Literal("${auth_player_name}".into()),
            ],
        };
        let command = build(&resolved(Some(arguments), None));

        assert!(command.program.ends_with("java.exe"));

        let main_class_at = command
            .args
            .iter()
            .position(|a| a == "net.minecraft.client.main.Main")
            .expect("main class must be present");
        let cp_at = command.args.iter().position(|a| a == "-cp").unwrap();
        let username_at = command.args.iter().position(|a| a == "--username").unwrap();

        assert!(cp_at < main_class_at, "JVM arguments must precede the main class");
        assert!(main_class_at < username_at, "game arguments must follow the main class");
    }

    #[test]
    fn the_classpath_uses_the_platform_separator() {
        let arguments = Arguments {
            jvm: vec![Argument::Literal("${classpath}".into())],
            game: vec![],
        };
        let version = resolved(Some(arguments), None);

        let windows = build_command(
            &version,
            &java(),
            &LaunchOptions::new(session(), "C:/game"),
            Path::new("C:/data/assets"),
            &platform(),
        );
        assert!(windows.args.iter().any(|a| a.contains(';')));

        let linux_platform = Platform { os: "linux".into(), ..platform() };
        let linux = build_command(
            &version,
            &java(),
            &LaunchOptions::new(session(), "/game"),
            Path::new("/data/assets"),
            &linux_platform,
        );
        assert!(linux.args.iter().any(|a| a.contains(':') && a.contains(".jar")));
        assert!(!linux.args.iter().any(|a| a.contains(";")));
    }

    #[test]
    fn the_uuid_is_passed_without_dashes() {
        // The vanilla launcher passes the compact form and the game expects it.
        let arguments = Arguments {
            jvm: vec![],
            game: vec![Argument::Literal("${auth_uuid}".into())],
        };
        let command = build(&resolved(Some(arguments), None));
        assert!(command.args.contains(&"069a79f444e94726a5befca90e38aaf5".to_string()));
    }

    #[test]
    fn heap_limits_are_always_set() {
        let command = build(&resolved(Some(Arguments::default()), None));
        assert!(command.args.iter().any(|a| a.starts_with("-Xmx")));
        assert!(command.args.iter().any(|a| a.starts_with("-Xms")));
    }

    #[test]
    fn platform_specific_jvm_arguments_are_filtered() {
        let arguments = Arguments {
            jvm: vec![Argument::Conditional {
                rules: vec![Rule {
                    action: RuleAction::Allow,
                    os: Some(OsRule { name: Some("osx".into()), version: None, arch: None }),
                    features: None,
                }],
                value: ArgumentValue::Single("-XstartOnFirstThread".into()),
            }],
            game: vec![],
        };
        let version = resolved(Some(arguments), None);

        let on_windows = build(&version);
        assert!(!on_windows.args.contains(&"-XstartOnFirstThread".to_string()));

        let mac = Platform { os: "osx".into(), arch: "arm64".into(), version: None };
        let on_mac = build_command(
            &version,
            &java(),
            &LaunchOptions::new(session(), "/game"),
            Path::new("/assets"),
            &mac,
        );
        assert!(on_mac.args.contains(&"-XstartOnFirstThread".to_string()));
    }

    #[test]
    fn demo_arguments_are_never_passed() {
        // Guarded by a feature flag this launcher does not enable.
        let mut features = HashMap::new();
        features.insert("is_demo_user".to_string(), true);
        let arguments = Arguments {
            jvm: vec![],
            game: vec![Argument::Conditional {
                rules: vec![Rule { action: RuleAction::Allow, os: None, features: Some(features) }],
                value: ArgumentValue::Single("--demo".into()),
            }],
        };
        let command = build(&resolved(Some(arguments), None));
        assert!(!command.args.contains(&"--demo".to_string()));
    }

    #[test]
    fn pre_1_13_versions_get_a_usable_command_line_anyway() {
        // They declare no JVM arguments, so the classpath and native path must
        // be supplied or the game cannot start at all.
        let command = build(&resolved(None, Some("--username ${auth_player_name} --version ${version_name}")));

        assert!(command.args.iter().any(|a| a.starts_with("-Djava.library.path=")));
        assert!(command.args.contains(&"-cp".to_string()));
        assert!(command.args.contains(&"Ricco".to_string()));
        assert!(command.args.contains(&"1.21.4".to_string()));
    }

    #[test]
    fn the_access_token_is_redacted_in_log_output() {
        // Log files get pasted into chats; a live token must not ride along.
        let arguments = Arguments {
            jvm: vec![],
            game: vec![Argument::Literal("${auth_access_token}".into())],
        };
        let command = build(&resolved(Some(arguments), None));

        assert!(command.args.iter().any(|a| a.contains("signature-that-is-long-enough")));
        let redacted = command.to_redacted_string();
        assert!(!redacted.contains("signature-that-is-long-enough"));
        assert!(redacted.contains("<access-token>"));
    }

    #[test]
    fn player_overrides_come_after_the_versions_own_arguments() {
        let mut options = LaunchOptions::new(session(), "C:/game");
        options.extra_jvm_args = vec!["-XX:+UseZGC".into()];
        let version = resolved(Some(Arguments::default()), None);

        let command = build_command(
            &version,
            &java(),
            &options,
            Path::new("C:/data/assets"),
            &platform(),
        );

        let override_at = command.args.iter().position(|a| a == "-XX:+UseZGC").unwrap();
        let main_at = command
            .args
            .iter()
            .position(|a| a == "net.minecraft.client.main.Main")
            .unwrap();
        assert!(override_at < main_at, "JVM overrides must stay in the JVM section");
    }
}
