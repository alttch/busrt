use async_trait::async_trait;
/// Demo of custom broker internal RPC
/// Do not build with "broker-api" feature
use elbus::broker::{Broker, BROKER_NAME};
use elbus::client::AsyncClient;
use elbus::rpc::{Rpc, RpcClient, RpcError, RpcEvent, RpcHandlers, RpcResult};
use elbus::{Frame, QoS};
use serde::Deserialize;
use std::time::Duration;
use tokio::time::sleep;

struct MyHandlers {}

#[derive(Deserialize)]
struct PingParams<'a> {
    message: Option<&'a str>,
}

#[async_trait]
impl RpcHandlers for MyHandlers {
    // RPC call handler. Will react to the "test" (any params) and "ping" (will parse params as
    // msgpack and return the "message" field back) methods
    async fn handle_call(&self, event: RpcEvent) -> RpcResult {
        match event.parse_method()? {
            "test" => Ok(Some("passed".as_bytes().to_vec())),
            "ping" => {
                let params: PingParams = rmp_serde::from_read_ref(event.payload())?;
                Ok(params.message.map(|m| m.as_bytes().to_vec()))
            }
            _ => Err(RpcError::method()),
        }
    }
    // Handle RPC notifications
    async fn handle_notification(&self, event: RpcEvent) {
        println!(
            "Got RPC notification from {}: {}",
            event.sender(),
            std::str::from_utf8(event.payload()).unwrap_or("something unreadable")
        );
    }
    // handle broadcast notifications and topic publications
    async fn handle_frame(&self, frame: Frame) {
        println!(
            "Got non-RPC frame from {}: {:?} {:?} {}",
            frame.sender(),
            frame.kind(),
            frame.topic(),
            std::str::from_utf8(frame.payload()).unwrap_or("something unreadable")
        );
    }
}

#[tokio::main]
async fn main() {
    // create a new broker instance
    let mut broker = Broker::new();
    // spawn unix server for external clients
    broker
        .spawn_unix_server("/tmp/elbus.sock", 8192, Duration::from_secs(5))
        .await
        .unwrap();
    // register the broker core client
    let mut core_client = broker.register_client(BROKER_NAME).unwrap();
    // subscribe the core client to all topics to print publish frames when received
    core_client.subscribe("#", QoS::No).await.unwrap();
    // create handlers object
    let handlers = MyHandlers {};
    // create RPC
    let core_rpc = RpcClient::new(core_client, handlers);
    println!("Waiting for frames to {}", BROKER_NAME);
    // this is the internal client, it will be connected forever
    while core_rpc.is_connected() {
        sleep(Duration::from_secs(1)).await;
    }
}
