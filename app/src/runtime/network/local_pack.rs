//! Test-only: loads a locally cached server pack (with its `.key` beside it) named by an env var.

use std::path::Path;

use resource_pack::LayeredPackView;

/// The pack at `$var` (a `.zip`/`.mcpack`), or `None` when the variable is unset.
pub(super) fn local_pack_view(var: &str) -> Option<LayeredPackView> {
    local_pack_view_at(Path::new(&std::env::var_os(var)?))
}

/// The cached pack archive at `path`, with its `.key` beside it when present.
pub(super) fn local_pack_view_at(path: &Path) -> Option<LayeredPackView> {
    let key = std::fs::read(path.with_extension("key")).unwrap_or_default();
    let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
    let (id, version) = stem.split_once('_').expect("<uuid>_<version> file name");
    let archive = protocol::ResourcePackArchive::with_content_key(
        id.parse().unwrap(),
        version.into(),
        String::new(),
        std::fs::read(path).unwrap(),
        key,
    );
    let stack =
        resource_pack::validate_handoff(protocol::ResourcePackHandoff::from_archives(vec![
            archive,
        ]));
    for rejection in stack.rejections() {
        eprintln!("local pack rejected: {}", rejection.reason);
    }
    Some(LayeredPackView::new(stack))
}

/// Writes every readable file of the `$var` pack under `$CINNABAR_DUMP_DIR`, for inspection.
#[test]
fn dump_local_pack() {
    let (Some(view), Some(dir)) = (
        local_pack_view("CINNABAR_SERVER_PACK"),
        std::env::var_os("CINNABAR_DUMP_DIR"),
    ) else {
        eprintln!(
            "skipping dump_local_pack: fixture unavailable; offline pack export; requires CINNABAR_SERVER_PACK and CINNABAR_DUMP_DIR"
        );
        return;
    };
    for path in view.list("") {
        if let Some(bytes) = view.read(path) {
            let target = Path::new(&dir).join(path);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(target, bytes).unwrap();
        }
    }
}
