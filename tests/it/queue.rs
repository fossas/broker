use broker::queue::Queue;

#[tokio::test]
async fn echo() {
    let queue = Queue::default();

    // Send the messages
    for i in 0..3 {
        let msg = format!("msg {i}");
        queue.send(&msg).await.expect("must send");
        println!("tx {i}: '{msg}'");
    }
    println!("tx: done");

    // Receive the messages
    let mut messages = Vec::new();
    for i in 0..3 {
        let msg = queue.recv().await.expect("must receive");
        println!("rx {i}: '{msg}'");
        messages.push(msg);
    }
    println!("rx: done");

    assert_eq!(
        messages,
        vec![
            String::from("msg 0"),
            String::from("msg 1"),
            String::from("msg 2"),
        ]
    );
}
