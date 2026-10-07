use std::process::{Command, Stdio};

const SIGN_IN_PAGE: &str = "https://www.microsoft.com/link?otc=";

/// Opens a separate browser window without blocking the menu on browser startup.
pub(super) fn open(code: &str) -> bool {
    let Some(url) = sign_in_url(code) else {
        return false;
    };
    std::thread::Builder::new()
        .name("sign-in-browser".into())
        .spawn(move || {
            if !launch(&url) {
                bevy::log::warn!("Microsoft sign-in browser could not open");
            }
        })
        .is_ok()
}

fn sign_in_url(code: &str) -> Option<String> {
    (!code.is_empty()
        && code
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'))
    .then(|| format!("{SIGN_IN_PAGE}{code}"))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Browser {
    Chromium,
    Firefox,
}

#[cfg(any(target_os = "linux", target_os = "windows", test))]
fn browser_kind(desktop: &str) -> Option<Browser> {
    let desktop = desktop.to_ascii_lowercase();
    if desktop.contains("firefox") {
        Some(Browser::Firefox)
    } else if ["chromium", "chrome", "edge", "brave", "vivaldi"]
        .iter()
        .any(|name| desktop.contains(name))
    {
        Some(Browser::Chromium)
    } else {
        None
    }
}

fn popup_args(browser: Browser, url: &str) -> Vec<String> {
    match browser {
        Browser::Chromium => vec![format!("--app={url}"), "--window-size=520,720".into()],
        Browser::Firefox => vec!["--new-window".into(), url.into()],
    }
}

fn spawn_browser(program: &str, args: &[String]) -> bool {
    let Ok(mut child) = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    // The user's browser may share an existing session and must outlive the client.
    child.wait().is_ok_and(|status| status.success())
}

#[cfg(target_os = "linux")]
fn launch(url: &str) -> bool {
    let xdg_settings = Command::new("xdg-settings")
        .args(["get", "default-web-browser"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok());
    let desktop = default_browser(xdg_settings, mimeapps_browser);
    let programs = linux_browsers(desktop.as_deref());
    for (program, browser) in programs {
        if spawn_browser(program, &popup_args(browser, url)) {
            return true;
        }
    }
    crate::desktop::open_with_default(url)
}

/// `xdg-settings` knows desktop-specific settings such as XFCE's helpers, so `mimeapps.list` is
/// read only when it is missing or reports nothing.
#[cfg(any(target_os = "linux", test))]
fn default_browser(
    xdg_settings: Option<String>,
    mimeapps: impl FnOnce() -> Option<String>,
) -> Option<String> {
    xdg_settings
        .map(|entry| entry.trim().to_owned())
        .filter(|entry| !entry.is_empty())
        .or_else(mimeapps)
}

/// The default browser's desktop entry from the `mimeapps.list` files.
#[cfg(target_os = "linux")]
fn mimeapps_browser() -> Option<String> {
    mimeapps_paths(&XdgDirs::from_env())
        .iter()
        .find_map(|path| default_browser_entry(&std::fs::read_to_string(path).ok()?))
}

#[cfg(any(target_os = "linux", test))]
struct XdgDirs {
    config_home: std::path::PathBuf,
    config_dirs: Vec<std::path::PathBuf>,
    data_home: std::path::PathBuf,
    data_dirs: Vec<std::path::PathBuf>,
    /// Lowercased `XDG_CURRENT_DESKTOP` names, most specific first.
    desktops: Vec<String>,
}

#[cfg(target_os = "linux")]
impl XdgDirs {
    fn from_env() -> Self {
        use std::{env, path::PathBuf};
        let var = |name| env::var(name).ok().filter(|value| !value.is_empty());
        let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let list = |name, default: &str| {
            var(name)
                .unwrap_or_else(|| default.to_owned())
                .split(':')
                .filter(|dir| !dir.is_empty())
                .map(PathBuf::from)
                .collect()
        };
        Self {
            config_home: var("XDG_CONFIG_HOME").map_or_else(|| home.join(".config"), PathBuf::from),
            config_dirs: list("XDG_CONFIG_DIRS", "/etc/xdg"),
            data_home: var("XDG_DATA_HOME")
                .map_or_else(|| home.join(".local/share"), PathBuf::from),
            data_dirs: list("XDG_DATA_DIRS", "/usr/local/share:/usr/share"),
            desktops: var("XDG_CURRENT_DESKTOP")
                .unwrap_or_default()
                .split(':')
                .filter(|name| !name.is_empty())
                .map(str::to_ascii_lowercase)
                .collect(),
        }
    }
}

/// `mimeapps.list` files in the XDG lookup order; earlier files win.
#[cfg(any(target_os = "linux", test))]
fn mimeapps_paths(dirs: &XdgDirs) -> Vec<std::path::PathBuf> {
    let configs = std::iter::once(dirs.config_home.clone()).chain(dirs.config_dirs.clone());
    let data = std::iter::once(&dirs.data_home)
        .chain(&dirs.data_dirs)
        .map(|dir| dir.join("applications"));
    configs
        .chain(data)
        .flat_map(|dir| {
            dirs.desktops
                .iter()
                .map(|desktop| format!("{desktop}-mimeapps.list"))
                .chain(std::iter::once("mimeapps.list".to_owned()))
                .map(move |name| dir.join(name))
        })
        .collect()
}

/// The default `http` handler in a `mimeapps.list`, falling back to `https`.
#[cfg(any(target_os = "linux", test))]
fn default_browser_entry(contents: &str) -> Option<String> {
    let mut section = "";
    let mut https = None;
    for line in contents.lines().map(str::trim) {
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            section = name;
            continue;
        }
        if section != "Default Applications" {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let entry = value
            .split(';')
            .map(str::trim)
            .find(|entry| !entry.is_empty());
        match key.trim() {
            "x-scheme-handler/http" if entry.is_some() => return entry.map(str::to_owned),
            "x-scheme-handler/https" if https.is_none() => https = entry.map(str::to_owned),
            _ => {}
        }
    }
    https
}

#[cfg(any(target_os = "linux", test))]
fn linux_browsers(desktop: Option<&str>) -> Vec<(&'static str, Browser)> {
    let mut browsers = vec![
        ("chromium", Browser::Chromium),
        ("chromium-browser", Browser::Chromium),
        ("google-chrome", Browser::Chromium),
        ("google-chrome-stable", Browser::Chromium),
        ("microsoft-edge", Browser::Chromium),
        ("microsoft-edge-stable", Browser::Chromium),
        ("brave-browser", Browser::Chromium),
        ("vivaldi", Browser::Chromium),
        ("firefox", Browser::Firefox),
    ];
    if let Some(desktop) = desktop {
        let kind = browser_kind(desktop);
        let desktop = desktop.to_ascii_lowercase();
        browsers.sort_by_key(|(program, browser)| {
            let name = program
                .trim_end_matches("-stable")
                .trim_end_matches("-browser");
            (!desktop.contains(name), Some(*browser) != kind)
        });
    }
    browsers
}

#[cfg(target_os = "macos")]
fn launch(url: &str) -> bool {
    for browser in ["Google Chrome", "Microsoft Edge", "Chromium"] {
        let mut args = vec!["-na".into(), browser.into(), "--args".into()];
        args.extend(popup_args(Browser::Chromium, url));
        if spawn_browser("open", &args) {
            return true;
        }
    }
    let mut args = vec!["-a".into(), "Firefox".into(), "--args".into()];
    args.extend(popup_args(Browser::Firefox, url));
    spawn_browser("open", &args) || spawn_browser("open", &[url.into()])
}

#[cfg(target_os = "windows")]
fn launch(url: &str) -> bool {
    for program in ["msedge.exe", "chrome.exe", "firefox.exe"] {
        let browser = browser_kind(program).unwrap_or(Browser::Chromium);
        if spawn_browser(program, &popup_args(browser, url)) {
            return true;
        }
    }
    for variable in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
        let Some(root) = std::env::var_os(variable) else {
            continue;
        };
        for (relative, browser) in [
            ("Microsoft/Edge/Application/msedge.exe", Browser::Chromium),
            ("Google/Chrome/Application/chrome.exe", Browser::Chromium),
            ("Mozilla Firefox/firefox.exe", Browser::Firefox),
        ] {
            let program = std::path::PathBuf::from(&root).join(relative);
            if let Some(program) = program.to_str()
                && spawn_browser(program, &popup_args(browser, url))
            {
                return true;
            }
        }
    }
    crate::desktop::open_with_default(url)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn launch(url: &str) -> bool {
    crate::desktop::open_with_default(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_prefills_the_device_code() {
        assert_eq!(
            sign_in_url("AB12-CD34"),
            Some(format!("{SIGN_IN_PAGE}AB12-CD34"))
        );
    }

    #[test]
    fn invalid_codes_never_open_a_browser() {
        for code in [
            "",
            "abc&otc=other",
            "https://example.test",
            "abc def",
            "abc\n",
            "é",
        ] {
            assert!(sign_in_url(code).is_none());
            assert!(!open(code));
        }
    }

    #[test]
    fn browser_commands_open_separate_windows() {
        let url = sign_in_url("AB1234").unwrap();
        assert_eq!(
            popup_args(Browser::Firefox, &url),
            ["--new-window", url.as_str()]
        );
        let args = popup_args(Browser::Chromium, &url);
        assert_eq!(args[0], format!("--app={url}"));
        assert!(args.iter().any(|arg| arg.starts_with("--window-size=")));
    }

    #[test]
    fn installed_default_browser_has_priority() {
        for (desktop, executable) in [
            ("org.mozilla.firefox.desktop", "firefox"),
            ("google-chrome.desktop", "google-chrome"),
            ("microsoft-edge.desktop", "microsoft-edge"),
            ("brave-browser.desktop", "brave-browser"),
        ] {
            assert_eq!(linux_browsers(Some(desktop)).first().unwrap().0, executable);
        }
    }

    #[test]
    fn xdg_settings_wins_and_mimeapps_only_fills_in() {
        let mimeapps = || Some("firefox.desktop".to_owned());
        assert_eq!(
            default_browser(Some("xfce4-web-browser.desktop\n".into()), mimeapps).as_deref(),
            Some("xfce4-web-browser.desktop")
        );
        assert_eq!(
            default_browser(Some(" \n".into()), mimeapps).as_deref(),
            Some("firefox.desktop")
        );
        assert_eq!(
            default_browser(None, mimeapps).as_deref(),
            Some("firefox.desktop")
        );
        assert_eq!(default_browser(None, || None), None);
    }

    #[test]
    fn mimeapps_default_prefers_http_then_https_in_the_default_section() {
        let contents = "[Added Associations]\nx-scheme-handler/http=other.desktop;\n\n\
                        [Default Applications]\nx-scheme-handler/https=brave-browser.desktop\n\
                        x-scheme-handler/http= ;firefox.desktop;chromium.desktop;\n";
        assert_eq!(
            default_browser_entry(contents).as_deref(),
            Some("firefox.desktop")
        );
        let https_only = "[Default Applications]\nx-scheme-handler/https=brave-browser.desktop\n";
        assert_eq!(
            default_browser_entry(https_only).as_deref(),
            Some("brave-browser.desktop")
        );
        assert_eq!(
            default_browser_entry("[Default Applications]\ntext/html=a.desktop\n"),
            None
        );
    }

    #[test]
    fn mimeapps_lookup_puts_user_and_desktop_specific_files_first() {
        let dirs = XdgDirs {
            config_home: "/home/dev/.config".into(),
            config_dirs: vec!["/etc/xdg".into()],
            data_home: "/home/dev/.local/share".into(),
            data_dirs: vec!["/usr/share".into()],
            desktops: vec!["kde".into()],
        };
        let paths = mimeapps_paths(&dirs);
        let expected = [
            "/home/dev/.config/kde-mimeapps.list",
            "/home/dev/.config/mimeapps.list",
            "/etc/xdg/kde-mimeapps.list",
            "/etc/xdg/mimeapps.list",
            "/home/dev/.local/share/applications/kde-mimeapps.list",
            "/home/dev/.local/share/applications/mimeapps.list",
            "/usr/share/applications/kde-mimeapps.list",
            "/usr/share/applications/mimeapps.list",
        ];
        assert_eq!(paths, expected.map(std::path::PathBuf::from).to_vec());
    }
}
