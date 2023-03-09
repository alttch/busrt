// Demo of custom broker internal RPC
use busrt::async_trait;
use busrt::broker::{Broker, ServerConfig, BROKER_NAME};
use busrt::client::AsyncClient;
use busrt::rpc::{Rpc, RpcClient, RpcError, RpcEvent, RpcHandlers, RpcResult};
use busrt::{Frame, QoS};
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
                let params: PingParams = rmp_serde::from_slice(event.payload())?;
                Ok(params.message.map(|m| m.as_bytes().to_vec()))
            }
            _ => Err(RpcError::method(None)),
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
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // create a new broker instance
    let mut broker = Broker::new();
    // spawn unix server for external clients
    broker
        .spawn_unix_server("/tmp/busrt.sock", ServerConfig::default())
        .await?;
    // register the broker core client
    let mut core_client = broker.register_client(BROKER_NAME).await?;
    // subscribe the core client to all topics to print publish frames when received
    core_client.subscribe("#", QoS::No).await?;
    // create handlers object
    let handlers = MyHandlers {};
    // create RPC
    let crpc = RpcClient::new(core_client, handlers);
    println!("Waiting for frames to {}", BROKER_NAME);
    // set broker client, optional, allows to spawn fifo servers, the client is wrapped in
    // Arc<Mutex<_>> as it is cloned for each fifo spawned and can be got back with core_rpc_client
    // broker method
    broker.set_core_rpc_client(crpc).await;
    // test it with echo .broker .hello > /tmp/busrt.fifo
    broker.spawn_fifo("/tmp/busrt.fifo", 8192).await?;
    // this is the internal client, it will be connected forever
    while broker
        .core_rpc_client()
        .lock()
        .await
        .as_ref()
        .unwrap()
        .is_connected()
    {
        sleep(Duration::from_secs(1)).await;
    }
    Ok(())
}
