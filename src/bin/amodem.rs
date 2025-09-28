use clap::{Parser, Subcommand};
use std::fs::File;
use std::io::{self, BufReader, BufWriter};
use trackmaker_rs::amodem::{config::Configuration, send, recv};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Send {
        #[arg(short, long)]
        input: Option<String>,
        #[arg(short, long)]
        output: Option<String>,
        #[arg(short, long, default_value_t = 1.0)]
        gain: f64,
        #[arg(long, default_value_t = 0.0)]
        silence: f64,
    },
    Recv {
        #[arg(short, long)]
        input: Option<String>,
        #[arg(short, long)]
        output: Option<String>,
    },
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();
    
    // Use BITRATE=1 configuration (1.0 kb/s, 2-QAM x 1 carriers, Fs=8.0 kHz)
    let config = Configuration::bitrate_1();
    
    eprintln!("Audio OFDM MODEM: {:.1} kb/s ({}-QAM x {} carriers) Fs={:.1} kHz",
             config.modem_bps / 1e3, 
             config.npoints,
             config.nfreq, 
             config.fs / 1e3);
    
    match cli.command {
        Commands::Send { input, output, gain, silence } => {
            let src: Box<dyn io::Read> = match input {
                Some(path) if path == "-" => Box::new(io::stdin()),
                Some(path) => Box::new(BufReader::new(File::open(path)?)),
                None => Box::new(io::stdin()),
            };
            
            let dst: Box<dyn io::Write> = match output {
                Some(path) if path == "-" => Box::new(io::stdout()),
                Some(path) => Box::new(BufWriter::new(File::create(path)?)),
                None => Box::new(io::stdout()),
            };
            
            send(&config, src, dst, gain, silence)?;
        }
        Commands::Recv { input, output } => {
            let src: Box<dyn io::Read> = match input {
                Some(path) if path == "-" => Box::new(io::stdin()),
                Some(path) => Box::new(BufReader::new(File::open(path)?)),
                None => Box::new(io::stdin()),
            };
            
            let dst: Box<dyn io::Write> = match output {
                Some(path) if path == "-" => Box::new(io::stdout()),
                Some(path) => Box::new(BufWriter::new(File::create(path)?)),
                None => Box::new(io::stdout()),
            };
            
            match recv(&config, src, dst) {
                Ok(_) => {},
                Err(e) => {
                    eprintln!("Decoding failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
    
    Ok(())
}
