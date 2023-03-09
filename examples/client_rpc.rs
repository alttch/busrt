// Demo of client RPC with no handler which just calls methods
//
// use client_rpc_handler example to test client/server
use busrt::ipc::{Client, Config};
use busrt::rpc::{Rpc, RpcClient};
use busrt::{empty_payload, QoS};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Amount {
    value: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let name = "test.client.123";
    let target = "test.client.rpc";
    // create a new client instance
    let config = Config::new("/tmp/busrt.sock", name);
    let client = Client::connect(&config).await?;
    // create RPC with no handlers
    let rpc = RpcClient::new0(client);
    // call the method with no confirm
    rpc.call0(target, "test", empty_payload!(), QoS::Processed)
        .await?;
    let mut payload: BTreeMap<&str, u32> = <_>::default();
    payload.insert("value", 10);
    // call a method with confirm to make sure the value is added
    rpc.call(
        target,
        "add",
        rmp_serde::to_vec_named(&payload)?.into(),
        QoS::Processed,
    )
    .await?;
    // call the method to read the sum
    let result = rpc
        .call(target, "get", empty_payload!(), QoS::Processed)
        .await?;
    let amount: Amount = rmp_serde::from_slice(result.payload())?;
    println!("{}", amount.value);
    Ok(())
}
