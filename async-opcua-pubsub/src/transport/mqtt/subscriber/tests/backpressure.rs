use super::*;

#[tokio::test]
async fn forwarding_full_queue_returns_rejected_payload_and_preserves_queued_payload() {
    // Given
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    let queued_payload = vec![0x01];
    let rejected_payload = vec![0x11];
    sender
        .send(queued_payload.clone())
        .await
        .expect("receiver remains open");
    let forwarder = PayloadForwarder::new(sender);

    // When
    let result = forwarder.forward(rejected_payload.clone());

    // Then
    assert_eq!(
        result,
        Err(tokio::sync::mpsc::error::TrySendError::Full(
            rejected_payload,
        ))
    );
    assert_eq!(receiver.recv().await, Some(queued_payload));
}
