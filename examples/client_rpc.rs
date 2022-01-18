use elbus::ipc::{Client, Config};
use elbus::rpc::{DummyHandlers, Rpc, RpcClient};
/// Demo of client RPC with no handler, which just calls methods
///
/// use client_rpc_handler example to test client/server
use elbus::{empty_payload, QoS};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
struct Amount {
    value: u64,
}

#[tokio::main]
async fn main() {
    let name = "test.client.123";
    let target = "test.client.rpc";
    // create a new client instance
    let config = Config::new("/tmp/elbus.sock", name);
    let client = Client::connect(&config).await.unwrap();
    // create RPC with no handlers
    let mut rpc = RpcClient::new(client, DummyHandlers {});
    // call the method with no confirm
    rpc.call0(target, "test", empty_payload!(), QoS::Processed)
        .await
        .unwrap();
    let mut payload: HashMap<&str, u32> = HashMap::new();
    payload.insert("value", 10);
    // call a method with confirm to make sure the value is added
    rpc.call(
        target,
        "add",
        rmp_serde::to_vec_named(&payload).unwrap().into(),
        QoS::Processed,
    )
    .await
    .unwrap();
    // call the method to read the sum
    let result = rpc
        .call(target, "get", empty_payload!(), QoS::Processed)
        .await
        .unwrap();
    let amount: Amount = rmp_serde::from_read_ref(result.payload()).unwrap();
    println!("{}", amount.value);
}
