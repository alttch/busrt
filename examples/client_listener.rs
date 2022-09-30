/// Client demo (listener)
use busrt::client::AsyncClient;
use busrt::ipc::{Client, Config};
use busrt::QoS;

#[tokio::main]
async fn main() {
    let name = "test.client.listener";
    // create a new client instance
    let config = Config::new("/tmp/busrt.sock", name);
    let mut client = Client::connect(&config).await.unwrap();
    // subscribe to all topics
    let opc = client
        .subscribe("#", QoS::Processed)
        .await
        .unwrap()
        .unwrap();
    opc.await.unwrap().unwrap();
    // handle incoming frames
    let rx = client.take_event_channel().unwrap();
    while let Ok(frame) = rx.recv().await {
        println!(
            "Frame from {}: {:?} {:?} {}",
            frame.sender(),
            frame.kind(),
            frame.topic(),
            std::str::from_utf8(frame.payload()).unwrap_or("something unreadable")
        );
    }
}
