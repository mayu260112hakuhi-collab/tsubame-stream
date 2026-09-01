use stream_core::{bounded_worker_channel, WorkerCommand};
#[test]
fn bounded_queue_never_blocks_on_overflow() {
    let (tx, _rx) = bounded_worker_channel(1);
    tx.try_send(WorkerCommand::Start).unwrap();
    assert!(tx.try_send(WorkerCommand::Stop).is_err());
}
