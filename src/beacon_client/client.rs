#![allow(dead_code)]

use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use alloy_primitives::B256;
use alloy_rpc_types_beacon::events::PayloadAttributesEvent;
use futures::{future::join_all, StreamExt};
use reqwest_eventsource::EventSource;
use tokio::{
    sync::{
        broadcast::{self, Sender},
        mpsc::UnboundedSender,
    },
    task::JoinError,
    time::sleep,
};
use tracing::{debug, error, warn};
use url::Url;

use crate::beacon_client::{
    error::BeaconClientError,
    types::{ApiResult, BeaconResponse, ProposerDuty, SyncStatus},
};

const BEACON_CLIENT_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct MultiBeaconClient {
    /// Vec of all beacon clients with a fixed usize ID used when
    /// fetching: `beacon_clients_by_last_response`
    pub beacon_clients: Vec<(usize, Arc<BeaconClient>)>,
    /// The ID of the beacon client with the most recent successful response.
    pub best_beacon_instance: Arc<AtomicUsize>,
}

impl MultiBeaconClient {
    pub fn new(beacon_clients: Vec<Arc<BeaconClient>>) -> Self {
        let beacon_clients_with_index = beacon_clients.into_iter().enumerate().collect();

        Self {
            beacon_clients: beacon_clients_with_index,
            best_beacon_instance: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn from_endpoint_strs(endpoints: &[String]) -> Self {
        debug!("creating new multi beacon client");
        let clients = endpoints
            .iter()
            .map(|endpoint| Arc::new(BeaconClient::from_endpoint_str(endpoint)))
            .collect();
        Self::new(clients)
    }

    /// Retrieves the sync status from multiple beacon clients and selects the best one.
    ///
    /// The function spawns async tasks to fetch the sync status from each beacon client.
    /// It then selects the sync status with the highest `head_slot`.
    pub async fn best_sync_status(&self) -> Result<SyncStatus, BeaconClientError> {
        let clients = self.beacon_clients_by_last_response();

        let handles = clients
            .into_iter()
            .map(|(_, client)| tokio::spawn(async move { client.sync_status().await }))
            .collect::<Vec<_>>();

        let results: Vec<Result<Result<SyncStatus, BeaconClientError>, JoinError>> =
            join_all(handles).await;

        let mut best_sync_status: Option<SyncStatus> = None;
        for join_result in results {
            match join_result {
                Ok(sync_status_result) => match sync_status_result {
                    Ok(sync_status) => {
                        if best_sync_status.as_ref().map_or(true, |current_best| {
                            current_best.head_slot < sync_status.head_slot
                        }) {
                            best_sync_status = Some(sync_status);
                        }
                    }
                    Err(err) => error!("Failed to get sync status: {err:?}"),
                },
                Err(join_err) => {
                    error!("Tokio join error for best_sync_status: {join_err:?}")
                }
            }
        }

        best_sync_status.ok_or(BeaconClientError::BeaconNodeUnavailable)
    }

    pub async fn get_proposer_duties(
        &self,
        epoch: u64,
    ) -> Result<(B256, Vec<ProposerDuty>), BeaconClientError> {
        let clients = self.beacon_clients_by_last_response();
        let mut last_error = None;

        for (i, client) in clients.into_iter() {
            match client.get_proposer_duties(epoch).await {
                Ok(proposer_duties) => {
                    self.best_beacon_instance.store(i, Ordering::Relaxed);
                    return Ok(proposer_duties);
                }
                Err(err) => {
                    last_error = Some(err);
                }
            }
        }

        Err(last_error.unwrap_or(BeaconClientError::BeaconNodeUnavailable))
    }

    /// `subscribe_to_payload_attributes_events` subscribes to payload attributes events from all
    /// beacon nodes.
    ///
    /// This function swaps async tasks for all beacon clients. Therefore,
    /// a single payload event will be received multiple times, likely once for every beacon node.
    pub fn subscribe_to_payload_attributes_events(&self, chan: Sender<PayloadAttributesEvent>) {
        let clients = self.beacon_clients_by_last_response();

        for (_, client) in clients {
            let chan = chan.clone();
            tokio::spawn(async move {
                client.subscribe_to_payload_attributes_events(chan).await;
            });
        }
    }

    pub async fn start_proposer_duties_sub(self, duties_tx: UnboundedSender<Vec<ProposerDuty>>) {
        let (payload_tx, mut payload_rx) = broadcast::channel(10);

        self.subscribe_to_payload_attributes_events(payload_tx);

        const EPOCH_SLOTS: u64 = 32;
        const EPOCH_REFRESH: u64 = EPOCH_SLOTS / 4;
        let mut last_updated_slot = 0;

        while let Ok(payload) = payload_rx.recv().await {
            let new_slot = payload.data.proposal_slot;
            if new_slot > last_updated_slot && new_slot % EPOCH_REFRESH == 0 {
                last_updated_slot = new_slot;

                let epoch = new_slot / EPOCH_SLOTS;
                match self.get_proposer_duties(epoch).await {
                    Ok((_, duties)) => {
                        if let Err(err) = duties_tx.send(duties) {
                            error!(?err, "failed sending duties");
                        }
                    }
                    Err(err) => {
                        error!(?err, "failed fetching duties")
                    }
                }
            }
        }
    }

    /// Returns a list of beacon clients, prioritized by the last successful response.
    ///
    /// The beacon client with the most recent successful response is placed at the
    /// beginning of the returned vector. All other clients maintain their original order.
    pub fn beacon_clients_by_last_response(&self) -> Vec<(usize, Arc<BeaconClient>)> {
        let mut instances = self.beacon_clients.clone();
        let index = self.best_beacon_instance.load(Ordering::Relaxed);
        if index != 0 {
            let pos = instances.iter().position(|(i, _)| *i == index).unwrap();
            instances.swap(0, pos);
        }
        instances
    }
}

#[derive(Clone, Debug)]
pub struct BeaconClient {
    pub http: reqwest::Client,
    pub endpoint: Url,
}

impl BeaconClient {
    pub fn new(http: reqwest::Client, endpoint: Url) -> Self {
        Self { http, endpoint }
    }

    pub fn from_endpoint_str(endpoint: &str) -> Self {
        let endpoint = Url::parse(endpoint).unwrap();
        let client = reqwest::ClientBuilder::new()
            .timeout(BEACON_CLIENT_REQUEST_TIMEOUT)
            .build()
            .unwrap();
        Self::new(client, endpoint)
    }

    pub async fn http_get(&self, path: &str) -> Result<reqwest::Response, BeaconClientError> {
        let target = self.endpoint.join(path)?;
        Ok(self.http.get(target).send().await?)
    }

    pub async fn get<T: serde::Serialize + serde::de::DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, BeaconClientError> {
        let result = self.http_get(path).await?.json().await?;
        match result {
            ApiResult::Ok(result) => Ok(result),
            ApiResult::Err(err) => Err(BeaconClientError::Api(err)),
        }
    }

    pub async fn sync_status(&self) -> Result<SyncStatus, BeaconClientError> {
        let response: BeaconResponse<SyncStatus> = self.get("eth/v1/node/syncing").await?;
        Ok(response.data)
    }

    pub async fn get_proposer_duties(
        &self,
        epoch: u64,
    ) -> Result<(B256, Vec<ProposerDuty>), BeaconClientError> {
        let endpoint = format!("eth/v1/validator/duties/proposer/{epoch}");
        let mut result: BeaconResponse<Vec<ProposerDuty>> = self.get(&endpoint).await?;
        let dependent_root_value = result.meta.remove("dependent_root").ok_or_else(|| {
            BeaconClientError::MissingExpectedData(
                "missing `dependent_root` in response".to_string(),
            )
        })?;
        let dependent_root: B256 = serde_json::from_value(dependent_root_value)?;
        Ok((dependent_root, result.data))
    }

    pub async fn subscribe_to_payload_attributes_events(
        &self,
        chan: Sender<PayloadAttributesEvent>,
    ) {
        self.subscribe_to_sse("payload_attributes", chan).await
    }
    /// Subscribe to SSE events from the beacon client `events` endpoint.
    pub async fn subscribe_to_sse<T: serde::de::DeserializeOwned>(
        &self,
        topic: &str,
        chan: Sender<T>,
    ) {
        let url = format!("{}eth/v1/events?topics={}", self.endpoint, topic);

        loop {
            let mut es = EventSource::get(&url);

            while let Some(event) = es.next().await {
                match event {
                    Ok(reqwest_eventsource::Event::Message(message)) => {
                        match serde_json::from_str::<T>(&message.data) {
                            Ok(data) => {
                                if chan.send(data).is_err() {
                                    debug!("no subscribers connected to sse broadcaster");
                                }
                            }
                            Err(err) => error!(err=%err, "Error parsing chunk"),
                        }
                    }
                    Ok(reqwest_eventsource::Event::Open) => {}
                    Err(err) => {
                        warn!(err=%err, "SSE stream ended, reconnecting...");
                        es.close();
                        break;
                    }
                }
            }
            sleep(Duration::from_millis(500)).await;
        }
    }
}
