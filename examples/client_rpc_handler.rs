/// Demo of client RPC handler
///
/// use client_rpc example to test client/server, don't forget to launch a standalone broker server
/// instance
use async_trait::async_trait;
use elbus::client::AsyncClient;
use elbus::ipc::{Client, Config};
use elbus::rpc::{Rpc, RpcClient, RpcError, RpcEvent, RpcHandlers, RpcResult};
use elbus::{Frame, QoS};
use serde::Deserialize;
use std::collections::HashMap;
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
                let mut payload = HashMap::new();
                payload.insert("ok", true);
                Ok(Some(rmp_serde::to_vec_named(&payload)?))
            }
            "get" => {
                let mut payload = HashMap::new();
                payload.insert("value", self.counter.load(atomic::Ordering::SeqCst));
                Ok(Some(rmp_serde::to_vec_named(&payload)?))
            }
            "add" => {
                let params: AddParams = rmp_serde::from_read_ref(event.payload())?;
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
async fn main() {
    let name = "test.client.rpc";
    // create a new client instance
    let config = Config::new("/tmp/elbus.sock", name);
    let mut client = Client::connect(&config).await.unwrap();
    // subscribe the cclient to all topics to print publish frames when received
    let opc = client
        .subscribe("#", QoS::Processed)
        .await
        .unwrap()
        .unwrap();
    // receive operation confirmation
    opc.await.unwrap().unwrap();
    // create handlers object
    let handlers = MyHandlers {
        counter: atomic::AtomicU64::default(),
    };
    // create RPC
    let rpc = RpcClient::new(client, handlers);
    println!("Waiting for frames to {}", name);
    while rpc.is_connected() {
        sleep(Duration::from_secs(1)).await;
        // if the broker is unavailable, ping sets the client and RPC to disconnected state
        // after, the program can try reconnecting or quit
        let _r = rpc.client().lock().await.ping().await;
    }
}
