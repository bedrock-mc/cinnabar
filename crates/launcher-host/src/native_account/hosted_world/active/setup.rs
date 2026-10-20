//! The completed pre-admission failure boundary; admitted peers and publication errors stay separate.

use super::*;

/// Isolates only a completed pre-admission failure after proving the original host healthy.
/// A failed/canceled health check never retries setup or authorizes an uncertain publication.
pub(in crate::native_account::hosted_world) async fn recover(
    world: &mut NativeHostedWorld,
    process: &HostedBackend,
    error: io::Error,
) -> io::Result<()> {
    let deadline = operation_deadline();
    let healthy = async {
        loop {
            world.check_backend(process, deadline).await?;
            let opening = match world.directory_ref()?.opening() {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                result => result?,
            };
            world.publish_approvals(deadline).await?;
            let current = match world.directory_ref()?.opening() {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                result => result?,
            };
            if !opening.same(&current) {
                return Err(unavailable());
            }
            if Instant::now() >= deadline {
                return Err(expired());
            }
            return Ok(());
        }
    }
    .await;
    // Failure keeps the caller's phase sealed and preserves its first original setup cause.
    healthy.map_err(|_| error)
}
