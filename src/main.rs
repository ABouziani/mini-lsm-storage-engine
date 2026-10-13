
use mini_lsm::Lsm;
use std::fs;
use std::io;

fn main() -> io::Result<()> {
    let directory = std::env::temp_dir()
        .join(format!("mini-lsm-demo-{}", std::process::id()));

    // Start with a clean directory for this demonstration.
    if directory.exists() {
        fs::remove_dir_all(&directory)?;
    }

    let mut db = Lsm::open(&directory, 3)?;

    println!("=== 1. Automatic flushing ===");

    db.put("A".into(), 10)?;
    db.put("B".into(), 20)?;

    println!("After two writes:");
    db.print_state();

    db.put("C".into(), 30)?;

    println!("\nAfter the third write:");
    db.print_state();

    println!("\n=== 2. Updating and adding keys ===");

    db.put("B".into(), 25)?;
    db.put("D".into(), 40)?;
    db.put("E".into(), 50)?;

    db.print_state();

    println!("\n=== 3. Lookups before deletion ===");

    for key in ["A", "B", "C", "D", "E", "Z"] {
        println!("{key} => {:?}", db.get(key));
    }

    println!("\n=== 4. Delete B ===");

    db.delete("B".into())?;
    println!("B => {:?}", db.get("B"));

    println!("\n=== 5. Compact SSTables ===");

    db.flush()?;
    db.compact()?;
    db.print_state();

    println!("\n=== 6. Reopen database ===");

    drop(db);

    let db = Lsm::open(&directory, 3)?;

    for key in ["A", "B", "C", "D", "E", "Z"] {
        println!("{key} => {:?}", db.get(key));
    }

    println!("\nDatabase directory: {}", directory.display());

    Ok(())
}
