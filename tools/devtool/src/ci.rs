use workspace_declarations::{MACOS, PLATFORMS, WINDOWS, within};

use crate::{
    Package, Selection,
    selection::{changed_packages, normalize, select_packages},
};

/// Workflow definitions; a change to them can change what any job runs on any platform.
const CI_INPUTS: &[&str] = &[".github/workflows", ".github/actions"];

/// The workspace-wide architecture gate: it runs every test but needs no platform runner.
const ARCHITECTURE_GATE: &str = "tools/architecture";

/// The client's asset-carrier integration tests skip without local carriers, so platform jobs
/// run only its library tests.
const CLIENT_PACKAGE: &str = "bedrock-client";

/// Rust work a pull request needs, and the platform runners it needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CiPlan {
    pub selection: Selection,
    pub macos: bool,
    pub windows: bool,
}

/// Plans pull-request checks. A platform runner is needed when a changed crate declares that
/// platform, a shader or workflow changes, or the whole workspace is selected. Reverse
/// dependencies do not count: the client declares both platforms and depends on nearly
/// everything.
#[must_use]
pub fn plan_ci(changed_paths: &[&str], packages: &[Package]) -> CiPlan {
    let touches = |root: &str| {
        changed_paths
            .iter()
            .any(|path| within(&normalize(path), root))
    };
    let ci_input = CI_INPUTS.iter().any(|&root| touches(root));
    let selection = if ci_input || touches(ARCHITECTURE_GATE) {
        Selection::Workspace
    } else {
        select_packages(changed_paths, packages)
    };
    let owners = changed_packages(changed_paths, packages);
    let needs = |platform: &str| {
        selection != Selection::NoPackages
            && (ci_input
                || changed_paths.iter().any(|path| path.ends_with(".wgsl"))
                || owners.as_ref().is_none_or(|owners| {
                    packages.iter().any(|package| {
                        owners.contains(&package.name)
                            && package.platforms.iter().any(|name| name == platform)
                    })
                }))
    };
    CiPlan {
        macos: needs(MACOS),
        windows: needs(WINDOWS),
        selection,
    }
}

impl CiPlan {
    /// Renders `key=value` lines for `$GITHUB_OUTPUT`. Tests are selected with nextest
    /// filtersets rather than `-p`, because Cargo unifies features only across the selected
    /// packages and a subset would miss the cache built for the whole workspace.
    #[must_use]
    pub fn outputs(&self, packages: &[Package]) -> String {
        let (scope, affected): (_, Vec<&Package>) = match &self.selection {
            Selection::Workspace => ("workspace", packages.iter().collect()),
            Selection::Packages(names) => (
                "packages",
                packages
                    .iter()
                    .filter(|package| names.contains(&package.name))
                    .collect(),
            ),
            Selection::NoPackages => ("none", Vec::new()),
        };
        let tests = if self.selection == Selection::Workspace {
            "all()".into()
        } else {
            filterset(affected.iter().map(|package| package.name.as_str()), false)
        };
        let mut outputs = format!(
            "scope={scope}\nmacos={}\nwindows={}\nnames={}\ntests={tests}\n",
            self.macos,
            self.windows,
            affected
                .iter()
                .map(|package| package.name.as_str())
                .collect::<Vec<_>>()
                .join(" "),
        );
        for platform in PLATFORMS {
            let declared = affected
                .iter()
                .filter(|package| package.platforms.iter().any(|name| name == platform))
                .map(|package| package.name.as_str());
            outputs.push_str(&format!("{platform}_tests={}\n", filterset(declared, true)));
        }
        outputs
    }
}

/// Joins nextest `package(=name)` matchers; `client_library` limits the client to its library
/// tests, as platform jobs do. Returns `none()` for no names, which workflow expressions would
/// otherwise treat as unset.
fn filterset<'a>(names: impl Iterator<Item = &'a str>, client_library: bool) -> String {
    let matchers = names
        .map(|name| {
            if client_library && name == CLIENT_PACKAGE {
                format!("(package(={name}) & kind(lib))")
            } else {
                format!("package(={name})")
            }
        })
        .collect::<Vec<_>>();
    if matchers.is_empty() {
        "none()".into()
    } else {
        matchers.join(" | ")
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, path::Path, process::Command};

    use super::{CLIENT_PACKAGE, PLATFORMS, plan_ci};
    use crate::{Package, Selection, packages_from_metadata};

    fn declares(package: Package, platforms: &[&str]) -> Package {
        package.with_platforms(platforms.iter().map(|name| (*name).into()).collect())
    }

    fn workspace() -> Vec<Package> {
        vec![
            Package::new("world", "crates/world", &[]),
            declares(
                Package::new("render", "crates/render", &["world"]),
                &["macos", "windows"],
            ),
            declares(
                Package::new("launcher", "crates/launcher", &[]),
                &["windows"],
            ),
            declares(
                Package::new("bedrock-client", "app", &["render", "launcher"]),
                &["macos", "windows"],
            ),
            Package::new("devtool", "tools/devtool", &[]),
            Package::new("architecture", "tools/architecture", &[]),
        ]
    }

    #[test]
    fn workflow_changes_check_everything_on_every_platform() {
        for path in [
            ".github/workflows/ci.yml",
            ".github/actions/build-cache/cache.py",
        ] {
            let plan = plan_ci(&[path], &workspace());
            assert_eq!(plan.selection, Selection::Workspace, "{path}");
            assert!(plan.macos && plan.windows, "{path}");
        }
    }

    #[test]
    fn architecture_gate_changes_check_everything_without_platform_runners() {
        let plan = plan_ci(&["tools/architecture/policy.toml"], &workspace());
        assert_eq!(plan.selection, Selection::Workspace);
        assert!(!plan.macos && !plan.windows);
    }

    #[test]
    fn unclassified_and_workspace_inputs_need_every_platform() {
        for path in ["Cargo.lock", "unexpected/input.txt"] {
            let plan = plan_ci(&[path], &workspace());
            assert_eq!(plan.selection, Selection::Workspace, "{path}");
            assert!(plan.macos && plan.windows, "{path}");
        }
    }

    #[test]
    fn changed_crates_declarations_choose_platform_runners() {
        let packages = workspace();
        let leaf = plan_ci(&["crates/world/src/lib.rs"], &packages);
        assert_eq!(
            leaf.selection,
            Selection::Packages(vec![
                "bedrock-client".into(),
                "render".into(),
                "world".into()
            ])
        );
        assert!(
            !leaf.macos && !leaf.windows,
            "declaring reverse dependencies alone need no platform runner"
        );
        let launcher = plan_ci(&["crates/launcher/src/lib.rs"], &packages);
        assert!(!launcher.macos && launcher.windows);
        let client = plan_ci(&["app/src/main.rs"], &packages);
        assert!(client.macos && client.windows);
        let shader = plan_ci(&["crates/world/shaders/sky.wgsl"], &packages);
        assert!(shader.macos && shader.windows);
    }

    #[test]
    fn rust_free_changes_need_no_rust_jobs() {
        let plan = plan_ci(
            &["docs/guide.md", "packaging/macos/Info.plist.in"],
            &workspace(),
        );
        assert_eq!(plan.selection, Selection::NoPackages);
        assert!(!plan.macos && !plan.windows);
        assert!(plan.outputs(&workspace()).contains("scope=none\n"));
    }

    #[test]
    fn outputs_select_affected_and_platform_tests_by_package() {
        let packages = workspace();
        let outputs = plan_ci(&["crates/render/src/lib.rs"], &packages).outputs(&packages);
        assert_eq!(
            outputs,
            "scope=packages\nmacos=true\nwindows=true\nnames=render bedrock-client\n\
             tests=package(=render) | package(=bedrock-client)\n\
             macos_tests=package(=render) | (package(=bedrock-client) & kind(lib))\n\
             windows_tests=package(=render) | (package(=bedrock-client) & kind(lib))\n"
        );
        let full = plan_ci(&["Cargo.toml"], &packages).outputs(&packages);
        assert!(full.contains("\ntests=all()\n"), "{full}");
        assert!(
            full.contains(
                "\nwindows_tests=package(=render) | package(=launcher) | (package(=bedrock-client) & kind(lib))\n"
            ),
            "{full}"
        );
        let leaf = plan_ci(&["tools/devtool/src/ci.rs"], &packages).outputs(&packages);
        assert!(
            leaf.ends_with(
                "\nnames=devtool\ntests=package(=devtool)\nmacos_tests=none()\nwindows_tests=none()\n"
            ),
            "{leaf}"
        );
    }

    /// The client's integration tests skip without carriers, so Linux still runs them.
    #[test]
    fn linux_tests_keep_client_integration_tests() {
        let packages = workspace();
        let outputs = plan_ci(&["app/tests/client_logging.rs"], &packages).outputs(&packages);
        assert!(
            outputs.contains("\ntests=package(=bedrock-client)\n"),
            "{outputs}"
        );
        assert!(
            outputs.contains("\nmacos_tests=(package(=bedrock-client) & kind(lib))\n"),
            "{outputs}"
        );
    }

    /// The real workspace's declarations use only platforms CI has runners for.
    #[test]
    fn workspace_declarations_name_known_platforms_and_packages() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let output = Command::new(env!("CARGO"))
            .current_dir(root)
            .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
            .output()
            .expect("run cargo metadata");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let packages = packages_from_metadata(&String::from_utf8(output.stdout).unwrap()).unwrap();
        let names: BTreeSet<_> = packages
            .iter()
            .map(|package| package.name.as_str())
            .collect();
        assert!(names.contains(CLIENT_PACKAGE));
        for package in &packages {
            for platform in &package.platforms {
                assert!(
                    PLATFORMS.contains(&platform.as_str()),
                    "{}: {platform}",
                    package.name
                );
            }
        }
        assert!(packages.iter().any(|package| !package.platforms.is_empty()));
    }
}
