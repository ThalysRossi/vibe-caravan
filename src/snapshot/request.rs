use std::path::Path;

use crate::config::Mode;

pub struct SnapshotRequest<'a> {
    pub mode: Mode,
    pub snapshot_every: Option<u32>,
    pub completed_batch_count: u32,
    pub batch_id: &'a str,
    pub destination_root: &'a Path,
    pub snapshot_root: Option<&'a Path>,
}
