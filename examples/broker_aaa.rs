/// Demo of a broker with AAA
///
/// The broker listens on 0.0.0.0:7777
///
/// Accepted client names: test (from localhost only), test2 (from any)
///
/// test is allowed to do anything
///
/// test2 is allowed to send direct messages to "test" only and subscribe to any topic
///
/// The broker force-disconnects the client named "test2" every 5 seconds
use elbus::broker::{AaaMap, Broker, ClientAaa, ServerConfig};
use ipnetwork::IpNetwork;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    // create a new broker instance
    let mut broker = Broker::new();
    broker.init_default_core_rpc().await.unwrap();
    // spawn unix server for external clients
    let aaa_map = AaaMap::default();
    aaa_map.lock().unwrap().insert(
        "test".to_owned(),
        ClientAaa::new().hosts_allow(vec![IpNetwork::V4("127.0.0.0/8".parse().unwrap())]),
    );
    aaa_map.lock().unwrap().insert(
        "test2".to_owned(),
        ClientAaa::new()
            .deny_publish()
            .deny_broadcast()
            .allow_p2p_to(vec!["test"]),
    );
    let config = ServerConfig::new().aaa_map(aaa_map);
    broker
        .spawn_tcp_server("0.0.0.0:7777", config)
        .await
        .unwrap();
    loop {
        sleep(Duration::from_secs(5)).await;
        println!("forcing test2 disconnect");
        if let Err(e) = broker.force_disconnect("test2") {
            eprintln!("{}", e);
        }
    }
}
