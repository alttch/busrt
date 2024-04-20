// Demo of client RPC handler
//
// use client_rpc example to test client/server, don't forget to launch a standalone broker server
// instance
use busrt::async_trait;
use busrt::client::AsyncClient;
use busrt::ipc::{Client, Config};
use busrt::rpc::{Rpc, RpcClient, RpcError, RpcEvent, RpcHandlers, RpcResult};
use busrt::{Frame, QoS};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::atomic;
use std::time::Duration;
use tokio::time::sleep;

struct MyHandlers {
    // all RPC handlers are launched in parallel multiple instances, so the internal variables need
    // to be either atomic or under Mutex/RwLock to be modified
    counter: atomic::AtomicU64,
}

#[derive(Deserialize)]
struct AddParams {
    value: u64,
}

#[async_trait]
impl RpcHandlers for MyHandlers {
    // RPC call handler. Will react to the "test" and "get" (any params) and "add" (will parse
    // params as msgpack and add the value to the internal counter) methods
    async fn handle_call(&self, event: RpcEvent) -> RpcResult {
        match event.parse_method()? {
            "test" => {
                let mut payload = BTreeMap::new();
                payload.insert("ok", true);
                Ok(Some(rmp_serde::to_vec_named(&payload)?))
            }
            "get" => {
                let mut payload = BTreeMap::new();
                payload.insert("value", self.counter.load(atomic::Ordering::SeqCst));
                Ok(Some(rmp_serde::to_vec_named(&payload)?))
            }
            "add" => {
                let params: AddParams = rmp_serde::from_slice(event.payload())?;
                self.counter
                    .fetch_add(params.value, atomic::Ordering::SeqCst);
                Ok(None)
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
    let name = "test.client.rpc";
    // create a new client instance
    let config = Config::new("/tmp/busrt.sock", name);
    let mut client = Client::connect(&config).await?;
    // subscribe the cclient to all topics to print publish frames when received
    let op_confirm = client.subscribe("#", QoS::Processed).await?.expect("no op");
    // receive operation confirmation
    op_confirm.await??;
    // create handlers object
    let handlers = MyHandlers {
        counter: atomic::AtomicU64::default(),
    };
    // create RPC
    let rpc = RpcClient::new(client, handlers);
    println!("Waiting for frames to {}", name);
    while rpc.is_connected() {
        sleep(Duration::from_secs(1)).await;
    }
    Ok(())
}
