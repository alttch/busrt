/// Client demo (listener)
use elbus::client::AsyncClient;
use elbus::ipc::{Client, Config};
use elbus::QoS;

#[tokio::main]
async fn main() {
    let name = "test.client.sender";
    // create a new client instance
    let config = Config::new("/tmp/elbus.sock", name);
    let mut client = Client::connect(&config).await.unwrap();
    // subscribe to all topics
    let opc = client
        .publish("some/topic", "hello".as_bytes().into(), QoS::Processed)
        .await
        .unwrap()
        .unwrap();
    opc.await.unwrap().unwrap();
    let opc = client
        .send(
            "test.client.listener",
            "hello".as_bytes().into(),
            QoS::Processed,
        )
        .await
        .unwrap()
        .unwrap();
    opc.await.unwrap().unwrap();
    let opc = client
        .send_broadcast("test.*", "hello everyone".as_bytes().into(), QoS::Processed)
        .await
        .unwrap()
        .unwrap();
    opc.await.unwrap().unwrap();
}
