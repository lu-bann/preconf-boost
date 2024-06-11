use std::collections::HashMap;

use alloy_rpc_types_beacon::{BlsPublicKey, BlsSignature};
use cb_common::pbs::{COMMIT_BOOST_API, PUBKEYS_PATH, SIGN_REQUEST_PATH};
use cb_common::types::Chain;
use cb_crypto::types::SignRequest;

use futures::future::join_all;
use rand::seq::SliceRandom;
use reqwest::Client;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::{
    beacon_client::types::ProposerDuty,
    config::PreconfConfig,
    types::{GatewayElection, SignedGatewayElection, ELECT_GATEWAY_PATH},
};

/// Commit module that delegates preconf rights to external gateway
pub struct GatewayElector {
    pub chain: Chain,
    pub id: String,
    pub url: String,
    pub config: PreconfConfig,

    /// Slot being proposed
    pub next_slot: u64,

    /// Proposer duties indexed by slot number
    pub duties: HashMap<u64, ProposerDuty>,

    /// Elected gateways by slot
    elections: HashMap<u64, BlsPublicKey>,

    /// Channel to receive proposer duties updates
    duties_rx: mpsc::UnboundedReceiver<Vec<ProposerDuty>>,
}

impl GatewayElector {
    pub fn new(
        chain: Chain,
        id: String,
        url: String,
        config: PreconfConfig,

        duties_rx: mpsc::UnboundedReceiver<Vec<ProposerDuty>>,
    ) -> Self {
        Self {
            chain,
            id,
            url,
            config,

            next_slot: 0,

            duties: HashMap::new(),
            elections: HashMap::new(),

            duties_rx,
        }
    }
}

impl GatewayElector {
    pub async fn run(mut self) {
        info!("Fetching validator pubkeys");

        let validator_pubkeys = self.get_pubkeys().await;

        info!("Fetched {} pubkeys", validator_pubkeys.len());

        while let Some(duties) = self.duties_rx.recv().await {
            self.duties.clear();

            // filter only slots where we're proposer and we havent elected yet
            let our_duties: Vec<_> = duties
                .into_iter()
                .filter(|d| {
                    validator_pubkeys.contains(&d.public_key)
                        && !self.elections.contains_key(&d.slot)
                })
                .collect();

            for duty in our_duties {
                // this could be done in parallel
                self.elect_gateway(duty).await;
            }
        }
    }

    /// Delegate preconf rights
    async fn elect_gateway(&mut self, duty: ProposerDuty) {
        let gateway_pubkey = *self
            .config
            .trusted_gateways
            .choose(&mut rand::thread_rng())
            .unwrap();

        info!(validator_pubkey = %duty.public_key, %gateway_pubkey, slot = duty.slot, "Sending gateway delegation");

        let election_message = GatewayElection {
            gateway_pubkey,
            slot: self.next_slot,
            validator_pubkey: duty.public_key,
            validator_index: duty.validator_index,
        };

        let request = SignRequest::builder(&self.id, duty.public_key).with_msg(&election_message);
        let url = format!("{}{COMMIT_BOOST_API}{SIGN_REQUEST_PATH}", self.url);

        let response = reqwest::Client::new()
            .post(url)
            .json(&request)
            .send()
            .await
            .expect("failed to get request");

        let status = response.status();
        let response_bytes = response.bytes().await.expect("failed to get bytes");

        if !status.is_success() {
            let err = String::from_utf8_lossy(&response_bytes).into_owned();
            error!(err, "failed sending delegation sign request");
            return;
        }

        let signature: BlsSignature =
            serde_json::from_slice(&response_bytes).expect("failed deser");

        let signed_election = SignedGatewayElection {
            message: election_message,
            signature,
        };

        let mut handles = Vec::new();

        info!("Sending delegatotion");

        for relay in &self.config.relays {
            let client = Client::new();
            handles.push(
                client
                    .post(format!("{}/{ELECT_GATEWAY_PATH}", relay.url))
                    .json(&signed_election)
                    .send(),
            );
        }

        let results = join_all(handles).await;

        for res in results {
            match res {
                Ok(r) => info!("Successful election: {r:?}"),
                Err(err) => error!("Failed election: {err}"),
            }
        }
    }

    pub async fn get_pubkeys(&self) -> Vec<BlsPublicKey> {
        let url = format!("{}{COMMIT_BOOST_API}{PUBKEYS_PATH}", self.url);
        let response = reqwest::Client::new()
            .get(url)
            .send()
            .await
            .expect("failed to get request");

        let status = response.status();
        let response_bytes = response.bytes().await.expect("failed to get bytes");

        if !status.is_success() {
            let err = String::from_utf8_lossy(&response_bytes).into_owned();
            error!(err, ?status, "failed to get signature");
            std::process::exit(1);
        }

        let pubkeys: Vec<BlsPublicKey> =
            serde_json::from_slice(&response_bytes).expect("failed deser");

        pubkeys
    }
}
