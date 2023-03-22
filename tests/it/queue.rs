use crate::helper::{assert_error_stack_snapshot, temp_ctx};
use broker::queue::{self, Queue, Receiver, Sender};

#[tokio::test]
async fn racing_senders_err() {
    let (_tmp, ctx) = temp_ctx!();

    let _first: Sender<()> = Sender::open(&ctx, Queue::Echo)
        .await
        .expect("must open first");
    let second: Result<Sender<()>, _> = Sender::open(&ctx, Queue::Echo).await;
    assert_error_stack_snapshot!(&"sender", second.expect_err("must fail to open second"));
}

#[tokio::test]
async fn racing_receivers_err() {
    let (_tmp, ctx) = temp_ctx!();

    let _first: Receiver<()> = Receiver::open(&ctx, Queue::Echo)
        .await
        .expect("must open first");
    let second: Result<Receiver<()>, _> = Receiver::open(&ctx, Queue::Echo).await;
    assert_error_stack_snapshot!(&"receiver", second.expect_err("must fail to open second"));
}

#[tokio::test]
async fn echo() {
    let (_tmp, ctx) = temp_ctx!();

    let (mut tx, mut rx) = queue::open::<String>(&ctx, Queue::Echo)
        .await
        .expect("must open queue");

    // Send the messages
    for i in 0..3 {
        let msg = format!("msg {i}");
        tx.send(&msg).await.expect("must send");
        println!("tx {i}: '{msg}'");
    }
    println!("tx: done");

    // Receive the messages
    let mut messages = Vec::new();
    for i in 0..3 {
        let rx = rx.recv().await.expect("must receive");
        let msg = rx.item().expect("must receive message");
        println!("rx {i}: '{msg}'");

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
