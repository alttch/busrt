// Demo of a broker with AAA
//
// The broker listens on 0.0.0.0:7777
//
// Accepted client names: test (from localhost only), test2 (from any)
//
// test is allowed to do anything
//
// test2 is allowed to send direct messages to "test" only and publish to subtopics of "news"
//
// The broker force-disconnects the client named "test2" every 5 seconds
use busrt::broker::{AaaMap, Broker, ClientAaa, ServerConfig};
use ipnetwork::IpNetwork;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // create a new broker instance
    let mut broker = Broker::new();
    broker.init_default_core_rpc().await?;
    // create AAA map
    let aaa_map = AaaMap::default();
    {
        let mut map = aaa_map.lock();
        map.insert(
            "test".to_owned(),
            ClientAaa::new().hosts_allow(vec![IpNetwork::V4("127.0.0.0/8".parse()?)]),
        );
        map.insert(
            "test2".to_owned(),
            ClientAaa::new()
                .allow_publish_to(&["news/#"])
                .deny_subscribe()
                .deny_broadcast()
                .allow_p2p_to(&["test"]),
        );
    }
    // put AAA map to the server config
    let config = ServerConfig::new().aaa_map(aaa_map);
    // spawn tcp server for external clients
    broker.spawn_tcp_server("0.0.0.0:7777", config).await?;
    // the map can be modified later at any time, however access controls are cached for clients
    // which are already connected
    loop {
        sleep(Duration::from_secs(5)).await;
        println!("forcing test2 disconnect");
        if let Err(e) = broker.force_disconnect("test2") {
            eprintln!("{}", e);
        }
    }
}
