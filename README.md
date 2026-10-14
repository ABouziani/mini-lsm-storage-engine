# Mini LSM-Tree Database

A lightweight, disk-backed key-value storage engine implemented in Rust to explore the core design principles behind Log-Structured Merge-Tree (LSM) databases.

The project focuses on storage-engine fundamentals, including write-ahead logging, sorted string tables, automatic flushing, tombstones, compaction, and recovery after interrupted writes.

## Features

- **MemTable:** Maintains the latest in-memory state using a sorted `BTreeMap`.
- **Write-Ahead Log (WAL):** Records mutations before applying them to memory to support recovery after process restarts.
- **Automatic flushing:** Converts the MemTable into an immutable SSTable when its capacity is reached.
- **SSTables:** Stores sorted key-value entries on disk and uses binary search for lookups.
- **Tombstones:** Represents deletions explicitly to prevent older values from reappearing.
- **Compaction:** Merges SSTables, retains the newest version of each key, and reduces the number of tables searched during reads.
- **Checksummed records:** Validates persisted records to detect corruption.
- **Crash-recovery mechanisms:** Handles incomplete trailing WAL records and uses synchronized writes and atomic file replacement where supported.
- **Integration tests:** Covers basic operations, automatic flushing, compaction, deletion, and recovery after reopening the database.

## Architecture

### Write path

```text
       Client Write
            |
            v
       Append to WAL
            |
            v
         MemTable
            |
      Capacity Reached?
            |
            v
     Flush to SSTable
            |
            v
       Clear WAL
```

### Read path

```text
        Read(key)
            |
            v
        MemTable
            |
      Key Found?
       /       \
     Yes        No
      |          |
      v          v
    Return    Newest SSTable
                  |
                  v
           Older SSTables
                  |
                  v
            Return Value
```

The MemTable is checked first, followed by SSTables from newest to oldest. A tombstone terminates the search and returns `None`.

### Compaction

Compaction merges existing SSTables into a single sorted table. When multiple versions of a key exist, the newest entry wins.

This implementation retains tombstones to avoid resurrecting deleted keys. Safely removing tombstones requires additional guarantees about compaction coverage and older data.

## Project Structure

```text
mini-lsm/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   └── main.rs
└── tests/
    └── unit-test.rs
```

| Component | Responsibility |
|---|---|
| `src/lib.rs` | Storage engine, WAL, MemTable, SSTables, recovery, and compaction |
| `src/main.rs` | Runnable demonstration of database operations |
| `tests/unit-test.rs` | Integration tests for persistence and storage semantics |

## Getting Started

### Prerequisites

- Rust toolchain
- Cargo

Install Rust using [rustup](https://rustup.rs/) if needed.

### Clone the repository

```bash
git clone https://github.com/ABouziani/mini-lsm.git
cd mini-lsm
```

Replace the repository URL if you choose a different GitHub repository name.

### Run the demonstration

```bash
cargo run
```

### Run the integration tests

```bash
cargo test --test unit-test
```

Run the complete test suite:

```bash
cargo test
```

Format the code:

```bash
cargo fmt
```

## Example

```rust
use mini_lsm::Lsm;

fn main() -> std::io::Result<()> {
    let mut db = Lsm::open("./data", 3)?;

    db.put("account:1".into(), 100)?;
    db.put("account:2".into(), 250)?;

    println!("{:?}", db.get("account:1"));
    // Some(100)

    db.put("account:1".into(), 150)?;
    println!("{:?}", db.get("account:1"));
    // Some(150)

    db.delete("account:2".into())?;
    println!("{:?}", db.get("account:2"));
    // None

    db.flush()?;
    db.compact()?;

    Ok(())
}
```

## Storage Format

The database stores its data in a directory containing:

- `wal.log` — append-only log of mutations.
- `table-<id>.sst` — immutable sorted tables.
- Temporary files used during SSTable creation and WAL replacement.

Records are encoded as text with hexadecimal keys and checksums. This makes the storage format easy to inspect during development, although a production engine would generally use a more compact binary format.

## Crash Recovery

The implementation explores several techniques used in durable storage systems:

- Appending and synchronizing WAL records before updating in-memory state.
- Synchronizing SSTable contents before publishing a completed table.
- Replaying valid, complete WAL records when reopening the database.
- Ignoring and truncating an incomplete final WAL record.
- Detecting corrupted complete records using checksums.
- Persisting the new compacted SSTable before removing older files.
- Synchronizing directory changes where supported.

These mechanisms are educational rather than a complete crash-consistency protocol. In particular, the implementation does not yet use a durable manifest, checksummed binary WAL framing, or exhaustive recovery logic for every possible interruption during compaction and WAL replacement.

## Current Limitations

This project prioritizes understanding the fundamentals over production performance.

- **Single-process, single-threaded:** No concurrent writers or readers.
- **In-memory SSTable indexes:** SSTable contents are loaded into memory on startup.
- **Simplified WAL format:** No log segmentation or checksummed binary framing.
- **Basic compaction:** All SSTables are merged together rather than using leveled or tiered strategies.
- **No Bloom filters:** Reads do not yet use probabilistic membership checks to skip unnecessary table searches.
- **No block cache:** Data is not organized into independently cached disk blocks.
- **No durable manifest:** Recovery discovers SSTables by scanning the database directory.
- **Limited crash guarantees:** Filesystem and hardware behavior can affect durability; the implementation is not intended for production workloads.

## Potential Improvements

- Implement a durable manifest for tracking SSTable creation, deletion, and compaction.
- Add a block-based SSTable format with sparse indexes.
- Implement Bloom filters to reduce unnecessary disk reads.
- Add WAL segmentation and checkpoints.
- Introduce a block cache.
- Explore leveled compaction and write-stall management.
- Add benchmarks for write throughput, read latency, and compaction overhead.
- Extend the storage model toward blockchain state storage and state-cache workloads.

## Learning Objectives

This project provides hands-on experience with:

- Rust ownership and data modeling.
- Ordered maps and binary search.
- Persistent storage and file I/O.
- Write-ahead logging and recovery.
- Immutable storage structures.
- Tombstones and version resolution.
- Compaction and storage amplification.
- Integration testing of persistent systems.

## License

This project is released under the MIT License. See [LICENSE](LICENSE) for details.
