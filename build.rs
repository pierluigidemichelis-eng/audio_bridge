fn main() {
    // Applichiamo le risorse solo quando il target è Windows
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("windows") {
        let mut res = winres::WindowsResource::new();
        
        // Assicurati che icona.ico sia nella stessa cartella di Cargo.toml
        res.set_icon("icona.ico");
        
        // Path del compilatore di risorse per la cross-compilazione su Linux Mint
        res.set_windres_path("/usr/bin/x86_64-w64-mingw32-windres");
        
        // Compila le risorse nell'eseguibile
        if let Err(e) = res.compile() {
            eprintln!("Errore compilazione risorse Windows: {}", e);
            std::process::exit(1);
        }
    }
}

