use bytes::Bytes;
use futures::{Stream, StreamExt};
use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

use crate::immich::RemoteAssetId;

pub(crate) type OriginalAssetStream =
    Pin<Box<dyn Stream<Item = Result<Bytes, String>> + Send + 'static>>;
pub(crate) type OriginalAssetFuture =
    Pin<Box<dyn Future<Output = Result<OriginalAssetStream, String>> + Send + 'static>>;

/// The request keys supplied by Cloud Files for one hydration operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FetchDataRequest {
    pub(crate) connection_key: i64,
    pub(crate) transfer_key: i64,
    pub(crate) request_key: i64,
    pub(crate) required_offset: i64,
    pub(crate) required_length: i64,
}

pub(crate) trait OriginalAssetClient: Send + Sync {
    fn stream_original(&self, asset_id: RemoteAssetId) -> OriginalAssetFuture;
}

/// Cloud Files operation seam. The platform implementation calls `CfExecute`.
pub(crate) trait DataTransfer: Send + Sync {
    /// Reports liveness while Cloud Files is waiting for the next network chunk.
    fn report_progress(
        &self,
        request: FetchDataRequest,
        total: i64,
        completed: i64,
    ) -> Result<(), String>;

    fn transfer_data(
        &self,
        request: FetchDataRequest,
        offset: i64,
        bytes: Bytes,
        completed: bool,
    ) -> Result<(), String>;
}

/// Dispatches callback work to Tauri's async worker pool, so Cloud Filter callback
/// threads only queue the request and return.
pub(crate) struct HydrationDispatcher {
    client: Arc<dyn OriginalAssetClient>,
    transfer: Arc<dyn DataTransfer>,
    cancellations: Mutex<HashMap<FetchRequestKey, CancellationToken>>,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct FetchRequestKey {
    connection_key: i64,
    transfer_key: i64,
    request_key: i64,
}

impl From<FetchDataRequest> for FetchRequestKey {
    fn from(request: FetchDataRequest) -> Self {
        Self {
            connection_key: request.connection_key,
            transfer_key: request.transfer_key,
            request_key: request.request_key,
        }
    }
}

/// Cloud Files treats a quiet provider as unresponsive.  Keep the callback
/// alive at least twice a second while the server is slow.
const PROGRESS_HEARTBEAT_INTERVAL: Duration = Duration::from_millis(500);
const PROGRESS_BYTES_INTERVAL: usize = 1024 * 1024;

impl HydrationDispatcher {
    pub(crate) fn new(
        client: Arc<dyn OriginalAssetClient>,
        transfer: Arc<dyn DataTransfer>,
    ) -> Self {
        Self {
            client,
            transfer,
            cancellations: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn dispatch(self: &Arc<Self>, asset_id: RemoteAssetId, request: FetchDataRequest) {
        let cancellation = CancellationToken::new();
        self.cancellations
            .lock()
            .expect("hydration cancellation mutex poisoned")
            .insert(request.into(), cancellation.clone());
        let dispatcher = self.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = dispatcher
                .hydrate_cancellable(asset_id.clone(), request, cancellation)
                .await
            {
                eprintln!("Hydration error for asset {}: {error}", asset_id.as_str());
            }
            dispatcher
                .cancellations
                .lock()
                .expect("hydration cancellation mutex poisoned")
                .remove(&request.into());
        });
    }

    /// Stops an outstanding request without completing it. Cloud Files owns the
    /// cancelled request and must not receive another transfer completion.
    pub(crate) fn cancel(&self, request: FetchDataRequest) {
        if let Some(cancellation) = self
            .cancellations
            .lock()
            .expect("hydration cancellation mutex poisoned")
            .get(&request.into())
        {
            cancellation.cancel();
        }
    }

    async fn hydrate(
        &self,
        asset_id: RemoteAssetId,
        request: FetchDataRequest,
    ) -> Result<(), String> {
        self.hydrate_cancellable(asset_id, request, CancellationToken::new())
            .await
    }

    async fn hydrate_cancellable(
        &self,
        asset_id: RemoteAssetId,
        request: FetchDataRequest,
        cancellation: CancellationToken,
    ) -> Result<(), String> {
        let mut stream = tokio::select! {
            _ = cancellation.cancelled() => return Ok(()),
            stream = self.client.stream_original(asset_id) => stream?,
        };
        let mut offset = request.required_offset;
        let mut heartbeat = tokio::time::interval(PROGRESS_HEARTBEAT_INTERVAL);
        heartbeat.tick().await;
        let _ = self
            .transfer
            .report_progress(request, request.required_length, 0);
        loop {
            let chunk = tokio::select! {
                _ = cancellation.cancelled() => return Ok(()),
                _ = heartbeat.tick() => {
                    let _ = self.transfer.report_progress(request, request.required_length, offset - request.required_offset);
                    continue;
                }
                chunk = stream.next() => chunk,
            };
            let Some(chunk) = chunk else { break };
            let bytes = chunk?;
            if bytes.is_empty() {
                continue;
            }
            // CfReportProviderProgress2 must be observed both frequently in
            // time and as a transfer grows.  A response chunk can be much
            // larger than 1 MiB, so split it at the provider-progress seam.
            for part in bytes.chunks(PROGRESS_BYTES_INTERVAL) {
                let part = Bytes::copy_from_slice(part);
                self.transfer
                    .transfer_data(request, offset, part.clone(), false)?;
                offset = offset
                    .checked_add(
                        i64::try_from(part.len()).map_err(|_| "original chunk is too large")?,
                    )
                    .ok_or_else(|| "original asset offset overflow".to_string())?;
                let _ = self.transfer.report_progress(
                    request,
                    request.required_length,
                    offset - request.required_offset,
                );
            }
        }
        if cancellation.is_cancelled() {
            return Ok(());
        }
        // A zero-byte completion tells Cloud Files the requested range is complete.
        // A successful transfer completion preserves the placeholder's in-sync state.
        self.transfer
            .transfer_data(request, offset, Bytes::new(), true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Mutex};
    use std::time::Duration;

    struct FakeClient {
        started: mpsc::Sender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    impl OriginalAssetClient for FakeClient {
        fn stream_original(&self, _: RemoteAssetId) -> OriginalAssetFuture {
            let _ = self.started.send(());
            self.release.lock().unwrap().recv().unwrap();
            Box::pin(async {
                Ok(Box::pin(futures::stream::iter(vec![
                    Ok(Bytes::from_static(b"original ")),
                    Ok(Bytes::from_static(b"bytes")),
                ])) as OriginalAssetStream)
            })
        }
    }

    #[derive(Default)]
    struct FakeTransfer {
        transfers: Mutex<Vec<(i64, Vec<u8>, bool)>>,
        progress: Mutex<Vec<(i64, i64)>>,
    }
    impl DataTransfer for FakeTransfer {
        fn report_progress(
            &self,
            _: FetchDataRequest,
            total: i64,
            completed: i64,
        ) -> Result<(), String> {
            self.progress.lock().unwrap().push((total, completed));
            Ok(())
        }

        fn transfer_data(
            &self,
            _: FetchDataRequest,
            offset: i64,
            bytes: Bytes,
            completed: bool,
        ) -> Result<(), String> {
            self.transfers
                .lock()
                .unwrap()
                .push((offset, bytes.to_vec(), completed));
            Ok(())
        }
    }

    #[test]
    fn fetch_data_returns_before_the_original_download_finishes() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let dispatcher = Arc::new(HydrationDispatcher::new(
            Arc::new(FakeClient {
                started: started_tx,
                release: Mutex::new(release_rx),
            }),
            Arc::new(FakeTransfer::default()),
        ));
        dispatcher.dispatch(RemoteAssetId::new("asset-id"), request());

        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        release_tx.send(()).unwrap();
    }

    #[test]
    fn opening_a_placeholder_streams_exact_original_bytes_and_completes_in_sync() {
        let (started_tx, _) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let transfer = Arc::new(FakeTransfer::default());
        let dispatcher = HydrationDispatcher::new(
            Arc::new(FakeClient {
                started: started_tx,
                release: Mutex::new(release_rx),
            }),
            transfer.clone(),
        );

        release_tx.send(()).unwrap();
        // This direct worker seam makes the expected transfer independent of scheduling.
        tauri::async_runtime::block_on(
            dispatcher.hydrate(RemoteAssetId::new("asset-id"), request()),
        )
        .unwrap();

        assert_eq!(
            *transfer.transfers.lock().unwrap(),
            [
                (32, b"original ".to_vec(), false),
                (41, b"bytes".to_vec(), false),
                (46, Vec::new(), true),
            ]
        );
        assert_eq!(
            *transfer.progress.lock().unwrap(),
            [(14, 0), (14, 9), (14, 14)]
        );
    }

    #[test]
    fn reports_progress_for_every_mebibyte_of_a_large_network_chunk() {
        struct LargeChunkClient;
        impl OriginalAssetClient for LargeChunkClient {
            fn stream_original(&self, _: RemoteAssetId) -> OriginalAssetFuture {
                Box::pin(async {
                    Ok(Box::pin(futures::stream::iter(vec![Ok(Bytes::from(vec![
                        7;
                        PROGRESS_BYTES_INTERVAL
                            * 2
                            + 1
                    ]))])) as OriginalAssetStream)
                })
            }
        }
        let transfer = Arc::new(FakeTransfer::default());
        let dispatcher = HydrationDispatcher::new(Arc::new(LargeChunkClient), transfer.clone());
        let mut large_request = request();
        large_request.required_length = (PROGRESS_BYTES_INTERVAL * 2 + 1) as i64;
        tauri::async_runtime::block_on(
            dispatcher.hydrate(RemoteAssetId::new("asset-id"), large_request),
        )
        .unwrap();

        assert_eq!(transfer.transfers.lock().unwrap().len(), 4); // 3 data parts plus completion
        assert_eq!(transfer.progress.lock().unwrap().len(), 4); // initial plus one report per part
    }

    struct PendingClient(mpsc::Sender<()>);
    impl OriginalAssetClient for PendingClient {
        fn stream_original(&self, _: RemoteAssetId) -> OriginalAssetFuture {
            let _ = self.0.send(());
            Box::pin(futures::future::pending())
        }
    }

    #[test]
    fn cancelling_an_outstanding_request_stops_the_download_without_completing_it() {
        let (started_tx, started_rx) = mpsc::channel();
        let transfer = Arc::new(FakeTransfer::default());
        let dispatcher = Arc::new(HydrationDispatcher::new(
            Arc::new(PendingClient(started_tx)),
            transfer.clone(),
        ));

        dispatcher.dispatch(RemoteAssetId::new("asset-id"), request());
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        dispatcher.cancel(request());

        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while !dispatcher.cancellations.lock().unwrap().is_empty()
            && std::time::Instant::now() < deadline
        {
            std::thread::yield_now();
        }
        assert!(dispatcher.cancellations.lock().unwrap().is_empty());
        assert!(transfer.transfers.lock().unwrap().is_empty());
    }

    fn request() -> FetchDataRequest {
        FetchDataRequest {
            connection_key: 1,
            transfer_key: 2,
            request_key: 3,
            required_offset: 32,
            required_length: 14,
        }
    }
}
