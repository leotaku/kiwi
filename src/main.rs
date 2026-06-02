mod typst_world;

use std::sync::Arc;

use axum::{Router, response::Html, routing::get};
use clap::Parser;
use typst::syntax::VirtualPath;
use typst_kit::files::FsRoot;
use typst_world::{TypstWorld, TypstWorldContext};
use walkdir::WalkDir;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(color=clap::ColorChoice::Never)]
struct Command {
    #[arg(short = 'a', long = "addr", default_value = "0.0.0.0")]
    #[arg(help = "Address to listen on", hide_default_value = true)]
    addr: std::net::IpAddr,

    #[arg(short = 'p', long = "port", default_value = "8080")]
    #[arg(help = "Port to listen on", hide_default_value = true)]
    port: u16,

    #[arg(help = "Path to serve as HTTP root")]
    directory: std::path::PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Command::parse();

    let entries: Vec<_> = WalkDir::new(args.directory)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().ends_with(".typ") && e.metadata().is_ok_and(|m| m.is_file()))
        .collect();

    let app = Router::new();

    let addr: std::net::SocketAddr = (args.addr, args.port).into();
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
