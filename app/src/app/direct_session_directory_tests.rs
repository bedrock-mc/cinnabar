use super::*;
use crate::args::ParseOutcome;

fn run_args(arguments: &[&str]) -> args::ClientArgs {
    match args::ClientArgs::parse_from(arguments.to_vec()) {
        Ok(ParseOutcome::Run(parsed)) => *parsed,
        outcome => panic!("expected run arguments, got {outcome:?}"),
    }
}

fn temporary_root(label: &str) -> std::path::PathBuf {
    let nonce = web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .expect("system clock must be after the Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("rust-mcbe-{label}-{}-{nonce}", std::process::id()))
}

#[test]
fn explicit_non_grammar_socket_dir_starts_without_a_guard() {
    // Base behavior: documented custom socket directories are accepted
    // even though their leaf violates the session-directory grammar.
    let root = temporary_root("explicit-sock-dir");
    let custom = root.join("custom.sock");
    let parsed = run_args(&[
        "client",
        "--address",
        "127.0.0.1:19132",
        "--socket-dir",
        custom.to_str().expect("temp path is UTF-8"),
    ]);
    assert!(parsed.socket_dir_explicit);

    let holder = bind_direct_session_directory(&parsed, resolve_socket_dir(&parsed.socket_dir))
        .expect("an explicit non-grammar socket directory must start cleanly");
    assert!(
        custom.is_dir(),
        "the historical side effect of ensuring the directory exists stays"
    );
    let owned_entries = fs::read_dir(&custom)
        .expect("read prepared socket directory")
        .count();
    assert_eq!(
        owned_entries, 0,
        "unguarded operator directories never receive an ownership marker"
    );
    drop(holder);
    assert!(
        custom.is_dir(),
        "teardown leaves an unowned operator directory untouched"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn derived_default_directory_still_binds_the_guard() {
    let root = temporary_root("derived-sock-dir");
    fs::create_dir_all(&root).expect("create temp root");
    let parsed = run_args(&["client", "--address", "127.0.0.1:19132"]);
    assert!(!parsed.socket_dir_explicit);
    let socket_dir = root.join("direct-123");

    let holder = bind_direct_session_directory(&parsed, socket_dir.clone())
        .expect("app-derived directories keep exclusive ownership");
    assert!(socket_dir.is_dir(), "binding prepares the owned directory");
    drop(holder);
    assert!(
        !socket_dir.exists(),
        "default-path ownership and teardown are unchanged"
    );
    let _ = fs::remove_dir_all(&root);
}
