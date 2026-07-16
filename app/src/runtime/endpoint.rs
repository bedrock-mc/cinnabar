use crate::*;

pub(crate) fn resolve_socket_dir(path: &Path) -> PathBuf {
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let executable = std::env::current_exe().unwrap_or_default();
    resolve_socket_dir_from(path, &current_dir, &executable)
}

pub(crate) fn resolve_socket_dir_from(
    path: &Path,
    current_dir: &Path,
    executable: &Path,
) -> PathBuf {
    if path.is_absolute() {
        return path.to_owned();
    }
    let current_candidate = current_dir.join(path);
    if bridge_endpoint_exists(&current_candidate) {
        return current_candidate;
    }
    let development_candidate = executable
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(|project_root| project_root.join(path));
    if let Some(candidate) = development_candidate
        && bridge_endpoint_exists(&candidate)
    {
        return candidate;
    }
    current_candidate
}

pub(crate) fn bridge_endpoint_exists(directory: &Path) -> bool {
    let endpoint = bridge_endpoint_path(directory);
    if cfg!(windows) {
        endpoint.is_file()
    } else {
        endpoint.exists()
    }
}

pub(crate) fn bridge_endpoint_path(directory: &Path) -> PathBuf {
    directory.join(if cfg!(windows) {
        "game.addr"
    } else {
        "game.sock"
    })
}

pub(crate) fn preflight_bridge_endpoint(socket_dir: &Path) -> Result<()> {
    if bridge_endpoint_exists(socket_dir) {
        return Ok(());
    }
    let endpoint = bridge_endpoint_path(socket_dir);
    bail!(
        "Go core is not running: expected bridge endpoint at {}. Start it first with `make core UPSTREAM=host:port` (socket directory: {}).",
        endpoint.display(),
        socket_dir.display(),
    )
}
