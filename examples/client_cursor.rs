// Demo of client cursor RPC
//
// use server_cursor example to test client/server
use busrt::ipc::{Client, Config};
use busrt::rpc::{Rpc, RpcClient};
use busrt::{cursors, empty_payload, QoS};
use serde::Deserialize;

#[derive(Deserialize)]
struct Customer {
    id: i64,
    name: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let name = "test.client.123";
    let target = "db";
    // create a new client instance
    let config = Config::new("/tmp/busrt.sock", name);
    let client = Client::connect(&config).await?;
    // create RPC with no handlers
    let rpc = RpcClient::new0(client);
    // get a cursor
    let cursor: cursors::Payload = rmp_serde::from_slice(
        rpc.call(target, "Ccustomers", empty_payload!(), QoS::Processed)
            .await?
            .payload(),
    )?;
    // let us use a cow to avoid unnecessary data serialization every time when the method is
    // called
    let packed_cursor = rmp_serde::to_vec_named(&cursor)?;
    let b_cursor = busrt::borrow::Cow::Borrowed(&packed_cursor);
    loop {
        // get customers one-by-one
        let result = rpc
            .call(target, "N", b_cursor.clone(), QoS::Processed)
            .await?;
        let data = result.payload();
        // the payload is empty when there are no more records left
        if data.is_empty() {
            break;
        }
        let customer: Customer = rmp_serde::from_slice(data)?;
        println!("{}: {}", customer.id, customer.name);
    }
    // do the same in bulk
    let bulk_size = 100;
    // get a cursor
    let mut cursor: cursors::Payload = rmp_serde::from_slice(
        rpc.call(target, "Ccustomers", empty_payload!(), QoS::Processed)
            .await?
            .payload(),
    )?;
    cursor.set_bulk_number(bulk_size);
    let packed_cursor = rmp_serde::to_vec_named(&cursor)?;
    let b_cursor = busrt::borrow::Cow::Borrowed(&packed_cursor);
    loop {
        // get customers in bulk
        let result = rpc
            .call(target, "NB", b_cursor.clone(), QoS::Processed)
            .await?;
        let customers: Vec<Customer> = rmp_serde::from_slice(result.payload())?;
        for customer in &customers {
            println!("{}: {}", customer.id, customer.name);
        }
        // stop if the block contains less records than the bulk size - that means it is the last
        // block
        if customers.len() < bulk_size {
            break;
        }
    }
    Ok(())
}
