use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use ech_db_server::config::Config;
use ech_db_server::fdb::Db;
use ech_db_server::server::Server;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "config.toml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let args = Args::parse();
    let config = Config::load(&args.config)?;
    let _network = unsafe { foundationdb::boot() };
    let db = Db::open(&config.fdb)?;
    let server = Arc::new(Server::new(db));
    server.run(config.listen).await?;
    Ok(())
}
