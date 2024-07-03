use alloy::rpc::types::beacon::{BlsPublicKey, BlsSignature};
use serde::{Deserialize, Serialize};
use tree_hash_derive::TreeHash;

pub const ELECT_GATEWAY_PATH: &str = "/constraints/elect_gateway";

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SignedPreconferElection {
    pub message: ElectedPreconfer,
    pub signature: BlsSignature,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, TreeHash)]
pub struct ElectedPreconfer {
    /// Slot this delegation is valid for.
    pub slot: u64,
    /// Public key of the validator proposing for `slot`.
    pub proposer_public_key: BlsPublicKey,
    /// Validator index of the validator proposing for `slot`.
    pub validator_index: u64,
    /// Gateway info. Set to default if proposer is handling pre-confirmations.
    /// Note: this should be `None` if the proposer is handling the pre-confirmations.
    /// Haven't quite figured out how to do optional TreeHash for sigp lib, so we just
    /// set this value to a default for proposer pre-confirmations.
    pub gateway_info: GatewayInfo,
}

#[derive(Debug, Default, Clone, TreeHash, Serialize, Deserialize)]
pub struct GatewayInfo {
    /// Public key of the gateway the proposer is delegating pre-confirmation control to.
    pub gateway_public_key: BlsPublicKey,
    /// Gateway recipient address builder must pay.
    pub gateway_recipient_address: ethereum_types::Address,
}
