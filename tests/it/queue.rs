use crate::helper::{assert_error_stack_snapshot, set_temp_data_root};
use broker::queue::{self, Queue, Receiver, Sender};

#[tokio::test]
async fn racing_senders_err() {
    let _root = set_temp_data_root();

    let _first = Sender::open(Queue::Echo).await.expect("must open first");
    let err = Sender::open(Queue::Echo)
        .await
        .expect_err("must fail to open second");
    assert_error_stack_snapshot!(&"sender", err);
}

#[tokio::test]
async fn racing_receivers_err() {
    let _root = set_temp_data_root();

    let _first = Receiver::open(Queue::Echo).await.expect("must open first");
    let err = Receiver::open(Queue::Echo)
        .await
        .expect_err("must fail to open second");
    assert_error_stack_snapshot!(&"receiver", err);
}

#[tokio::test]
async fn echo() {
    let _root = set_temp_data_root();

    let (mut tx, mut rx) = queue::open(Queue::Echo).await.expect("must open queue");

    // Send the messages
    for i in 0..3 {
        let msg = format!("msg {i}");
        tx.send(msg.as_bytes()).await.expect("must send");
        println!("tx {i}: '{msg}'");
    }
    println!("tx: done");

    // Receive the messages
    let mut messages = Vec::new();
    for i in 0..3 {
        let rx = rx.recv().await.expect("must receive");
        println!("rx {i}: '{}'", String::from_utf8_lossy(rx.data()));

        let msg = String::from_utf8(rx.data().to_vec()).expect("must have parsed message");
        messages.push(msg);
        rx.commit().expect("must commit");
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
