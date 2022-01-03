use std::{
    convert::TryInto,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::{Instant, SystemTime},
};

use byteorder::{BigEndian, WriteBytesExt};
use git_features::progress::Progress;

use crate::multi_index;

mod error {
    /// The error returned by [multi_index::File::write_from_index_paths()][super::multi_index::File::write_from_index_paths()]..
    #[derive(Debug, thiserror::Error)]
    #[allow(missing_docs)]
    pub enum Error {
        #[error(transparent)]
        Io(#[from] std::io::Error),
        #[error("Interrupted")]
        Interrupted,
        #[error(transparent)]
        OpenIndex(#[from] crate::index::init::Error),
    }
}
pub use error::Error;

/// An entry suitable for sorting and writing
pub(crate) struct Entry {
    pub(crate) id: git_hash::ObjectId,
    pub(crate) pack_index: u32,
    pub(crate) pack_offset: crate::data::Offset,
    /// Used for sorting in case of duplicates
    index_mtime: SystemTime,
}

/// Options for use in [`multi_index::File::write_from_index_paths()`].
pub struct Options {
    /// The kind of hash to use for objects and to expect in the input files.
    pub object_hash: git_hash::Kind,
}

/// The result of [`multi_index::File::write_from_index_paths()`].
pub struct Outcome<P> {
    /// The calculated multi-index checksum of the file at `multi_index_path`.
    pub multi_index_checksum: git_hash::ObjectId,
    /// The input progress
    pub progress: P,
}

impl multi_index::File {
    pub(crate) const SIGNATURE: &'static [u8] = b"MIDX";
    pub(crate) const HEADER_LEN: usize = 4 /*signature*/ +
        1 /*version*/ +
        1 /*object id version*/ +
        1 /*num chunks */ +
        1 /*num base files */ +
        4 /*num pack files*/;

    /// Create a new multi-index file for writing to `out` from the pack index files at `index_paths`.
    ///
    /// Progress is sent to `progress` and interruptions checked via `should_interrupt`.
    pub fn write_from_index_paths<P>(
        mut index_paths: Vec<PathBuf>,
        out: impl std::io::Write,
        mut progress: P,
        should_interrupt: &AtomicBool,
        Options { object_hash }: Options,
    ) -> Result<Outcome<P>, Error>
    where
        P: Progress,
    {
        let out = git_features::hash::Write::new(out, object_hash);
        let (index_paths_sorted, index_filenames_sorted) = {
            index_paths.sort();
            let file_names = index_paths
                .iter()
                .map(|p| PathBuf::from(p.file_name().expect("file name present")))
                .collect::<Vec<_>>();
            (index_paths, file_names)
        };

        let entries = {
            let mut entries = Vec::new();
            let start = Instant::now();
            let mut progress = progress.add_child("Collecting entries");
            progress.init(Some(index_paths_sorted.len()), git_features::progress::count("indices"));

            // This could be parallelized… but it's probably not worth it unless you have 500mio objects.
            for (index_id, index) in index_paths_sorted.iter().enumerate() {
                let mtime = index
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                let index = crate::index::File::at(index, object_hash)?;

                entries.reserve(index.num_objects() as usize);
                entries.extend(index.iter().map(|e| Entry {
                    id: e.oid,
                    pack_index: index_id as u32,
                    pack_offset: e.pack_offset,
                    index_mtime: mtime,
                }));
                progress.inc();
                if should_interrupt.load(Ordering::Relaxed) {
                    return Err(Error::Interrupted);
                }
            }
            progress.show_throughput(start);

            let start = Instant::now();
            progress.set_name("Deduplicate");
            progress.init(Some(entries.len()), git_features::progress::count("entries"));
            entries.sort_by(|l, r| {
                l.id.cmp(&r.id)
                    .then_with(|| l.index_mtime.cmp(&r.index_mtime).reverse())
                    .then_with(|| l.pack_index.cmp(&r.pack_index))
            });
            entries.dedup_by_key(|e| e.id);
            progress.inc_by(entries.len());
            progress.show_throughput(start);
            if should_interrupt.load(Ordering::Relaxed) {
                return Err(Error::Interrupted);
            }
            entries
        };

        let mut cf = git_chunk::file::Index::for_writing();
        cf.plan_chunk(
            multi_index::chunk::index_names::ID,
            multi_index::chunk::index_names::storage_size(&index_filenames_sorted),
        );
        cf.plan_chunk(multi_index::chunk::fanout::ID, multi_index::chunk::fanout::SIZE as u64);
        cf.plan_chunk(
            multi_index::chunk::lookup::ID,
            multi_index::chunk::lookup::storage_size(entries.len(), object_hash),
        );
        cf.plan_chunk(
            multi_index::chunk::offsets::ID,
            multi_index::chunk::offsets::storage_size(entries.len()),
        );

        let num_large_offsets = multi_index::chunk::large_offsets::num_large_offsets(&entries);
        if let Some(num_large_offsets) = num_large_offsets {
            cf.plan_chunk(
                multi_index::chunk::large_offsets::ID,
                multi_index::chunk::large_offsets::storage_size(num_large_offsets),
            );
        }

        let mut write_progress = progress.add_child("Writing multi-index");
        let write_start = Instant::now();
        write_progress.init(
            Some(cf.planned_storage_size() as usize + Self::HEADER_LEN),
            git_features::progress::bytes(),
        );
        let mut out = git_features::progress::Write {
            inner: out,
            progress: write_progress,
        };

        let bytes_written = Self::write_header(
            &mut out,
            cf.num_chunks().try_into().expect("BUG: wrote more than 256 chunks"),
            index_paths_sorted.len() as u32,
            object_hash,
        )?;

        {
            progress.set_name("Writing chunks");
            progress.init(Some(cf.num_chunks()), git_features::progress::count("chunks"));

            let mut chunk_write = cf.into_write(&mut out, bytes_written)?;
            while let Some(chunk_to_write) = chunk_write.next_chunk() {
                match chunk_to_write {
                    multi_index::chunk::index_names::ID => {
                        multi_index::chunk::index_names::write(&index_filenames_sorted, &mut chunk_write)?
                    }
                    multi_index::chunk::fanout::ID => multi_index::chunk::fanout::write(&entries, &mut chunk_write)?,
                    multi_index::chunk::lookup::ID => multi_index::chunk::lookup::write(&entries, &mut chunk_write)?,
                    multi_index::chunk::offsets::ID => {
                        multi_index::chunk::offsets::write(&entries, num_large_offsets.is_some(), &mut chunk_write)?
                    }
                    multi_index::chunk::large_offsets::ID => multi_index::chunk::large_offsets::write(
                        &entries,
                        num_large_offsets.expect("available if planned"),
                        &mut chunk_write,
                    )?,
                    unknown => unreachable!("BUG: forgot to implement chunk {:?}", std::str::from_utf8(&unknown)),
                }
                progress.inc();
                if should_interrupt.load(Ordering::Relaxed) {
                    return Err(Error::Interrupted);
                }
            }
        }

        // write trailing checksum
        let multi_index_checksum: git_hash::ObjectId = out.inner.hash.digest().into();
        out.inner.inner.write_all(multi_index_checksum.as_slice())?;
        out.progress.show_throughput(write_start);

        Ok(Outcome {
            multi_index_checksum,
            progress,
        })
    }

    fn write_header(
        mut out: impl std::io::Write,
        num_chunks: u8,
        num_indices: u32,
        object_hash: git_hash::Kind,
    ) -> std::io::Result<usize> {
        out.write_all(Self::SIGNATURE)?;
        out.write_all(&[crate::multi_index::Version::V1 as u8])?;
        out.write_all(&[object_hash as u8])?;
        out.write_all(&[num_chunks])?;
        out.write_all(&[0])?; /* unused number of base files */
        out.write_u32::<BigEndian>(num_indices)?;

        Ok(Self::HEADER_LEN)
    }
}