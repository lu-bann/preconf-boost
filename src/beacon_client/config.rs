use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BeaconClientConfig {
    pub beacon_client_addresses: Vec<String>,
    pub core: Option<usize>,
}
