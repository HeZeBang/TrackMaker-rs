use jack;
use tracing::{debug, error, info, warn};

pub fn print_jack_info(client: &jack::Client) -> (usize, usize) {
    let sample_rate = client.sample_rate();
    let buffer_size = client.buffer_size();
    info!("JACK Server Info:");
    info!("  Sample Rate: {} Hz", sample_rate);
    info!("  Buffer Size: {} samples", buffer_size);
    info!(
        "  Buffer Duration: {:.2} ms",
        (buffer_size as f64 / sample_rate as f64) * 1000.0
    );
    (sample_rate as usize, buffer_size as usize)
}

pub fn connect_system_ports(client: &jack::Client, in_port_name: &str, out_port_name: &str) {
    let system_input_ports = client.ports(
        None,
        None,
        jack::PortFlags::IS_INPUT | jack::PortFlags::IS_PHYSICAL,
    );

    let system_output_ports = client.ports(
        None,
        None,
        jack::PortFlags::IS_OUTPUT | jack::PortFlags::IS_PHYSICAL,
    );

    debug!("{} physical input found.", system_input_ports.len());
    debug!("{} physical output found.", system_output_ports.len());

    if let Some(system_out) = system_output_ports.first() {
        match client.connect_ports_by_name(system_out, in_port_name) {
            Ok(_) => info!("Connected Input: {} -> {}", system_out, in_port_name),
            Err(e) => error!("Failed connecting Input {} -> {}: {}", system_out, in_port_name, e),
        }
    }

    if let Some(system_in) = system_input_ports.first() {
        match client.connect_ports_by_name(out_port_name, system_in) {
            Ok(_) => info!("Connected Output: {} -> {}", out_port_name, system_in),
            Err(e) => error!("Failed connecting Output {} -> {}: {}", out_port_name, system_in, e),
        }
    }

    if system_output_ports.is_empty() || system_input_ports.is_empty() {
        warn!("Missing input / output");
    }
}


