use bytes::Bytes;
use futures::{Stream, StreamExt};
use std::{future::Future, pin::Pin, sync::Arc};

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
}

impl HydrationDispatcher {
    pub(crate) fn new(
        client: Arc<dyn OriginalAssetClient>,
        transfer: Arc<dyn DataTransfer>,
    ) -> Self {
        Self { client, transfer }
    }

    pub(crate) fn dispatch(self: &Arc<Self>, asset_id: RemoteAssetId, request: FetchDataRequest) {
        let dispatcher = self.clone();
        tauri::async_runtime::spawn(async move {
            let _ = dispatcher.hydrate(asset_id, request).await;
        });
    }

    async fn hydrate(
        &self,
        asset_id: RemoteAssetId,
        request: FetchDataRequest,
    ) -> Result<(), String> {
        let mut stream = self.client.stream_original(asset_id).await?;
        let mut offset = request.required_offset;
        while let Some(chunk) = stream.next().await {
            let bytes = chunk?;
            if bytes.is_empty() {
                continue;
            }
            self.transfer
                .transfer_data(request, offset, bytes.clone(), false)?;
            offset = offset
                .checked_add(i64::try_from(bytes.len()).map_err(|_| "original chunk is too large")?)
                .ok_or_else(|| "original asset offset overflow".to_string())?;
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
    struct FakeTransfer(Mutex<Vec<(i64, Vec<u8>, bool)>>);
    impl DataTransfer for FakeTransfer {
        fn transfer_data(
            &self,
            _: FetchDataRequest,
            offset: i64,
            bytes: Bytes,
            completed: bool,
        ) -> Result<(), String> {
            self.0
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
            *transfer.0.lock().unwrap(),
            [
                (32, b"original ".to_vec(), false),
                (41, b"bytes".to_vec(), false),
                (46, Vec::new(), true),
            ]
        );
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
