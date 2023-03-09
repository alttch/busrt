// Client demo (listener)
use busrt::client::AsyncClient;
use busrt::ipc::{Client, Config};
use busrt::QoS;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let name = "test.client.sender";
    // create a new client instance
    let config = Config::new("/tmp/busrt.sock", name);
    let mut client = Client::connect(&config).await?;
    // publish to a topic
    let opc = client
        .publish("some/topic", "hello".as_bytes().into(), QoS::Processed)
        .await?
        .expect("no op");
    opc.await??;
    // send a direct message
    let opc = client
        .send(
            "test.client.listener",
            "hello".as_bytes().into(),
            QoS::Processed,
        )
        .await?
        .expect("no op");
    opc.await??;
    // send a broadcast message
    let opc = client
        .send_broadcast("test.*", "hello everyone".as_bytes().into(), QoS::Processed)
        .await?
        .expect("no op");
    opc.await??;
    Ok(())
}
