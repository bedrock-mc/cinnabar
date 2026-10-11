//! Complete failure evidence: unknown output never contributes a partial signature set.
use super::{junit_failures, numbered_signatures};
use crate::{CommandSpec, DevtoolError};
use regex::Regex;
use std::{collections::BTreeSet, fs, path::Path, process::Output};

/// Returns signatures only when all failure output belongs to a supported diagnostic grammar.
pub(super) fn signatures(
    command: &CommandSpec,
    result: &Output,
    report: Option<&Path>,
    root: &Path,
    architecture_selector: &str,
) -> Result<Option<BTreeSet<String>>, DevtoolError> {
    let (Ok(stdout), Ok(stderr)) = (
        std::str::from_utf8(&result.stdout),
        std::str::from_utf8(&result.stderr),
    ) else {
        return Ok(None);
    };
    if result.status.code().is_none() {
        return Ok(None);
    }
    let text = format!("{stdout}\n{stderr}");
    if let Some(report) = report {
        let Ok(xml) = fs::read_to_string(report) else {
            return Ok(None);
        };
        // Errors include process aborts and timeouts; these are not ordinary failed tests.
        if xml.contains("<error") {
            return Ok(None);
        }
        let failures = junit_failures(&xml)?;
        return Ok(nextest_output(&text, &failures).then_some(failures));
    }
    if command.program == "cargo" {
        match command.args.first().map(String::as_str) {
            Some("test") => return Ok(cargo_tests(&text, root)),
            Some("clippy") => return Ok(clippy(&text, root)),
            Some("fmt") => return Ok(formatting(&text, root)),
            _ => {}
        }
        if command.args.first().is_some_and(|arg| arg == "run")
            && command
                .args
                .split(|arg| arg == "--")
                .next()
                .unwrap_or_default()
                .windows(2)
                .any(|pair| {
                    (pair[0] == "-p" || pair[0] == "--package") && pair[1] == architecture_selector
                })
        {
            return Ok(architecture(&text));
        }
    }
    if command.program == "go" && command.args.iter().any(|arg| arg == "test") {
        return Ok(go_tests(&text));
    }
    Ok(None)
}

/// Recognizes Cargo's progress lines, never its diagnostics or failure explanations.
fn cargo_progress(line: &str) -> bool {
    static PROGRESS: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    PROGRESS.get_or_init(|| Regex::new(
        r"^(?:(?:Compiling|Checking|Fresh) \S+ v\d\S*(?: \(.+\))?|Finished `[^`]+` profile \[[^\]]+\] target\(s\) in [\d.hms ]+|Running (?:`[^`]+`|(?:unittests|tests/).+ \(.+\))|Blocking waiting for file lock on (?:artifact directory|package cache|build directory)|Updating (?:crates.io index|git repository `[^`]+`)|Downloading crates \.\.\.|Downloaded \S+ v\d\S*(?: \(.+\))?|Locking \d+ packages? to .+|Adding \S+ v\d\S*)$"
    ).unwrap()).is_match(line)
}

/// Classifies every Clippy error event and requires matching Cargo failure summaries.
fn clippy(text: &str, root: &Path) -> Option<BTreeSet<String>> {
    let summary = Regex::new(r"^error: could not compile `[^`]+`(?: \([^)]*\))? due to (\d+) previous errors?(?:; .*warnings? emitted)?$").unwrap();
    let mut errors = Vec::new();
    let mut summarized = 0;
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            match value["reason"].as_str()? {
                "compiler-message" => {
                    let message = &value["message"];
                    match message["level"].as_str()? {
                        "error" => {
                            message["message"].as_str()?;
                            message["spans"].as_array()?;
                            let mut diagnostic = message.clone();
                            diagnostic.as_object_mut()?.remove("rendered");
                            let encoded_root =
                                serde_json::to_string(&root.to_string_lossy()).ok()?;
                            errors.push(
                                diagnostic
                                    .to_string()
                                    .replace(&encoded_root[1..encoded_root.len() - 1], "{root}"),
                            );
                        }
                        "warning" | "note" | "help" => {}
                        "failure-note"
                            if message["message"]
                                .as_str()?
                                .starts_with("For more information about this error") => {}
                        _ => return None,
                    }
                }
                "compiler-artifact" | "build-script-executed" => {}
                "build-finished" if value["success"] == false => {}
                _ => return None,
            }
        } else if let Some(capture) = summary.captures(line) {
            summarized += capture[1].parse::<usize>().ok()?;
        } else if !cargo_progress(line) {
            return None;
        }
    }
    (!errors.is_empty() && summarized == errors.len()).then(|| numbered_signatures(errors))
}

/// Requires complete stable-libtest framing and rejects unknown output outside failed test bodies.
fn cargo_tests(text: &str, root: &Path) -> Option<BTreeSet<String>> {
    let failures = crate::cargo_failures::parse(text, root);
    if failures.is_empty() {
        return None;
    }
    let mut body = false;
    let result = Regex::new(r"^test result: (?:ok|FAILED)\. \d+ passed; \d+ failed; \d+ ignored; \d+ measured; \d+ filtered out; finished in [\d.]+s$").unwrap();
    let target = Regex::new(r"^`?(?:-p \S+ )?--(?:lib|test \S+|bin \S+|example \S+)`?$").unwrap();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            match value["reason"].as_str()? {
                "compiler-artifact" | "build-script-executed" => continue,
                "build-finished" if value["success"] == true => continue,
                "compiler-message" if value["message"]["level"] == "warning" => continue,
                _ => return None,
            }
        }
        if trimmed.starts_with("---- ") && trimmed.ends_with(" stdout ----") {
            let name = trimmed
                .strip_prefix("---- ")
                .unwrap()
                .strip_suffix(" stdout ----")
                .unwrap();
            if !failures.iter().any(|signature| {
                serde_json::from_str::<serde_json::Value>(signature)
                    .ok()
                    .is_some_and(|value| value["test"] == name)
            }) {
                return None;
            }
            body = true;
            continue;
        }
        if trimmed == "failures:" || result.is_match(trimmed) {
            body = false;
            continue;
        }
        if trimmed.starts_with("test result:") {
            return None;
        }
        if cargo_progress(trimmed)
            || trimmed.starts_with("running ")
                && (trimmed.ends_with(" test") || trimmed.ends_with(" tests"))
            || trimmed.starts_with("test ")
                && [" ... ok", " ... FAILED", " ... ignored"]
                    .iter()
                    .any(|suffix| trimmed.ends_with(suffix))
            || trimmed.starts_with("error: test failed, to rerun pass ")
            || trimmed.starts_with("error: ")
                && (trimmed.ends_with(" target failed:") || trimmed.ends_with(" targets failed:"))
            || target.is_match(trimmed)
        {
            continue;
        }
        if body && !trimmed.starts_with("error:") {
            continue;
        }
        if line.starts_with("    ")
            && failures.iter().any(|signature| {
                serde_json::from_str::<serde_json::Value>(signature)
                    .ok()
                    .is_some_and(|value| value["test"] == trimmed)
            })
        {
            continue;
        }
        return None;
    }
    Some(failures)
}

/// Requires every Go event to be recognized and every package failure to have failed tests.
fn go_tests(text: &str) -> Option<BTreeSet<String>> {
    let mut failures = BTreeSet::new();
    let mut packages = BTreeSet::new();
    let mut accounted = BTreeSet::new();
    let mut running = BTreeSet::new();
    let summary = Regex::new(
        r"^(?:FAIL(?:\t\S+\t[\d.]+s)?|ok\s+\S+\s+[\d.]+s|\?\s+\S+\s+\[no test files\])\n$",
    )
    .unwrap();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let value: serde_json::Value = serde_json::from_str(line).ok()?;
        let package = value["Package"].as_str()?.to_owned();
        let test = value["Test"].as_str().filter(|test| !test.is_empty());
        let identity = test.map(|test| format!("{package}\t{test}"));
        match value["Action"].as_str()? {
            "run" => {
                running.insert(identity?);
            }
            "fail" => {
                if let Some(identity) = identity {
                    running.remove(&identity);
                    failures.insert(identity);
                    accounted.insert(package);
                } else {
                    if running
                        .iter()
                        .any(|test| test.starts_with(&format!("{package}\t")))
                    {
                        return None;
                    }
                    packages.insert(package);
                }
            }
            "pass" | "skip" => {
                if let Some(identity) = identity {
                    running.remove(&identity);
                }
            }
            "output" => {
                let output = value["Output"].as_str()?;
                if let Some(identity) = identity {
                    if !running.contains(&identity) {
                        return None;
                    }
                } else if !summary.is_match(output) {
                    return None;
                }
            }
            "pause" | "cont" if running.contains(identity.as_ref()?) => {}
            "start" => {}
            _ => return None,
        }
    }
    (running.is_empty() && packages == accounted && !failures.is_empty()).then_some(failures)
}

/// Classifies only recognized architecture diagnostics; unsupported forms remain strict.
fn architecture(text: &str) -> Option<BTreeSet<String>> {
    let known = Regex::new(
        r"^(?:\S+: forbidden dependency path `[^`]+`|workspace member `[^`]+` has no crate rule)$",
    )
    .unwrap();
    let mut failures = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if cargo_progress(line) {
            continue;
        }
        if !known.is_match(line) {
            return None;
        }
        failures.push(line.into());
    }
    (!failures.is_empty()).then(|| numbered_signatures(failures))
}

/// Classifies complete rustfmt diff blocks, rejecting unrelated diagnostics before or after them.
fn formatting(text: &str, root: &Path) -> Option<BTreeSet<String>> {
    let mut diffs = Vec::new();
    let mut current = None::<String>;
    let header = Regex::new(r"^Diff in .+:\d+:$").unwrap();
    let number = Regex::new(r":\d+:$").unwrap();
    for line in text.lines() {
        if header.is_match(line) {
            if let Some(diff) = current.take() {
                diffs.push(diff)
            }
            current = Some(
                number
                    .replace(&line.replace(&*root.to_string_lossy(), "{root}"), ":")
                    .into_owned(),
            );
        } else if line.is_empty() || line.starts_with([' ', '+', '-']) {
            if let Some(diff) = &mut current {
                diff.push('\n');
                diff.push_str(line);
            } else if !line.is_empty() {
                return None;
            }
        } else {
            return None;
        }
    }
    if let Some(diff) = current {
        diffs.push(diff)
    }
    (!diffs.is_empty()).then(|| numbered_signatures(diffs))
}

/// Associates nextest's failure detail blocks with reported tests and rejects tool-level errors.
fn nextest_output(text: &str, failures: &BTreeSet<String>) -> bool {
    let status =
        Regex::new(r"^(FAIL|PASS|SKIP)\s+\[[\d.hms ]*\]\s+(?:\(\d+/\d+\)\s+)?(\S+)\s+(\S+)$")
            .unwrap();
    let run = Regex::new(r"^Nextest run ID [0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12} with nextest profile: [A-Za-z0-9_-]+$").unwrap();
    let header = Regex::new(r"^---- (?:STDOUT|STDERR):\s+\[[\d.hms ]+\]\s+(\S+)\s+(\S+)$").unwrap();
    let starting =
        Regex::new(r"^Starting \d+ tests? across \d+ binar(?:y|ies)(?: \(\d+ tests? skipped\))?$")
            .unwrap();
    let summary = Regex::new(r"^Summary\s+\[[\d.hms ]+\]\s+\d+ tests? run: (?:(?:\d+ passed(?: \(\d+ flaky\))?|\d+ failed), )*\d+ skipped$").unwrap();
    let mut detail = false;
    let mut identified = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line.chars().count() >= 12 && line.chars().all(|character| character == '─') {
            detail = false;
            identified = false;
            continue;
        }
        if line.starts_with("---- ") {
            let Some(capture) = header.captures(line) else {
                return false;
            };
            detail = failures.contains(&format!("{}\t{}", &capture[1], &capture[2]));
            if !detail {
                return false;
            }
            identified = true;
        } else if line.starts_with("Summary ") || line.starts_with("error: ") {
            detail = false;
            identified = false;
            if !(summary.is_match(line) || line == "error: test run failed") {
                return false;
            }
        } else if line == "stdout ───" || line == "stderr ───" {
            if !identified {
                return false;
            }
            detail = true;
        } else if let Some(capture) = status.captures(line) {
            if &capture[1] == "FAIL"
                && !failures.contains(&format!("{}\t{}", &capture[2], &capture[3]))
            {
                return false;
            }
            identified = &capture[1] == "FAIL";
            detail = false;
        } else if !(detail && raw.starts_with("    ")
            || cargo_progress(line)
            || starting.is_match(line)
            || run.is_match(line))
        {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn libtest_rejects_unknown_output_beside_an_identified_failure() {
        let artifact = serde_json::json!({"reason":"compiler-artifact", "profile":{"test":true}, "manifest_path":"/repo/Cargo.toml", "executable":"fixture", "target":{"name":"fixture","kind":["lib"]}});
        let text = format!(
            "{artifact}\nRunning unittests src/lib.rs (fixture)\ntest old ... FAILED\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\nerror: test failed, to rerun pass `--lib`\nerror: 1 target failed:\n    `--lib`\n"
        );
        assert!(cargo_tests(&text, Path::new("/repo")).is_some());
        for unknown in [
            "unknown failure",
            "error[E9999]: compiler failure",
            "error: failed to run custom build command",
            "process aborted",
            "{\"reason\":\"unrecognized-failure\"}",
        ] {
            assert!(cargo_tests(&format!("{text}{unknown}\n"), Path::new("/repo")).is_none());
        }
    }

    #[test]
    fn go_test_failures_do_not_account_for_unknown_or_package_only_failures() {
        let known = "{\"Action\":\"fail\",\"Package\":\"fixture\",\"Test\":\"TestOld\"}\n{\"Action\":\"fail\",\"Package\":\"fixture\"}\n";
        assert!(go_tests(known).is_some());
        for unknown in [
            "unknown failure",
            "{\"Action\":\"build-fail\",\"Package\":\"fixture\"}",
            "{\"Action\":\"fail\",\"Package\":\"new\"}",
            "{\"Action\":\"output\",\"Package\":\"fixture\",\"Output\":\"new package-level failure\"}",
            "{\"Action\":\"run\",\"Package\":\"fixture\",\"Test\":\"TestAborted\"}",
        ] {
            assert!(go_tests(&format!("{known}{unknown}\n")).is_none());
        }
    }

    #[test]
    fn junit_failures_do_not_account_for_unknown_output_or_aborts() {
        let temp = tempfile::tempdir().unwrap();
        let report = temp.path().join("report.xml");
        let mut output = std::process::Command::new("git")
            .arg("--version")
            .output()
            .unwrap();
        output.stdout.clear();
        output.stderr.clear();
        let failures = BTreeSet::from(["fixture::lib\told".to_owned()]);
        assert!(nextest_output(
            "FAIL [0.01s] fixture::lib old\nSummary [0.01s] 1 test run: 0 passed, 1 failed, 0 skipped\nerror: test run failed",
            &failures
        ));
        let modern = "Nextest run ID 00000000-0000-0000-0000-000000000000 with nextest profile: devtool\nStarting 1 test across 1 binary\nFAIL [0.01s] (1/1) fixture::lib old\nstdout ───\n    fixture failure output\nstderr ───\n    fixture panic\n────────────\nSummary [0.01s] 1 test run: 0 passed, 1 failed, 0 skipped\nFAIL [0.01s] (1/1) fixture::lib old\nerror: test run failed";
        assert!(nextest_output(modern, &failures));
        for text in [
            "FAIL [0.01s] (1/1) fixture::lib old\nunknown failure",
            "FAIL [0.01s] (1/1) fixture::lib old\nstdout ───\nunknown failure",
        ] {
            assert!(!nextest_output(text, &failures));
        }
        for unknown in [
            "Starting unknown failure",
            "PASS unknown failure",
            "Summary unknown failure",
            "FAIL [0.01s] fixture::lib old_new",
        ] {
            assert!(!nextest_output(unknown, &failures));
        }
        let command = CommandSpec::cargo(&["nextest", "run"]);
        fs::write(&report, "<testsuites><testsuite><testcase classname='fixture::lib' name='old'><failure/></testcase></testsuite></testsuites>").unwrap();
        assert!(
            signatures(
                &command,
                &output,
                Some(&report),
                temp.path(),
                "architecture"
            )
            .unwrap()
            .is_some()
        );
        output.stderr = b"unknown gate failure".to_vec();
        assert!(
            signatures(
                &command,
                &output,
                Some(&report),
                temp.path(),
                "architecture"
            )
            .unwrap()
            .is_none()
        );
        output.stderr.clear();
        fs::write(&report, "<testsuites><testsuite><testcase classname='fixture::lib' name='old'><failure/></testcase><testcase classname='fixture::lib' name='new'><error type='abort'/></testcase></testsuite></testsuites>").unwrap();
        assert!(
            signatures(
                &command,
                &output,
                Some(&report),
                temp.path(),
                "architecture"
            )
            .unwrap()
            .is_none()
        );
    }
}
