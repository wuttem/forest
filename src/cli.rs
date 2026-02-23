use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Sets a custom config file
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Turn debugging information on
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub debug: u8,

    /// Tenant ID
    #[arg(long)]
    pub tenant: Option<String>,

    //  Bin API
    #[arg(long)]
    pub bind_api: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    Server {
        /// MQTT v3 Bind Address
        #[arg(long)]
        bind_mqtt_v3: Option<String>,
        /// MQTT v5 Bind Address
        #[arg(long)]
        bind_mqtt_v5: Option<String>,
    },
    Version,
    #[command(name = "create-device")]
    CreateDevice {
        /// Device ID
        #[arg(long)]
        device_id: String,
    },
}
