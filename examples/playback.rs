use jack;

fn main() {
    println!("Hello, world!");

    // Setting up a JACK client
    let (client, _status) =
        jack::Client::new("track_maker", jack::ClientOptions::NO_START_SERVER)
            .unwrap();

    // Display JACK server information
    let sample_rate = client.sample_rate();
    let buffer_size = client.buffer_size();
    println!("JACK Server Info:");
    println!("  Sample Rate: {} Hz", sample_rate);
    println!("  Buffer Size: {} samples", buffer_size);
    println!(
        "  Buffer Duration: {:.2} ms",
        (buffer_size as f64 / sample_rate as f64) * 1000.0
    );

    // Mono audio ports
    let in_port = client
        .register_port("tm_in", jack::AudioIn::default())
        .unwrap();
    let mut out_port = client
        .register_port("tm_out", jack::AudioOut::default())
        .unwrap();

    // Get port names before moving them into the closure
    let in_port_name = in_port.name().unwrap();
    let out_port_name = out_port.name().unwrap();

    // Process callback
    let process_cb =
        move |_: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
            let in_buffer = in_port.as_slice(ps);
            let out_buffer = out_port.as_mut_slice(ps);

            // Simple pass-through (copy input to output)
            out_buffer.copy_from_slice(in_buffer);

            jack::Control::Continue
        };
    let process = jack::contrib::ClosureProcessHandler::new(process_cb);

    // Activate
    let active_client = client
        .activate_async((), process)
        .unwrap();

    // Connect to system ports
    // Get system input ports (for connecting our output to)
    let system_input_ports = active_client
        .as_client()
        .ports(
            None,
            None,
            jack::PortFlags::IS_INPUT | jack::PortFlags::IS_PHYSICAL,
        );

    // Get system output ports (for connecting to our input)
    let system_output_ports = active_client
        .as_client()
        .ports(
            None,
            None,
            jack::PortFlags::IS_OUTPUT | jack::PortFlags::IS_PHYSICAL,
        );

    println!("Found {} system input ports", system_input_ports.len());
    println!("Found {} system output ports", system_output_ports.len());

    // Connect system output to our input (so we can receive audio)
    if let Some(system_out) = system_output_ports.first() {
        match active_client
            .as_client()
            .connect_ports_by_name(system_out, &in_port_name)
        {
            Ok(_) => println!("Connected {} to {}", system_out, in_port_name),
            Err(e) => eprintln!("Failed to connect input: {}", e),
        }
    }

    // Connect our output to system input (so audio can be heard)
    if let Some(system_in) = system_input_ports.first() {
        match active_client
            .as_client()
            .connect_ports_by_name(&out_port_name, system_in)
        {
            Ok(_) => println!("Connected {} to {}", out_port_name, system_in),
            Err(e) => eprintln!("Failed to connect output: {}", e),
        }
    }

    println!("Audio processing started. Running for 10 seconds...");

    // Wait for 10 seconds
    std::thread::sleep(std::time::Duration::from_secs(10));

    println!("Exiting gracefully...");
    if let Err(err) = active_client.deactivate() {
        eprintln!("Error deactivating client: {}", err);
    }
}
