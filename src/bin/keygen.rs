use sha2::{Sha256, Digest};
use std::io::{stdin, stdout, Write};

fn main() {
    println!("=== AUDIOTX PRO - GENERATORE CHIAVI LICENZA ===");
    
    print!("Inserisci HWID del target: ");
    stdout().flush().ok();
    let mut hwid = String::new();
    stdin().read_line(&mut hwid).ok();
    let hwid = hwid.trim().to_uppercase();

    print!("Inserisci Nome Cliente   : ");
    stdout().flush().ok();
    let mut name = String::new();
    stdin().read_line(&mut name).ok();
    let name = name.trim().to_string();

    // Stessa identica logica e chiave segreta usata nel main.rs
    let mut hasher = Sha256::new();
    hasher.update(hwid.as_bytes());
    hasher.update(name.as_bytes());
    hasher.update(b"MmcyS5isfQKEdMPnn3F6N1n4tFLbtjPjPvip6spNjg");
    
    let key = format!("{:x}", hasher.finalize())[..16].to_uppercase();

    println!("\n-------------------------------------------");
    println!("CLIENTE    : {}", name);
    println!("HWID TARGET: {}", hwid);
    println!("CHIAVE     : {}", key);
    println!("-------------------------------------------\n");
}
