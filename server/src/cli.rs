use std::net::SocketAddr;

use clap::{command, Parser, arg, ValueEnum};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub(crate) struct Cli {

    /// Sets a socket address to listen on
    #[arg(short, long, value_name = "[ADDRESS[:PORT]]", default_value_t = SocketAddr::from(([127, 0, 0, 1], 42000)))]
    pub(crate) listen: SocketAddr,

    /// Address of a node in the ring to join
    #[arg(short, long, value_name = "[ADDRESS[:PORT]]")]
    pub(crate) ring: Option<SocketAddr>,

    /// Set the log level
    #[arg(short('L'), long, value_name = "LEVEL", value_enum, default_value_t = LogLevel::Info)]
    pub(crate) log_level: LogLevel,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub(crate) enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}