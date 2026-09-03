#![expect(
    clippy::expect_used,
    reason = "Each fallible step must fail the real mDNS contract test"
)]

use alloc::collections::BTreeMap;
use core::net::Ipv4Addr;
use core::time::Duration;

use if_addrs as _;
use mdns_sd as _;
use quickshare_network::{Advertisement, DnsSd, lan::Listener};

const SERVICE_TYPE: &str = "_quickshare-test._tcp.local.";

#[test]
fn advertised_service_is_resolved_through_mdns() {
    let dns_sd = DnsSd::new().expect("mDNS daemon should start");
    let listener = Listener::bind_any().expect("listener should bind");
    let advertisement = Advertisement {
        addresses: vec![Ipv4Addr::LOCALHOST],
        hostname: String::from("quickshare-test.local."),
        instance: String::from("rust-peer"),
        port: listener.port(),
        properties: BTreeMap::from([(String::from("n"), String::from("peer"))]),
        service_type: String::from(SERVICE_TYPE),
    };
    let published = listener
        .publish(&dns_sd, &advertisement)
        .expect("listener should advertise");
    let browser = dns_sd.browse(SERVICE_TYPE).expect("browse should start");

    let resolved = browser
        .resolve(Duration::from_secs(3))
        .expect("browse should remain healthy")
        .expect("advertised service should resolve");

    assert_eq!(resolved.instance(), "rust-peer");
    assert_eq!(resolved.port(), published.port());
    assert_eq!(resolved.property("n"), Some("peer"));
    published.stop().expect("service should unregister");
    browser.stop().expect("browse should stop");
    dns_sd.shutdown().expect("mDNS daemon should stop");
}
