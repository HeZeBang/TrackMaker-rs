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

pub fn connect_system_ports(
    client: &jack::Client,
    in_port_name: &str,
    out_port_name: &str,
) {
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
            Ok(_) => {
                info!("Connected Input: {} -> {}", system_out, in_port_name)
            }
            Err(e) => error!(
                "Failed connecting Input {} -> {}: {}",
                system_out, in_port_name, e
            ),
        }
    }

    if let Some(system_in) = system_input_ports.first() {
        match client.connect_ports_by_name(out_port_name, system_in) {
            Ok(_) => {
                info!("Connected Output: {} -> {}", out_port_name, system_in)
            }
            Err(e) => error!(
                "Failed connecting Output {} -> {}: {}",
                out_port_name, system_in, e
            ),
        }
    }

    if system_output_ports.is_empty() || system_input_ports.is_empty() {
        warn!("Missing input / output");
    }
}

pub fn list_system_input_ports(client: &jack::Client) -> Vec<String> {
    client
        .ports(
            None,
            None,
            jack::PortFlags::IS_INPUT | jack::PortFlags::IS_PHYSICAL,
        )
        .into_iter()
        .map(|s| s.to_string())
        .collect()
}

pub fn list_system_output_ports(client: &jack::Client) -> Vec<String> {
    client
        .ports(
            None,
            None,
            jack::PortFlags::IS_OUTPUT | jack::PortFlags::IS_PHYSICAL,
        )
        .into_iter()
        .map(|s| s.to_string())
        .collect()
}

pub fn connect_input_from_first_system_output(
    client: &jack::Client,
    in_port_name: &str,
) {
    let system_outputs = list_system_output_ports(client);
    debug!("{} physical output found.", system_outputs.len());
    if let Some(system_out) = system_outputs.first() {
        match client.connect_ports_by_name(system_out, in_port_name) {
            Ok(_) => {
                info!("Connected Input: {} -> {}", system_out, in_port_name)
            }
            Err(e) => error!(
                "Failed connecting Input {} -> {}: {}",
                system_out, in_port_name, e
            ),
        }
    } else {
        warn!(
            "No system physical output found to feed input {}",
            in_port_name
        );
    }
}

pub fn connect_output_to_first_system_input(
    client: &jack::Client,
    out_port_name: &str,
) {
    let system_inputs = list_system_input_ports(client);
    debug!("{} physical input found.", system_inputs.len());
    if let Some(system_in) = system_inputs.first() {
        match client.connect_ports_by_name(out_port_name, system_in) {
            Ok(_) => {
                info!("Connected Output: {} -> {}", out_port_name, system_in)
            }
            Err(e) => error!(
                "Failed connecting Output {} -> {}: {}",
                out_port_name, system_in, e
            ),
        }
    } else {
        warn!(
            "No system physical input found for output {}",
            out_port_name
        );
    }
}

pub fn disconnect_input_sources(client: &jack::Client, in_port_name: &str) {
    let system_outputs = list_system_output_ports(client);
    for sys_out in system_outputs.iter() {
        match client.disconnect_ports_by_name(sys_out, in_port_name) {
            Ok(()) => {
                info!("Disconnected Input: {} -> {}", sys_out, in_port_name)
            }
            Err(e) => {
                debug!("Skip disconnect {} -> {}: {}", sys_out, in_port_name, e)
            }
        }
    }
}

pub fn disconnect_output_sinks(client: &jack::Client, out_port_name: &str) {
    let system_inputs = list_system_input_ports(client);
    for sys_in in system_inputs.iter() {
        match client.disconnect_ports_by_name(out_port_name, sys_in) {
            Ok(()) => {
                info!("Disconnected Output: {} -> {}", out_port_name, sys_in)
            }
            Err(e) => {
                debug!("Skip disconnect {} -> {}: {}", out_port_name, sys_in, e)
            }
        }
    }
}
