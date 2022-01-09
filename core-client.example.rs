let (core_client, rx) = broker.register_client("broker").await.unwrap();
core_client.subscribe("#").await.unwrap();
tokio::spawn(async move {
    loop {
        dbg!(
            core_client
                .send("test.123", "123".as_bytes().to_vec())
                .await
        );
        dbg!(
            core_client
                .send_broadcast("test.*", "bc".as_bytes().to_vec())
                .await
        );
        dbg!(
            core_client
                .publish("test/topic", "content".as_bytes().to_vec())
                .await
        );
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
});
tokio::spawn(async move {
    while let Ok(frame) = rx.recv().await {
        dbg!(frame.tp(), frame.sender(), frame.topic(), frame.payload());
    }
    dbg!("channel closed");
});
