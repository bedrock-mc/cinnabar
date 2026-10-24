//! Application lifetime for a native addressed join's existing trust dialog.

use launcher::install_layout::InstallLayout;
use launcher_host::server_trust::NativeSessionTrust;
use protocol::{NativeConnectionTarget, NativeTransport, SessionEndpoint};
use std::io;

/// Binds the exact endpoint to a join-local capability and the original installed trust file.
pub(super) fn bind(
    target: NativeConnectionTarget,
    layout: &InstallLayout,
) -> io::Result<(NativeConnectionTarget, NativeSessionTrust)> {
    let trust = NativeSessionTrust::new(layout.server_trust_file());
    let target = NativeConnectionTarget::addressed(target.address, trust.authorization())?;
    Ok((target, trust))
}

/// Supplies the same trust owner for direct startup before its original network worker begins.
pub(crate) fn initial(
    endpoint: Option<SessionEndpoint>,
    layout: &InstallLayout,
) -> io::Result<(Option<SessionEndpoint>, Option<super::TrustOwner>)> {
    match endpoint {
        Some(SessionEndpoint::Native(target)) if target.transport == NativeTransport::Addressed => {
            let (target, trust) = bind(target, layout)?;
            Ok((Some(SessionEndpoint::Native(target)), Some(Box::new(trust))))
        }
        endpoint => Ok((endpoint, None)),
    }
}
