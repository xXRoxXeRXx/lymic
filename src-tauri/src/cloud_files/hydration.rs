//! Hydration: filling a placeholder with the full original of its Remote Asset (ADR-0003).
//!
//! The adapter hands in what the platform asked for; this resolves the file through the projection
//! (`Library::resolve`), streams the original from Immich and passes it to the platform in blocks.
//! The platform only accepts data at offsets that are multiples of 4096, except for the final block
//! that ends at the end of the file, so network chunks of any size are regrouped by
//! [`BlockAligner`]. The whole original is always fetched, never a preview and never a byte range.

use super::failure::{Failure, FailureKind};
use super::library::{Library, OriginalSource, OriginalStream};

/// The platform's granularity for data transfers.
pub(super) const ALIGNMENT: u64 = 4096;
/// How much is handed to the platform at once: a multiple of [`ALIGNMENT`].
pub(super) const BLOCK_BYTES: usize = 1024 * 1024;

/// A piece of the original at its offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Block {
    pub offset: u64,
    pub data: Vec<u8>,
}

/// Regroups arbitrarily sized chunks into blocks of [`BLOCK_BYTES`], so every block but the last
/// starts and ends on an [`ALIGNMENT`] boundary.
pub(super) struct BlockAligner {
    buffer: Vec<u8>,
    next_offset: u64,
}

impl BlockAligner {
    /// Starts at `offset`, which must be a multiple of [`ALIGNMENT`].
    pub(super) fn starting_at(offset: u64) -> Self {
        debug_assert_eq!(offset % ALIGNMENT, 0);
        Self {
            buffer: Vec::new(),
            next_offset: offset,
        }
    }

    /// Takes the next bytes and returns every full block they complete.
    pub(super) fn push(&mut self, data: &[u8]) -> Vec<Block> {
        self.buffer.extend_from_slice(data);
        let mut blocks = Vec::new();
        while self.buffer.len() >= BLOCK_BYTES {
            let rest = self.buffer.split_off(BLOCK_BYTES);
            let data = std::mem::replace(&mut self.buffer, rest);
            blocks.push(Block {
                offset: self.next_offset,
                data,
            });
            self.next_offset += BLOCK_BYTES as u64;
        }
        blocks
    }

    /// The bytes still held, which end the file.
    pub(super) fn finish(self) -> Option<Block> {
        (!self.buffer.is_empty()).then_some(Block {
            offset: self.next_offset,
            data: self.buffer,
        })
    }
}

/// Where the platform takes data. A refusal ends the hydration.
pub(super) trait DataSink {
    fn transfer(&mut self, block: &Block) -> Result<(), Failure>;
}

/// What the platform asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DataRequest {
    /// The placeholder's path relative to the Virtual Library root.
    pub path: String,
    /// The first byte the reader needs.
    pub required_offset: u64,
    /// The size the placeholder was created with.
    pub file_size: u64,
}

fn align_down(value: u64) -> u64 {
    value / ALIGNMENT * ALIGNMENT
}

/// Hydrates the placeholder at `request.path`. Returns the number of bytes handed to the platform.
///
/// The bytes below the aligned start of the requested range are read but not transferred; with the
/// full hydration policy that start is 0. A body that does not end exactly at the size the
/// placeholder announced fails the read: the platform would otherwise wait for bytes that never
/// come, or reject the excess.
pub(super) async fn hydrate<L, S, K>(
    library: &L,
    source: &S,
    sink: &mut K,
    request: &DataRequest,
) -> Result<u64, Failure>
where
    L: Library,
    S: OriginalSource,
    K: DataSink,
{
    let asset = library
        .resolve(&request.path)
        .await
        .map_err(Failure::from_library)?;
    let mut stream = source.open(&asset).await?;
    // The server says how long the original is before sending it: a size that differs from the
    // placeholder's cannot hydrate it, so fail now instead of downloading the whole body first.
    if let Some(announced) = stream.content_length() {
        if announced != request.file_size {
            return Err(size_mismatch(
                request,
                if announced > request.file_size {
                    "longer"
                } else {
                    "shorter"
                },
            ));
        }
    }

    let start = align_down(request.required_offset);
    let mut aligner = BlockAligner::starting_at(start);
    let mut position: u64 = 0;
    let mut transferred: u64 = 0;
    while let Some(chunk) = stream.next_chunk().await? {
        let end = position + chunk.len() as u64;
        if end > request.file_size {
            return Err(size_mismatch(request, "longer"));
        }
        let skip = start.saturating_sub(position).min(chunk.len() as u64) as usize;
        position = end;
        for block in aligner.push(&chunk[skip..]) {
            sink.transfer(&block)?;
            transferred += block.data.len() as u64;
        }
    }
    if position != request.file_size {
        return Err(size_mismatch(request, "shorter"));
    }
    if let Some(block) = aligner.finish() {
        sink.transfer(&block)?;
        transferred += block.data.len() as u64;
    }
    Ok(transferred)
}

fn size_mismatch(request: &DataRequest, how: &str) -> Failure {
    Failure::new(
        FailureKind::Failed,
        format!(
            "the original is {how} than the {} bytes announced for '{}'",
            request.file_size, request.path
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::immich::RemoteAssetId;
    use crate::virtual_library::{Entry, VirtualLibraryError};
    use bytes::Bytes;
    use std::sync::Mutex;

    fn bytes_of(length: usize) -> Vec<u8> {
        (0..length).map(|index| (index % 251) as u8).collect()
    }

    #[test]
    fn the_aligner_emits_full_blocks_at_aligned_offsets_and_holds_the_rest() {
        let data = bytes_of(BLOCK_BYTES * 2 + 100);
        let mut aligner = BlockAligner::starting_at(0);

        let mut blocks = aligner.push(&data[..BLOCK_BYTES - 1]);
        assert!(blocks.is_empty(), "not a full block yet");
        blocks.extend(aligner.push(&data[BLOCK_BYTES - 1..]));
        let last = aligner.finish().unwrap();

        assert_eq!(
            blocks.iter().map(|b| b.offset).collect::<Vec<_>>(),
            vec![0, BLOCK_BYTES as u64]
        );
        assert!(blocks.iter().all(|b| b.data.len() == BLOCK_BYTES));
        assert_eq!(last.offset, 2 * BLOCK_BYTES as u64);
        assert_eq!(last.data, &data[2 * BLOCK_BYTES..]);
        assert_eq!(BLOCK_BYTES as u64 % ALIGNMENT, 0);
    }

    #[test]
    fn the_aligner_starts_at_the_offset_it_is_given_and_has_nothing_to_finish_when_empty() {
        let mut aligner = BlockAligner::starting_at(8192);
        let blocks = aligner.push(&bytes_of(BLOCK_BYTES));

        assert_eq!(blocks[0].offset, 8192);
        assert_eq!(aligner.finish(), None);
    }

    struct FakeLibrary {
        resolves: Result<RemoteAssetId, VirtualLibraryError>,
    }

    impl Library for FakeLibrary {
        async fn list(&self, _path: &str) -> Result<Vec<Entry>, VirtualLibraryError> {
            unreachable!("hydration never lists")
        }

        async fn resolve(&self, _path: &str) -> Result<RemoteAssetId, VirtualLibraryError> {
            self.resolves.clone()
        }
    }

    fn library() -> FakeLibrary {
        FakeLibrary {
            resolves: Ok(RemoteAssetId::new("asset-1")),
        }
    }

    /// An original served in the given chunks, then either its end or an error.
    struct FakeSource {
        chunks: Vec<Bytes>,
        then_error: Option<Failure>,
        open_error: Option<Failure>,
        opened: Mutex<Vec<String>>,
        /// The `Content-Length` the server announces, if it does.
        announced: Option<u64>,
    }

    impl FakeSource {
        fn serving(data: &[u8], chunk: usize) -> Self {
            Self {
                chunks: data.chunks(chunk).map(Bytes::copy_from_slice).collect(),
                then_error: None,
                open_error: None,
                opened: Mutex::new(Vec::new()),
                announced: Some(data.len() as u64),
            }
        }
    }

    struct FakeStream {
        chunks: std::vec::IntoIter<Bytes>,
        then_error: Option<Failure>,
        announced: Option<u64>,
    }

    impl OriginalSource for FakeSource {
        type Stream = FakeStream;

        async fn open(&self, asset: &RemoteAssetId) -> Result<FakeStream, Failure> {
            self.opened.lock().unwrap().push(asset.to_string());
            match &self.open_error {
                Some(error) => Err(error.clone()),
                None => Ok(FakeStream {
                    chunks: self.chunks.clone().into_iter(),
                    then_error: self.then_error.clone(),
                    announced: self.announced,
                }),
            }
        }
    }

    impl OriginalStream for FakeStream {
        fn content_length(&self) -> Option<u64> {
            self.announced
        }

        async fn next_chunk(&mut self) -> Result<Option<Bytes>, Failure> {
            match self.chunks.next() {
                Some(chunk) => Ok(Some(chunk)),
                None => match self.then_error.take() {
                    Some(error) => Err(error),
                    None => Ok(None),
                },
            }
        }
    }

    #[derive(Default)]
    struct RecordingSink {
        blocks: Vec<Block>,
        refuse_after: Option<usize>,
    }

    impl DataSink for RecordingSink {
        fn transfer(&mut self, block: &Block) -> Result<(), Failure> {
            if self.refuse_after == Some(self.blocks.len()) {
                return Err(Failure::new(FailureKind::Failed, "platform refused"));
            }
            self.blocks.push(block.clone());
            Ok(())
        }
    }

    fn request(file_size: usize) -> DataRequest {
        DataRequest {
            path: "All Photos/2024/03 March/IMG_0001.jpg".to_string(),
            required_offset: 0,
            file_size: file_size as u64,
        }
    }

    fn assert_platform_alignment(blocks: &[Block], file_size: u64) {
        for block in blocks {
            assert_eq!(block.offset % ALIGNMENT, 0, "offset {}", block.offset);
            let end = block.offset + block.data.len() as u64;
            assert!(
                (block.data.len() as u64).is_multiple_of(ALIGNMENT) || end == file_size,
                "only the block ending the file may be unaligned: {block:?}"
            );
        }
    }

    #[tokio::test]
    async fn streams_the_whole_original_to_the_platform_in_aligned_blocks() {
        let data = bytes_of(BLOCK_BYTES * 2 + 12_345);
        // Network chunks that never line up with the block size.
        let source = FakeSource::serving(&data, 700_001);
        let mut sink = RecordingSink::default();

        let transferred = hydrate(&library(), &source, &mut sink, &request(data.len()))
            .await
            .unwrap();

        assert_eq!(transferred, data.len() as u64);
        assert_platform_alignment(&sink.blocks, data.len() as u64);
        let joined: Vec<u8> = sink.blocks.iter().flat_map(|b| b.data.clone()).collect();
        assert_eq!(joined, data);
        assert_eq!(
            sink.blocks.iter().map(|b| b.offset).collect::<Vec<_>>(),
            vec![0, BLOCK_BYTES as u64, 2 * BLOCK_BYTES as u64]
        );
        assert_eq!(*source.opened.lock().unwrap(), vec!["asset-1".to_string()]);
    }

    #[tokio::test]
    async fn an_original_announced_with_another_size_fails_before_any_byte_is_transferred() {
        let data = bytes_of(5_000);
        let mut source = FakeSource::serving(&data, 1_000);
        source.announced = Some(4_000);
        let mut sink = RecordingSink::default();

        let failure = hydrate(&library(), &source, &mut sink, &request(5_000))
            .await
            .unwrap_err();

        assert_eq!(failure.kind, FailureKind::Failed);
        assert!(sink.blocks.is_empty(), "nothing was handed to the platform");
    }

    #[tokio::test]
    async fn an_original_announced_with_the_right_size_or_none_is_served() {
        let data = bytes_of(5_000);
        for announced in [Some(5_000), None] {
            let mut source = FakeSource::serving(&data, 1_000);
            source.announced = announced;
            let mut sink = RecordingSink::default();

            hydrate(&library(), &source, &mut sink, &request(5_000))
                .await
                .unwrap();

            assert_eq!(sink.blocks.len(), 1, "announced {announced:?}");
        }
    }

    #[tokio::test]
    async fn a_small_file_is_one_unaligned_block_that_ends_the_file() {
        let data = bytes_of(5_000);
        let source = FakeSource::serving(&data, 1_000);
        let mut sink = RecordingSink::default();

        hydrate(&library(), &source, &mut sink, &request(5_000))
            .await
            .unwrap();

        assert_eq!(sink.blocks, vec![Block { offset: 0, data }]);
    }

    #[tokio::test]
    async fn an_unknown_path_fails_as_not_found_without_touching_the_network() {
        let library = FakeLibrary {
            resolves: Err(VirtualLibraryError::NotFound),
        };
        let source = FakeSource::serving(&[1], 1);

        let failure = hydrate(
            &library,
            &source,
            &mut RecordingSink::default(),
            &request(1),
        )
        .await
        .unwrap_err();

        assert_eq!(failure.kind, FailureKind::NotFound);
        assert!(source.opened.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_refused_or_failing_original_fails_the_read_with_its_kind() {
        let mut source = FakeSource::serving(&[], 1);
        source.open_error = Some(Failure::new(FailureKind::Unauthorized, "401"));

        let failure = hydrate(
            &library(),
            &source,
            &mut RecordingSink::default(),
            &request(1),
        )
        .await
        .unwrap_err();

        assert_eq!(failure.kind, FailureKind::Unauthorized);
    }

    #[tokio::test]
    async fn a_connection_that_drops_mid_download_fails_the_read() {
        let data = bytes_of(BLOCK_BYTES + 10);
        let mut source = FakeSource::serving(&data[..BLOCK_BYTES + 5], BLOCK_BYTES);
        source.then_error = Some(Failure::new(FailureKind::Offline, "reset"));
        // The server announced the whole original before the connection dropped.
        source.announced = Some(data.len() as u64);

        let failure = hydrate(
            &library(),
            &source,
            &mut RecordingSink::default(),
            &request(data.len()),
        )
        .await
        .unwrap_err();

        assert_eq!(failure.kind, FailureKind::Offline);
    }

    #[tokio::test]
    async fn an_original_shorter_than_announced_fails_instead_of_leaving_the_reader_waiting() {
        let source = FakeSource::serving(&bytes_of(100), 100);

        let failure = hydrate(
            &library(),
            &source,
            &mut RecordingSink::default(),
            &request(200),
        )
        .await
        .unwrap_err();

        assert_eq!(failure.kind, FailureKind::Failed);
        assert!(failure.message.contains("shorter"), "{}", failure.message);
    }

    #[tokio::test]
    async fn an_original_longer_than_announced_fails_before_the_excess_reaches_the_platform() {
        let source = FakeSource::serving(&bytes_of(300), 300);
        let mut sink = RecordingSink::default();

        let failure = hydrate(&library(), &source, &mut sink, &request(200))
            .await
            .unwrap_err();

        assert_eq!(failure.kind, FailureKind::Failed);
        assert!(failure.message.contains("longer"), "{}", failure.message);
        assert!(sink.blocks.is_empty());
    }

    #[tokio::test]
    async fn a_platform_that_refuses_a_block_stops_the_download() {
        let data = bytes_of(BLOCK_BYTES * 3);
        let source = FakeSource::serving(&data, BLOCK_BYTES);
        let mut sink = RecordingSink {
            refuse_after: Some(1),
            ..Default::default()
        };

        let failure = hydrate(&library(), &source, &mut sink, &request(data.len()))
            .await
            .unwrap_err();

        assert_eq!(failure.message, "platform refused");
        assert_eq!(sink.blocks.len(), 1);
    }

    #[tokio::test]
    async fn a_request_that_starts_later_skips_the_bytes_before_its_aligned_start() {
        let data = bytes_of(20_000);
        let source = FakeSource::serving(&data, 3_000);
        let mut sink = RecordingSink::default();
        let request = DataRequest {
            required_offset: 5_000,
            ..request(data.len())
        };

        let transferred = hydrate(&library(), &source, &mut sink, &request)
            .await
            .unwrap();

        // 5000 aligns down to 4096; everything before it is not transferred.
        assert_eq!(transferred, 20_000 - 4_096);
        assert_eq!(sink.blocks[0].offset, 4_096);
        assert_eq!(sink.blocks[0].data, &data[4_096..]);
    }
}
