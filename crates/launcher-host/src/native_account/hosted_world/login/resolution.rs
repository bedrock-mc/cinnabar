//! Acquires one signed UDP-address admission result without replacing the accepted transport.

use super::*;
use bedrock_net::login::verify_authenticated_request;
use dns_lookup::{AddrInfoHints, Protocol, SockType, getaddrinfo};
use std::{cell::RefCell, net::Ipv6Addr};

/// Captures a pure-parser cache miss only after the original provider/signature checks.
struct MissingAddress<'a> {
    policy: &'a NativeHostedLoginPolicy,
    address: RefCell<Option<String>>,
}

impl LoginVerifier for MissingAddress<'_> {
    fn verify(&self, token: &str, utc: SystemTime) -> Result<TrustedIdentity, LoginError> {
        self.policy.verify(token, utc)
    }

    fn resolve_client_address(&self, address: &str) -> Result<(), LoginError> {
        match self.policy.resolve_client_address(address) {
            Err(LoginError::AddressResolutionRequired) => {
                *self.address.borrow_mut() = Some(address.into());
                Err(LoginError::AddressResolutionRequired)
            }
            result => result,
        }
    }
}

impl NativeHostedLoginPolicy {
    /// Verifies the retained request locally and returns only its exact validated resolver miss.
    fn missing_address(
        &self,
        request: &[u8],
        maximum: usize,
    ) -> Result<Option<String>, LoginError> {
        let verifier = MissingAddress {
            policy: self,
            address: RefCell::new(None),
        };
        match verify_authenticated_request(request, &verifier, self.utc()?, maximum) {
            Ok(_) => Ok(None),
            Err(LoginError::AddressResolutionRequired) => verifier
                .address
                .into_inner()
                .map(Some)
                .ok_or(LoginError::InvalidClientData),
            Err(error) => Err(error),
        }
    }
}

impl NativeHostedWorld {
    /// Services the original directory while acquiring one result under the first Login cutoff.
    /// Cancellation cannot update the policy, and never claims to interrupt an executing OS call.
    pub(in crate::native_account::hosted_world) async fn preflight_login(
        &mut self,
        request: &[u8],
        policy: &mut NativeHostedLoginPolicy,
        maximum: usize,
    ) -> io::Result<()> {
        let Some(address) = policy
            .missing_address(request, maximum)
            .map_err(|_| denied())?
        else {
            return Ok(());
        };
        let admitted = self
            .peer_io(Some(policy.deadline), resolve(address.clone()))
            .await?;
        self.wait_policy(policy).await?;
        policy.utc().map_err(|_| denied())?;
        policy.resolution = Some((address, admitted));
        Ok(())
    }
}

/// Aborts a queued blocking lookup when its caller retires; running OS calls retain no authority.
struct Lookup(tokio::task::JoinHandle<bool>);

impl Drop for Lookup {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Returns only admission/refusal; all OS results and iterator storage die inside the blocking call.
async fn resolve(address: String) -> io::Result<bool> {
    let mut lookup = Lookup(tokio::task::spawn_blocking(move || lookup_udp(&address)));
    (&mut lookup.0)
        .await
        .map_err(|_| io::Error::other("hosted Login address lookup did not complete"))
}

/// Uses only the pure verifier's normalized host/service, with UDP rather than TCP service hints.
fn lookup_udp(address: &str) -> bool {
    let Some((host, port)) = address.rsplit_once(':') else {
        return false;
    };
    let host = if let Some(host) = host.strip_prefix('[') {
        let Some(host) = host.strip_suffix(']') else {
            return false;
        };
        host
    } else {
        host
    };
    // A literal's nonempty zone remains opaque during address admission.
    let host = match host.split_once('%') {
        Some((ip, zone)) if !zone.is_empty() && ip.parse::<Ipv6Addr>().is_ok() => ip,
        _ => host,
    };
    let Some(service) = service(port) else {
        return false;
    };
    let hints = AddrInfoHints {
        socktype: SockType::DGram.into(),
        protocol: Protocol::UDP.into(),
        ..Default::default()
    };
    getaddrinfo(
        (!host.is_empty()).then_some(host),
        Some(&service),
        Some(hints),
    )
    .is_ok_and(|mut addresses| addresses.any(|address| address.is_ok()))
}

/// Canonicalizes validated Go numeric spellings, otherwise lowercases a named UDP service.
fn service(port: &str) -> Option<String> {
    let (negative, digits) = match port.as_bytes().first() {
        Some(b'-') => (true, &port[1..]),
        Some(b'+') => (false, &port[1..]),
        _ => (false, port),
    };
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Some(port.to_lowercase());
    }
    let number = if digits.is_empty() {
        0
    } else {
        digits.parse::<u16>().ok()?
    };
    (!negative || number == 0).then(|| number.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Local UDP admission preserves numeric spellings and resolves services independently of zones.
    #[test]
    fn udp_os_admission_preserves_local_numeric_and_service_forms() {
        let cases = [
            ("localhost:19132", true),
            ("localhost:", true),
            ("localhost:+", true),
            ("localhost:-", true),
            ("localhost:+0", true),
            ("localhost:-0", true),
            ("localhost:00019132", true),
            ("localhost:domain", true),
            ("localhost:DoMaIn", true),
            ("localhost:cinnabar-no-such-udp-service", false),
            ("127.0.0.1:domain", true),
            ("[::1]:domain", true),
            ("[::1%nonexistent-cnbr-zone]:domain", true),
        ];
        let observed: Vec<_> = cases
            .into_iter()
            .map(|(address, expected)| (address, expected, lookup_udp(address)))
            .collect();
        assert!(
            observed
                .iter()
                .all(|(_, expected, actual)| expected == actual),
            "local UDP admission (address, expected, actual): {observed:?}"
        );
    }

    /// An empty zone uses platform host resolution, with the same result for its named UDP port.
    #[test]
    fn udp_os_empty_zone_named_service_matches_numeric_port_admission() {
        let named = lookup_udp("[::1%]:domain");
        let numeric = lookup_udp("[::1%]:53");
        assert_eq!(
            named, numeric,
            "empty-zone admission depends on the service spelling"
        );
    }
}
