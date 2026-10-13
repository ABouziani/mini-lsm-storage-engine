
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

// ============================================================
// ENTRY
// ============================================================

#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    Value(u64),
    Tombstone,
}

// ============================================================
// MEMTABLE
// ============================================================

struct MemTable {
    entries: BTreeMap<String, Entry>,
}

impl MemTable {
    fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    fn put(&mut self, key: String, value: u64) {
        self.entries.insert(key, Entry::Value(value));
    }

    fn delete(&mut self, key: String) {
        self.entries.insert(key, Entry::Tombstone);
    }

    fn get(&self, key: &str) -> Option<&Entry> {
        self.entries.get(key)
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ============================================================
// SSTABLE
// ============================================================

struct SSTable {
    id: u64,
    path: PathBuf,
    entries: Vec<(String, Entry)>,
}

impl SSTable {
    fn from_memtable(
        id: u64,
        directory: &Path,
        memtable: &MemTable,
    ) -> io::Result<Self> {
        let entries = memtable
            .entries
            .iter()
            .map(|(key, entry)| (key.clone(), entry.clone()))
            .collect();

        Self::write(id, directory, entries)
    }

    fn write(
        id: u64,
        directory: &Path,
        entries: Vec<(String, Entry)>,
    ) -> io::Result<Self> {
        let path = directory.join(format!("table-{id}.sst"));
        let temp_path = directory.join(format!("table-{id}.tmp"));

        {
            let mut file = File::create(&temp_path)?;

            for (key, entry) in &entries {
                writeln!(file, "{}", encode_entry(key, entry))?;
            }

            file.sync_all()?;
        }

        fs::rename(&temp_path, &path)?;

        Ok(Self { id, path, entries })
    }

    fn load(path: PathBuf, id: u64) -> io::Result<Self> {
        let file = File::open(&path)?;
        let reader = BufReader::new(file);
        let mut entries = Vec::new();

        for line in reader.lines() {
            entries.push(decode_entry(&line?)?);
        }

        // SSTables must be sorted for binary search.
        if entries.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "SSTable keys are not strictly sorted",
            ));
        }

        Ok(Self { id, path, entries })
    }

    fn get(&self, key: &str) -> Option<&Entry> {
        match self
            .entries
            .binary_search_by(|(stored_key, _)| stored_key.as_str().cmp(key))
        {
            Ok(index) => Some(&self.entries[index].1),
            Err(_) => None,
        }
    }
}

// ============================================================
// LSM DATABASE
// ============================================================

pub struct Lsm {
    directory: PathBuf,
    wal_path: PathBuf,
    memtable: MemTable,
    sstables: Vec<SSTable>, // Newest first
    capacity: usize,
    next_id: u64,
}

impl Lsm {
    // Open an existing database or create a new one.
    pub fn open(
        path: impl AsRef<Path>,
        capacity: usize,
    ) -> io::Result<Self> {
        assert!(capacity > 0, "Capacity must be greater than zero");

        let directory = path.as_ref().to_path_buf();
        fs::create_dir_all(&directory)?;

        let wal_path = directory.join("wal.log");
        let mut sstables = Vec::new();
        let mut next_id = 0;

        // Load all SSTables from disk.
        for item in fs::read_dir(&directory)? {
            let item = item?;
            let path = item.path();

            if path.extension().and_then(|s| s.to_str()) != Some("sst") {
                continue;
            }

            let Some(filename) = path.file_stem().and_then(|s| s.to_str())
            else {
                continue;
            };

            let Some(id_string) = filename.strip_prefix("table-") else {
                continue;
            };

            let Ok(id) = id_string.parse::<u64>() else {
                continue;
            };

            let table = SSTable::load(path, id)?;
            next_id = next_id.max(id + 1);
            sstables.push(table);
        }

        // Higher table IDs are newer.
        sstables.sort_by(|a, b| b.id.cmp(&a.id));

        let mut db = Self {
            directory,
            wal_path,
            memtable: MemTable::new(),
            sstables,
            capacity,
            next_id,
        };

        // Restore writes not yet safely represented by SSTables.
        db.recover_wal()?;

        if db.memtable.len() >= db.capacity {
            db.flush()?;
        }

        Ok(db)
    }

    // --------------------------------------------------------
    // WAL
    // --------------------------------------------------------

    fn append_wal(&self, key: &str, entry: &Entry) -> io::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.wal_path)?;

        writeln!(file, "{}", encode_entry(key, entry))?;
        file.sync_data()
    }

    fn recover_wal(&mut self) -> io::Result<()> {
        if !self.wal_path.exists() {
            return Ok(());
        }

        let file = File::open(&self.wal_path)?;
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let (key, entry) = decode_entry(&line?)?;

            match entry {
                Entry::Value(value) => self.memtable.put(key, value),
                Entry::Tombstone => self.memtable.delete(key),
            }
        }

        Ok(())
    }

    fn clear_wal(&self) -> io::Result<()> {
        let file = File::create(&self.wal_path)?;
        file.sync_all()
    }

    // --------------------------------------------------------
    // PUT
    // --------------------------------------------------------

    pub fn put(&mut self, key: String, value: u64) -> io::Result<()> {
        Self::validate_key(&key)?;

        let entry = Entry::Value(value);

        // Durability first: record the operation in the WAL.
        self.append_wal(&key, &entry)?;

        // Then update the in-memory view.
        self.memtable.put(key, value);

        self.flush_if_full()
    }

    // --------------------------------------------------------
    // DELETE
    // --------------------------------------------------------

    pub fn delete(&mut self, key: String) -> io::Result<()> {
        Self::validate_key(&key)?;

        let entry = Entry::Tombstone;

        self.append_wal(&key, &entry)?;
        self.memtable.delete(key);

        self.flush_if_full()
    }

    fn validate_key(key: &str) -> io::Result<()> {
        if key.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Keys must not be empty",
            ));
        }

        Ok(())
    }

    // --------------------------------------------------------
    // GET
    // --------------------------------------------------------

    pub fn get(&self, key: &str) -> Option<u64> {
        // 1. Check the newest in-memory data.
        match self.memtable.get(key) {
            Some(Entry::Value(value)) => return Some(*value),
            Some(Entry::Tombstone) => return None,
            None => {}
        }

        // 2. Search SSTables newest to oldest.
        for table in &self.sstables {
            match table.get(key) {
                Some(Entry::Value(value)) => return Some(*value),
                Some(Entry::Tombstone) => return None,
                None => {}
            }
        }

        None
    }

    // --------------------------------------------------------
    // FLUSH
    // --------------------------------------------------------

    fn flush_if_full(&mut self) -> io::Result<()> {
        if self.memtable.len() >= self.capacity {
            self.flush()?;
        }

        Ok(())
    }

    pub fn flush(&mut self) -> io::Result<()> {
        if self.memtable.is_empty() {
            return Ok(());
        }

        let table = SSTable::from_memtable(
            self.next_id,
            &self.directory,
            &self.memtable,
        )?;

        self.next_id += 1;

        // Publish the new table in memory.
        self.sstables.insert(0, table);
        self.memtable = MemTable::new();

        // Clear the WAL only after the table has been written.
        self.clear_wal()?;

        Ok(())
    }

    // --------------------------------------------------------
    // COMPACTION
    // --------------------------------------------------------

    pub fn compact(&mut self) -> io::Result<()> {
        // Persist pending writes first.
        self.flush()?;

        if self.sstables.len() < 2 {
            return Ok(());
        }

        let mut merged = BTreeMap::new();

        // Process oldest first, newest last.
        // Newer values overwrite older versions.
        for table in self.sstables.iter().rev() {
            for (key, entry) in &table.entries {
                merged.insert(key.clone(), entry.clone());
            }
        }

        // Keep tombstones: this implementation doesn't reclaim
        // deleted keys because it has no range/level metadata.
        let entries: Vec<(String, Entry)> = merged.into_iter().collect();

        // Persist the merged table before deleting the old tables.
        let compacted =
            SSTable::write(self.next_id, &self.directory, entries)?;

        self.next_id += 1;

        for table in &self.sstables {
            fs::remove_file(&table.path)?;
        }

        self.sstables = vec![compacted];

        Ok(())
    }

    // --------------------------------------------------------
    // DIAGNOSTICS
    // --------------------------------------------------------

    pub fn print_state(&self) {
        println!("MemTable entries: {}", self.memtable.len());

        for (key, entry) in &self.memtable.entries {
            println!("  {key} => {entry:?}");
        }

        println!("SSTable count: {}", self.sstables.len());

        for table in &self.sstables {
            println!("  SSTable {}:", table.id);

            for (key, entry) in &table.entries {
                println!("    {key} => {entry:?}");
            }
        }
    }
}

// ============================================================
// ENCODING
// ============================================================

// Hex encoding allows keys to contain spaces and Unicode.
fn encode_key(key: &str) -> String {
    key.as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn decode_key(hex: &str) -> io::Result<String> {
    if hex.len() % 2 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid hex key",
        ));
    }

    let mut bytes = Vec::new();

    for pair in hex.as_bytes().chunks_exact(2) {
        let pair = std::str::from_utf8(pair)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let byte = u8::from_str_radix(pair, 16)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        bytes.push(byte);
    }

    String::from_utf8(bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn encode_entry(key: &str, entry: &Entry) -> String {
    match entry {
        Entry::Value(value) => {
            format!("P {} {}", encode_key(key), value)
        }
        Entry::Tombstone => format!("D {}", encode_key(key)),
    }
}

fn decode_entry(line: &str) -> io::Result<(String, Entry)> {
    let mut parts = line.split_whitespace();

    let kind = parts.next().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "Missing entry type")
    })?;

    let encoded_key = parts.next().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "Missing key")
    })?;

    let key = decode_key(encoded_key)?;

    let entry = match kind {
        "P" => {
            let value = parts
                .next()
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Missing value",
                    )
                })?
                .parse::<u64>()
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

            Entry::Value(value)
        }
        "D" => Entry::Tombstone,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unknown entry type",
            ));
        }
    };

    if parts.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Unexpected extra fields",
        ));
    }

    Ok((key, entry))
}
