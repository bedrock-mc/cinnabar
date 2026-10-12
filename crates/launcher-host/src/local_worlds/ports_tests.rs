use super::*;

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
