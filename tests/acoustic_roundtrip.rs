use trackmaker_rs::transmission::{PskReceiver, PskSender};

#[test]
fn acoustic_link_round_trip_without_audio_device() {
    let sender = PskSender::new_default();
    let receiver = PskReceiver::new_default();

    let message = "Rust makes acoustic links fun! 你好，世界！".repeat(12);
    let tx_signal = sender.transmit_text(&message);
    assert!(
        !tx_signal.is_empty(),
        "transmitted waveform should not be empty"
    );

    let received = receiver
        .receive_text(&tx_signal)
        .expect("receiver should decode the offline waveform");

    assert_eq!(received, message);
}
