use super::*;

#[test]
fn automatic_allocation_moves_past_a_reusable_but_udp_blocked_candidate() {
    let players = 3;
    let blocked = free_bds_port(players, 0).expect("first window");
    let first_tcp = TcpListener::bind((LOCAL_SERVER_HOST, blocked)).expect("hold first TCP");
    let _blocked_udp: Vec<_> = (0..players)
        .map(|offset| {
            let port = u16::try_from(u32::from(blocked) + offset).expect("port");
            UdpSocket::bind((LOCAL_SERVER_HOST, port)).expect("occupy first UDP window")
        })
        .collect();
    let available = free_bds_port(players, 0).expect("second window");
    drop(first_tcp);

    let selected = automatic_bds_port(players, || {
        // Model an allocator that reuses the first port whenever its listener is dropped.
        TcpListener::bind((LOCAL_SERVER_HOST, blocked))
            .or_else(|_| TcpListener::bind((LOCAL_SERVER_HOST, available)))
    })
    .expect("the second candidate has an available TCP and UDP window");
    assert_eq!(selected, available);
    TcpListener::bind((LOCAL_SERVER_HOST, blocked)).expect("rejected TCP candidate released");
    TcpListener::bind((LOCAL_SERVER_HOST, available)).expect("selected TCP candidate released");
}

#[test]
fn exhausted_automatic_allocation_releases_rejected_signaling_ports() {
    let mut occupied_udp = Vec::new();
    let mut candidates = Vec::new();
    let error = automatic_bds_port(1, || {
        let tcp = TcpListener::bind((LOCAL_SERVER_HOST, 0))?;
        let port = tcp.local_addr()?.port();
        occupied_udp.push(UdpSocket::bind((LOCAL_SERVER_HOST, port))?);
        candidates.push(port);
        Ok(tcp)
    })
    .expect_err("every candidate has an occupied UDP port");
    assert!(error.to_string().contains("UDP transport port"), "{error}");
    assert_eq!(candidates.len(), AUTOMATIC_ATTEMPTS);
    for port in candidates {
        TcpListener::bind((LOCAL_SERVER_HOST, port)).expect("rejected TCP candidate released");
    }
}

#[test]
fn automatic_windows_reserve_both_protocols_and_release_every_port() {
    let players = 3;
    let port = free_bds_port(players, 0).expect("window");
    let _tcp = TcpListener::bind((LOCAL_SERVER_HOST, port)).expect("TCP released");
    let _udp: Vec<UdpSocket> = (0..players)
        .map(|offset| {
            let port = u16::try_from(u32::from(port) + offset).expect("port");
            UdpSocket::bind((LOCAL_SERVER_HOST, port)).expect("UDP released")
        })
        .collect();
    for invalid in [0, 65_535] {
        assert!(
            free_bds_port(invalid, 0).is_err(),
            "{invalid} players must not get an empty or wrapping window"
        );
    }
}

#[test]
fn a_fixed_port_reserves_its_exact_window_or_fails() {
    let players = 3;
    let port = free_bds_port(players, 0).expect("window");
    assert_eq!(free_bds_port(players, port).expect("fixed window"), port);
    {
        let _taken = TcpListener::bind((LOCAL_SERVER_HOST, port)).expect("take TCP");
        assert!(
            free_bds_port(players, port).is_err(),
            "an occupied fixed TCP port must fail instead of moving"
        );
    }
    {
        let last = u16::try_from(u32::from(port) + players - 1).expect("port");
        let _taken = UdpSocket::bind((LOCAL_SERVER_HOST, last)).expect("take UDP");
        let error = free_bds_port(players, port).expect_err("occupied UDP window");
        assert!(error.to_string().contains("UDP transport"), "{error}");
        let _tcp = TcpListener::bind((LOCAL_SERVER_HOST, port))
            .expect("a failed UDP reservation leaked its TCP port");
        let _udp = UdpSocket::bind((LOCAL_SERVER_HOST, port))
            .expect("a failed UDP reservation leaked an earlier UDP port");
    }
    assert!(
        free_bds_port(players, 65_535).is_err(),
        "a wrapping fixed window must fail"
    );
}

#[test]
fn lan_discovery_ports_must_be_free_and_outside_the_window() {
    for invalid in [0, 19_132, 19_132 + 7] {
        assert!(check_bds_lan_port(19_132, 8, invalid).is_err(), "{invalid}");
    }
    let taken = UdpSocket::bind((LOCAL_SERVER_HOST, 0)).expect("take a port");
    let port = taken.local_addr().expect("address").port();
    assert!(
        check_bds_lan_port(19_132, 8, port).is_err(),
        "an occupied discovery port must fail before launch"
    );
    drop(taken);
    check_bds_lan_port(19_132, 8, port).expect("released discovery port");
    assert_eq!(lan_address(0), format!("127.0.0.1:{LAN_DISCOVERY_PORT}"));
    assert_eq!(
        lan_address(LAN_DISCOVERY_PORT + 1),
        format!("127.0.0.1:{}", LAN_DISCOVERY_PORT + 1)
    );
}

#[test]
fn udp_windows_name_every_port() {
    assert_eq!(udp_port_range(5000, 1), "5000");
    assert_eq!(udp_port_range(5000, 8), "5000-5007");
    assert_eq!(
        bds_udp_mapping(5000, 32000, 8),
        "127.0.0.1:5000-5007:32000-32007"
    );
}
