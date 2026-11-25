//! Hydration: filling a placeholder with the full original of its Remote Asset (ADR-0003).
//!
//! The adapter hands in what the platform asked for; this resolves the file through the projection
//! (`Library::resolve`), streams the original from Immich and passes it to the platform in blocks.
//! The platform only accepts data at offsets that are multiples of 4096, except for the final block
//! that ends at the end of the file, so network chunks of any size are regrouped by
//! [`BlockAligner`]. Only the original is fetched, never a preview. It is read from the aligned start
//! of the requested range (a `Range` request when the server honours it, byte 0 with the bytes
//! before it dropped when it does not) and, under the progressive hydration policy, only a bounded
//! read-ahead past the end of that range: the platform asks again when the reader gets that far, so
//! a video starts at once and a seek does not wait for the bytes in between.

use super::failure::{Failure, FailureKind};
use super::library::{Library, OriginalSource, OriginalStream};

/// The platform's granularity for data transfers.
pub(super) const ALIGNMENT: u64 = 4096;
/// How much is handed to the platform at once: a multiple of [`ALIGNMENT`].
pub(super) const BLOCK_BYTES: usize = 1024 * 1024;
/// How far past the end of the requested range a download goes before it stops, so a sequential
/// reader is not interrupted by a request per read and a seek is not queued behind a download of
/// the whole file. The platform asks again for what the reader needs next.
pub(super) const READ_AHEAD_BYTES: u64 = 32 * 1024 * 1024;
/// The first bytes of a read are handed over as soon as they arrive (in pieces of any multiple of
/// [`ALIGNMENT`]) instead of after a whole block: a program that reads the start of a file gives
/// up when the first bytes take too long. This many bytes from the start of a read get that
/// treatment; after that the full blocks resume.
pub(super) const EAGER_BYTES: u64 = 8 * 1024 * 1024;

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

    /// Hands over what is held up to the last [`ALIGNMENT`] boundary, without waiting for a full
    /// block. `None` when less than one aligned unit is held.
    pub(super) fn flush_aligned(&mut self) -> Option<Block> {
        let ready = self.buffer.len() / ALIGNMENT as usize * ALIGNMENT as usize;
        if ready == 0 {
            return None;
        }
        let rest = self.buffer.split_off(ready);
        let data = std::mem::replace(&mut self.buffer, rest);
        let block = Block {
            offset: self.next_offset,
            data,
        };
        self.next_offset += ready as u64;
        Some(block)
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
    /// How many bytes from `required_offset` the reader waits for. 0 means everything to the end
    /// of the file.
    pub required_length: u64,
    /// The size the placeholder was created with.
    pub file_size: u64,
}

fn align_down(value: u64) -> u64 {
    value / ALIGNMENT * ALIGNMENT
}

/// Hydrates the placeholder at `request.path`. Returns the number of bytes handed to the platform.
///
/// The bytes below the aligned start of the requested range are not transferred: they are not even
/// downloaded when the server serves the range, and are read and dropped when it sends the whole
/// original. The download stops [`READ_AHEAD_BYTES`] after the end of the requested range, at a
/// block boundary, so the reader's range is always complete when it ends; a read of the whole file
/// (the full hydration policy asks for it) therefore downloads the whole file. A body that does not
/// end exactly at the size the placeholder announced fails the read: the platform would otherwise
/// wait for bytes that never come, or reject the excess.
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
    let start = align_down(request.required_offset);
    let mut stream = source.open(&asset, start).await?;
    // Where the body begins: the aligned start when the server served the range, 0 when it sent
    // the whole original. Anything else cannot be placed in the file.
    let mut position = stream.starts_at();
    if position != 0 && position != start {
        return Err(Failure::new(
            FailureKind::Failed,
            format!(
                "the original for '{}' began at byte {position}, not at the {start} asked for",
                request.path
            ),
        ));
    }
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

    let stop_at = stop_at(request);
    // The first bytes of the range are handed over as they arrive, up to the end of the range the
    // reader waits for (or [`EAGER_BYTES`] of it).
    let eager_until = if request.required_length == 0 {
        start
    } else {
        request
            .required_offset
            .saturating_add(request.required_length)
            .min(start.saturating_add(EAGER_BYTES))
            .min(request.file_size)
    };
    let mut aligner = BlockAligner::starting_at(start);
    let mut transferred: u64 = 0;
    // The end of what the platform has been given.
    let mut handed_over = start;
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
            handed_over = block.offset + block.data.len() as u64;
        }
        if handed_over < eager_until {
            if let Some(block) = aligner.flush_aligned() {
                sink.transfer(&block)?;
                transferred += block.data.len() as u64;
                handed_over = block.offset + block.data.len() as u64;
            }
        }
        if handed_over >= stop_at && handed_over < request.file_size {
            // Enough for now: dropping the stream ends the download. What the aligner still holds
            // is not on a block boundary and is read again when the platform asks.
            return Ok(transferred);
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

/// The offset from which the download may stop: past the end of the requested range plus the
/// read-ahead, or the end of the file for a read of all of it.
fn stop_at(request: &DataRequest) -> u64 {
    if request.required_length == 0 {
        return request.file_size;
    }
    request
        .required_offset
        .saturating_add(request.required_length)
        .saturating_add(READ_AHEAD_BYTES)
        .min(request.file_size)
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
        /// The byte each open asked to start at.
        opened_from: Mutex<Vec<u64>>,
        /// Whether the server honours a range request (otherwise it sends the whole original).
        ranged: bool,
        /// Serves from this byte whatever was asked for.
        forced_start: Option<u64>,
        data: Vec<u8>,
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
                opened_from: Mutex::new(Vec::new()),
                ranged: false,
                forced_start: None,
                data: data.to_vec(),
                announced: Some(data.len() as u64),
            }
        }
    }

    struct FakeStream {
        starts_at: u64,
        chunks: std::vec::IntoIter<Bytes>,
        then_error: Option<Failure>,
        announced: Option<u64>,
    }

    impl OriginalSource for FakeSource {
        type Stream = FakeStream;

        async fn open(&self, asset: &RemoteAssetId, from: u64) -> Result<FakeStream, Failure> {
            self.opened.lock().unwrap().push(asset.to_string());
            self.opened_from.lock().unwrap().push(from);
            let served_from = self
                .forced_start
                .or((self.ranged && from > 0).then_some(from));
            let (starts_at, chunks) = if let Some(from) = served_from {
                (
                    from,
                    self.data[from as usize..]
                        .chunks(3_000)
                        .map(Bytes::copy_from_slice)
                        .collect(),
                )
            } else {
                (0, self.chunks.clone())
            };
            match &self.open_error {
                Some(error) => Err(error.clone()),
                None => Ok(FakeStream {
                    starts_at,
                    chunks: chunks.into_iter(),
                    then_error: self.then_error.clone(),
                    announced: self.announced,
                }),
            }
        }
    }

    impl OriginalStream for FakeStream {
        fn starts_at(&self) -> u64 {
            self.starts_at
        }

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
            required_length: 0,
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
    async fn a_later_start_is_asked_for_as_a_range_and_only_the_rest_is_downloaded() {
        let data = bytes_of(20_000);
        let mut source = FakeSource::serving(&data, 3_000);
        source.ranged = true;
        let mut sink = RecordingSink::default();
        let request = DataRequest {
            required_offset: 5_000,
            ..request(data.len())
        };

        let transferred = hydrate(&library(), &source, &mut sink, &request)
            .await
            .unwrap();

        assert_eq!(*source.opened_from.lock().unwrap(), [4_096]);
        assert_eq!(transferred, 20_000 - 4_096);
        assert_eq!(sink.blocks[0].offset, 4_096);
        assert_eq!(sink.blocks[0].data, &data[4_096..]);
    }

    #[tokio::test]
    async fn a_server_without_range_support_gives_the_same_blocks_as_one_with() {
        let data = bytes_of(BLOCK_BYTES + 20_000);
        let mut blocks = Vec::new();
        for ranged in [true, false] {
            let mut source = FakeSource::serving(&data, 700_001);
            source.ranged = ranged;
            let mut sink = RecordingSink::default();
            let request = DataRequest {
                required_offset: 2 * ALIGNMENT + 1,
                ..request(data.len())
            };

            hydrate(&library(), &source, &mut sink, &request)
                .await
                .unwrap();

            assert_platform_alignment(&sink.blocks, data.len() as u64);
            blocks.push(sink.blocks);
        }
        assert_eq!(blocks[0], blocks[1]);
        assert_eq!(blocks[0][0].offset, 2 * ALIGNMENT);
    }

    #[tokio::test]
    async fn a_read_from_the_start_never_asks_for_a_range() {
        let data = bytes_of(5_000);
        let mut source = FakeSource::serving(&data, 1_000);
        source.ranged = true;

        hydrate(
            &library(),
            &source,
            &mut RecordingSink::default(),
            &request(data.len()),
        )
        .await
        .unwrap();

        assert_eq!(*source.opened_from.lock().unwrap(), [0]);
    }

    #[tokio::test]
    async fn a_body_that_starts_at_an_unexpected_byte_fails_the_read() {
        let data = bytes_of(20_000);
        let mut source = FakeSource::serving(&data, 3_000);
        // The server serves from 4096, but the request wanted to start at 8192.
        source.forced_start = Some(4_096);
        let request = DataRequest {
            required_offset: 8_192,
            ..request(data.len())
        };

        let failure = hydrate(&library(), &source, &mut RecordingSink::default(), &request)
            .await
            .unwrap_err();

        assert_eq!(failure.kind, FailureKind::Failed);
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

    /// A reader's range inside a large file, as the progressive policy asks for it.
    fn ranged_request(offset: u64, length: u64, file_size: u64) -> DataRequest {
        DataRequest {
            required_offset: offset,
            required_length: length,
            file_size,
            ..request(0)
        }
    }

    #[tokio::test]
    async fn a_read_of_the_start_stops_a_read_ahead_after_its_range_instead_of_the_whole_file() {
        let data = bytes_of(READ_AHEAD_BYTES as usize + 4 * BLOCK_BYTES);
        let source = FakeSource::serving(&data, 700_001);
        let mut sink = RecordingSink::default();

        let transferred = hydrate(
            &library(),
            &source,
            &mut sink,
            &ranged_request(0, 4_096, data.len() as u64),
        )
        .await
        .unwrap();

        // The range plus the read-ahead, rounded up to a block, and not the whole file.
        assert!(transferred >= READ_AHEAD_BYTES + 4_096, "{transferred}");
        assert!(transferred < data.len() as u64, "{transferred}");
        assert_platform_alignment(&sink.blocks, data.len() as u64);
        let joined: Vec<u8> = sink.blocks.iter().flat_map(|b| b.data.clone()).collect();
        assert_eq!(joined, data[..joined.len()]);
    }

    #[tokio::test]
    async fn a_seek_far_into_a_file_downloads_from_there_and_not_the_bytes_in_between() {
        let data = bytes_of(3 * READ_AHEAD_BYTES as usize);
        let mut source = FakeSource::serving(&data, 700_001);
        source.ranged = true;
        let mut sink = RecordingSink::default();
        let offset = 2 * READ_AHEAD_BYTES + 100;

        hydrate(
            &library(),
            &source,
            &mut sink,
            &ranged_request(offset, 8_192, data.len() as u64),
        )
        .await
        .unwrap();

        let from = align_down(offset);
        assert_eq!(*source.opened_from.lock().unwrap(), [from]);
        assert_eq!(sink.blocks[0].offset, from);
        let last = sink.blocks.last().unwrap();
        assert_eq!(
            last.offset + last.data.len() as u64,
            data.len() as u64,
            "the end of the file is reached before the read-ahead is used up"
        );
        assert_platform_alignment(&sink.blocks, data.len() as u64);
    }

    #[tokio::test]
    async fn the_range_the_reader_waits_for_is_complete_when_the_download_stops() {
        // A range that ends in the middle of a block, far from the end of the file.
        let data = bytes_of(4 * READ_AHEAD_BYTES as usize);
        let source = FakeSource::serving(&data, 123_457);
        let mut sink = RecordingSink::default();
        let range = (5_000u64, 3 * BLOCK_BYTES as u64 + 17);

        hydrate(
            &library(),
            &source,
            &mut sink,
            &ranged_request(range.0, range.1, data.len() as u64),
        )
        .await
        .unwrap();

        let last = sink.blocks.last().unwrap();
        assert!(last.offset + last.data.len() as u64 >= range.0 + range.1);
        assert_eq!(sink.blocks[0].offset, 4_096);
    }

    #[tokio::test]
    async fn a_read_of_the_whole_file_downloads_all_of_it_under_either_policy() {
        let data = bytes_of(READ_AHEAD_BYTES as usize + BLOCK_BYTES + 7);
        for length in [0, data.len() as u64] {
            let source = FakeSource::serving(&data, 700_001);
            let mut sink = RecordingSink::default();

            let transferred = hydrate(
                &library(),
                &source,
                &mut sink,
                &ranged_request(0, length, data.len() as u64),
            )
            .await
            .unwrap();

            assert_eq!(transferred, data.len() as u64, "length {length}");
        }
    }

    #[test]
    fn the_aligner_hands_over_what_it_holds_up_to_the_last_aligned_unit() {
        let mut aligner = BlockAligner::starting_at(0);
        aligner.push(&bytes_of(ALIGNMENT as usize * 2 + 100));

        let block = aligner.flush_aligned().unwrap();

        assert_eq!(block.offset, 0);
        assert_eq!(block.data.len() as u64, 2 * ALIGNMENT);
        assert!(
            aligner.flush_aligned().is_none(),
            "100 bytes are not a unit"
        );
        // The rest keeps its place: the next block starts where the first ended.
        aligner.push(&bytes_of(ALIGNMENT as usize));
        assert_eq!(aligner.flush_aligned().unwrap().offset, 2 * ALIGNMENT);
    }

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Records, with the number of network chunks delivered so far, each block as it is handed over.
    struct TimedSink {
        delivered: Arc<AtomicUsize>,
        blocks: Vec<(usize, Block)>,
    }

    impl DataSink for TimedSink {
        fn transfer(&mut self, block: &Block) -> Result<(), Failure> {
            self.blocks
                .push((self.delivered.load(Ordering::SeqCst), block.clone()));
            Ok(())
        }
    }

    /// A source whose streams count the chunks they deliver.
    struct CountingSource {
        inner: FakeSource,
        delivered: Arc<AtomicUsize>,
    }

    struct CountingStream {
        inner: FakeStream,
        delivered: Arc<AtomicUsize>,
    }

    impl OriginalSource for CountingSource {
        type Stream = CountingStream;

        async fn open(&self, asset: &RemoteAssetId, from: u64) -> Result<CountingStream, Failure> {
            Ok(CountingStream {
                inner: self.inner.open(asset, from).await?,
                delivered: self.delivered.clone(),
            })
        }
    }

    impl OriginalStream for CountingStream {
        fn starts_at(&self) -> u64 {
            self.inner.starts_at()
        }

        fn content_length(&self) -> Option<u64> {
            self.inner.content_length()
        }

        async fn next_chunk(&mut self) -> Result<Option<Bytes>, Failure> {
            let chunk = self.inner.next_chunk().await?;
            if chunk.is_some() {
                self.delivered.fetch_add(1, Ordering::SeqCst);
            }
            Ok(chunk)
        }
    }

    #[tokio::test]
    async fn the_range_a_reader_waits_for_is_handed_over_with_the_first_chunks_not_after_a_full_block(
    ) {
        // Chunks of 16 KiB, as a network delivers them: the first 64 KiB are there after 4 chunks.
        let data = bytes_of(4 * BLOCK_BYTES);
        let delivered = Arc::new(AtomicUsize::new(0));
        let source = CountingSource {
            inner: FakeSource::serving(&data, 16 * 1024),
            delivered: delivered.clone(),
        };
        let mut sink = TimedSink {
            delivered,
            blocks: Vec::new(),
        };

        hydrate(
            &library(),
            &source,
            &mut sink,
            &ranged_request(0, 65_536, data.len() as u64),
        )
        .await
        .unwrap();

        let (chunks_at_first, first) = &sink.blocks[0];
        assert_eq!(first.offset, 0);
        assert!(*chunks_at_first <= 4, "after {chunks_at_first} chunks");
        let covered: u64 = sink
            .blocks
            .iter()
            .take_while(|(_, block)| block.offset < 65_536)
            .map(|(_, block)| block.data.len() as u64)
            .sum();
        assert!(covered >= 65_536);
        let blocks: Vec<Block> = sink.blocks.iter().map(|(_, b)| b.clone()).collect();
        assert_platform_alignment(&blocks, data.len() as u64);
        let joined: Vec<u8> = blocks.iter().flat_map(|b| b.data.clone()).collect();
        assert_eq!(joined, data, "every byte arrives once, in order");
    }
}
