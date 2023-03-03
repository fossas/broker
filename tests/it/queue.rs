use crate::helper::assert_error_stack_snapshot;
use broker::queue::{Queue, Receiver, Sender};

#[tokio::test]
async fn racing_senders_err() {
    let _first = Sender::open(Queue::Echo).await.expect("must open first");
    let err = Sender::open(Queue::Echo)
        .await
        .expect_err("must fail to open second");
    assert_error_stack_snapshot!(&"sender", err);
}

#[tokio::test]
async fn racing_receivers_err() {
    let _first = Receiver::open(Queue::Echo).await.expect("must open first");
    let err = Receiver::open(Queue::Echo)
        .await
        .expect_err("must fail to open second");
    assert_error_stack_snapshot!(&"receiver", err);
}
