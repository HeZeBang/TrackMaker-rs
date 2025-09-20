//! Records audio to a buffer and then plays it back using the default input/output devices.
//!
//! The input data is recorded to a buffer array and then played back through the output device.

use clap::Parser;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

#[derive(Parser, Debug)]
#[command(version, about = "CPAL record_wav example", long_about = None)]
struct Opt {
    /// The audio device to use
    #[arg(short, long, default_value_t = String::from("default"))]
    device: String,

    /// Use the JACK host
    #[cfg(all(
        any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd"
        ),
        feature = "jack"
    ))]
    #[arg(short, long)]
    #[allow(dead_code)]
    jack: bool,
}

fn main() -> Result<(), anyhow::Error> {
    let opt = Opt::parse();

    // Conditionally compile with jack if the feature is specified.
    #[cfg(all(
        any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd"
        ),
        feature = "jack"
    ))]
    // Manually check for flags. Can be passed through cargo with -- e.g.
    // cargo run --release --example beep --features jack -- --jack
    let host = if opt.jack {
        cpal::host_from_id(cpal::available_hosts()
            .into_iter()
            .find(|id| *id == cpal::HostId::Jack)
            .expect(
                "make sure --features jack is specified. only works on OSes where jack is available",
            )).expect("jack host unavailable")
    } else {
        cpal::default_host()
    };

    #[cfg(any(
        not(any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd"
        )),
        not(feature = "jack")
    ))]
    let host = cpal::default_host();

    // Set up the input device and stream with the default input config.
    let input_device = if opt.device == "default" {
        host.default_input_device()
    } else {
        host.input_devices()?
            .find(|x| {
                x.name()
                    .map(|y| y == opt.device)
                    .unwrap_or(false)
            })
    }
    .expect("failed to find input device");

    println!("Input device: {}", input_device.name()?);

    let input_config = input_device
        .default_input_config()
        .expect("Failed to get default input config");
    println!("Default input config: {input_config:?}");

    // Set up the output device for playback
    let output_device = host
        .default_output_device()
        .expect("failed to find output device");

    println!("Output device: {}", output_device.name()?);

    let default_output_config = output_device
        .default_output_config()
        .expect("Failed to get default output config");
    println!("Default output config: {default_output_config:?}");

    let output_config = cpal::StreamConfig {
        channels: default_output_config.channels(),
        sample_rate: cpal::SampleRate(48000),
        buffer_size: cpal::BufferSize::Default,
    };
    println!("Output config: {output_config:?}");

    // Create a buffer to store recorded audio data
    let audio_buffer: Arc<Mutex<VecDeque<f32>>> =
        Arc::new(Mutex::new(VecDeque::new()));

    // Recording phase
    println!("Begin recording...");

    let buffer_clone = audio_buffer.clone();
    let err_fn = move |err| {
        eprintln!("an error occurred on stream: {err}");
    };

    let input_stream = input_device.build_input_stream(
        &input_config.into(),
        move |data, _: &_| record_to_buffer_f32(data, &buffer_clone),
        err_fn,
        None,
    )?;

    input_stream.play()?;

    // Let recording go for roughly three seconds.
    std::thread::sleep(std::time::Duration::from_secs(3));
    drop(input_stream);

    let buffer_size = audio_buffer
        .lock()
        .unwrap()
        .len();
    println!("Recording complete! Captured {} samples.", buffer_size);

    // Playback phase
    println!("Begin playback...");

    let buffer_clone = audio_buffer.clone();
    let playback_pos_mutex = Arc::new(Mutex::new(0));
    let playback_pos_clone = playback_pos_mutex.clone();

    let err_fn = move |err| {
        eprintln!("an error occurred on playback stream: {err}");
    };

    let output_stream = output_device.build_output_stream(
        &output_config.into(),
        move |data, _: &_| {
            play_from_buffer_f32(data, &buffer_clone, &playback_pos_clone)
        },
        err_fn,
        None,
    )?;

    output_stream.play()?;

    // Wait for playback to complete (roughly the same duration as recording)
    std::thread::sleep(std::time::Duration::from_secs(3));
    drop(output_stream);

    println!("Playback complete!");
    Ok(())
}

type AudioBuffer = Arc<Mutex<VecDeque<f32>>>;

fn record_to_buffer_f32(input: &[f32], buffer: &AudioBuffer) {
    if let Ok(mut guard) = buffer.try_lock() {
        for &sample in input.iter() {
            guard.push_back(sample);
        }
    }
}

fn play_from_buffer_f32(
    output: &mut [f32],
    buffer: &AudioBuffer,
    playback_pos: &Arc<Mutex<usize>>,
) {
    if let (Ok(buffer_guard), Ok(mut pos_guard)) =
        (buffer.try_lock(), playback_pos.try_lock())
    {
        let mut pos = *pos_guard;
        let buffer_vec: Vec<f32> = buffer_guard
            .iter()
            .cloned()
            .collect();

        for sample in output.iter_mut() {
            if pos < buffer_vec.len() {
                *sample = buffer_vec[pos];
                pos += 1;
            } else {
                *sample = 0.0;
            }
        }

        *pos_guard = pos;
    }
}