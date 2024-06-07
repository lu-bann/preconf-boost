use alloy_rpc_types_beacon::BlsPublicKey;

#[derive(Debug, Default, Clone)]
pub struct PreconfConfig {
    pub trusted_gateways: Vec<BlsPublicKey>,
    pub beacon_nodes: Vec<String>,
}

impl PreconfConfig {
    pub fn load() -> Self {
        todo!()
    }
}
