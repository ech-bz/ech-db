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
    #[arg(long)]
    machine_id: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
        .init();
    let args = Args::parse();
    let _network = unsafe { foundationdb::boot() };
    let db = Db::open(&args.cluster_file, args.machine_id.as_deref())?;
    if let Some(machine_id) = &args.machine_id {
        tracing::info!(%machine_id, "configured FDB client locality");
    }
    let server = Arc::new(Server::new(db)?);
    server.run(args.listen).await?;
    Ok(())
}
