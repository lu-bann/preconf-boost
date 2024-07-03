use beacon_client::client::MultiBeaconClient;
use cb_common::{config::load_module_config, utils::initialize_tracing_log};

use elector::GatewayElector;

use config::PreconfConfig;
use tokio::sync::mpsc;
use tracing::{error, info};

mod beacon_client;
mod config;
mod elector;
mod types;

#[tokio::main]
async fn main() {
    initialize_tracing_log();

    let config = load_module_config::<PreconfConfig>().expect("failed to load config");

    // start beacon client
    let (beacon_tx, _) = tokio::sync::broadcast::channel(10);
    let multi_beacon_client = MultiBeaconClient::from_endpoint_strs(&config.extra.beacon_nodes);
    multi_beacon_client
        .subscribe_to_payload_attributes_events(beacon_tx.clone())
        .await;
    let (duties_tx, duties_rx) = mpsc::unbounded_channel();
    tokio::spawn(
        multi_beacon_client.subscribe_to_proposer_duties(duties_tx, beacon_tx.subscribe()),
    );

    info!(module_id = config.id, "Starting module");

    let elector = GatewayElector::new(config, duties_rx);

    if let Err(err) = elector.run().await {
        error!(?err, "Error running elector")
    }
}
