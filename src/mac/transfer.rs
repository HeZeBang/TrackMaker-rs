use std::fs;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use tracing::{debug, error, info};

use crate::audio::recorder;
use crate::mac;
use crate::mac::csma::CsmaNode;
use crate::phy::LineCodingKind;
use crate::ui::progress::{ProgressManager, templates};
use crate::utils::consts::*;

pub fn run_sender(
    shared: recorder::AppShared,
    progress_manager: ProgressManager,
    sample_rate: u32,
    line_coding: LineCodingKind,
    sender_mac: mac::types::MacAddr,
    receiver_mac: mac::types::MacAddr,
    tx_timeout: u64,
) {
    info!("=== Sender Mode (with Stop-and-Wait) ===");
    info!("Using line coding: {}", line_coding.name());

    // Read input file
    let input_path = format!("INPUT{}to{}.bin", &sender_mac, &receiver_mac);
    let file_data = match fs::read(&input_path) {
        Ok(data) => {
            info!("Read {} bytes from {}", data.len(), input_path);
            data
        }
        Err(e) => {
            error!("Failed to read {}: {}", input_path, e);
            return;
        }
    };

    info!("=== Sender Mode (with Stop-and-Wait) ===");

    let progress_manager = Arc::new(Mutex::new(progress_manager));

    let _sender_progress = progress_manager
        .lock()
        .unwrap()
        .create_bar("sender", 0u64, templates::SENDER, "sender")
        .unwrap();

    let (tx, rx) = crossbeam_channel::unbounded::<Vec<u8>>();

    let sub_progress_manager = progress_manager.clone();
    let handle = thread::spawn(move || {
        let mut node = CsmaNode::new(
            shared,
            sub_progress_manager,
            sample_rate,
            line_coding,
            sender_mac,
            receiver_mac,
        );

        node.run_sender_loop(tx_timeout, rx);
    });

    // Split data into frames and push to queue
    for chunk in file_data.chunks(MAX_FRAME_DATA_SIZE) {
        progress_manager
            .lock()
            .unwrap()
            .increasae_length("sender", 1)
            .unwrap_or_else(|err| {
                debug!("Error while updating sender: {:?}", err)
            });
        tx.send(chunk.to_vec())
            .unwrap_or_else(|e| {
                error!("Failed to send data chunk to sender thread: {}", e);
            });
    }

    drop(tx); // Close the channel

    handle.join().unwrap();
}

pub fn run_receiver(
    shared: recorder::AppShared,
    progress_manager: ProgressManager,
    max_recording_duration_samples: u32,
    line_coding: LineCodingKind,
    receiver_addr: mac::types::MacAddr,
    sender_addr: mac::types::MacAddr,
    rx_duration: u64,
) {
    info!("=== Receiver Mode ===");
    info!("Using line coding: {}", line_coding.name());

    let (tx, rx) = crossbeam_channel::unbounded::<Vec<u8>>();

    let progress_manager = Arc::new(Mutex::new(progress_manager));

    let _progress_bar = progress_manager
        .lock()
        .unwrap()
        .create_bar(
            "recording",
            max_recording_duration_samples as u64,
            templates::RECEIVER,
            "receiver",
        )
        .unwrap();

    let sub_progress_manager = progress_manager.clone();
    let handle = thread::spawn(move || {
        let mut node = CsmaNode::new(
            shared,
            sub_progress_manager,
            SAMPLE_RATE,
            line_coding,
            receiver_addr,
            sender_addr,
        );

        node.run_receiver_loop(max_recording_duration_samples, rx_duration, tx);
    });

    let mut all_data = Vec::new();
    while let Ok(data) = rx.recv() {
        all_data.push(data);
    }

    handle.join().unwrap();

    let output_data: Vec<u8> = all_data
        .into_iter()
        .flatten()
        .collect();

    let output_path = format!("OUTPUT{}to{}.bin", &sender_addr, &receiver_addr);
    match fs::write(&output_path, &output_data) {
        Ok(_) => debug!("Written to {}", &output_path),
        Err(e) => error!("Failed to write {}: {}", output_path, e),
    }
}
