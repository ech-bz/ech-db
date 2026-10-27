use std::net::SocketAddr;
use std::sync::Arc;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use ech_db_server::fdb::Db;
use ech_db_server::server::Server;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    listen: SocketAddr,
    #[arg(long)]
    cluster_file: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let args = Args::parse();
    let _network = unsafe { foundationdb::boot() };
    let db = Db::open(&args.cluster_file)?;
    let server = Arc::new(Server::new(db));
    server.run(args.listen).await?;
    Ok(())
}
