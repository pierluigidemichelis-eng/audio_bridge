#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod gui;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::HeapRb;
use std::net::UdpSocket;
use std::sync::{Arc, Mutex, mpsc};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use std::collections::BTreeMap;
use sha2::{Sha256, Digest};
use gui::{AudioCommand, Tab, Config};
#[cfg(feature = "gui")]
use gui::AudioApp;

// --- SEZIONE PC: IMPORT REALE ---
#[cfg(not(target_os = "android"))]
use opus::{Encoder, Decoder, Application, Channels, Bitrate};

// --- SEZIONE ANDROID: STRUTTURA FITTIZIA PER FAR COMPILARE SENZA ERRORI ---
#[cfg(target_os = "android")]
pub struct Encoder;
#[cfg(target_os = "android")]
pub struct Decoder;

#[cfg(target_os = "android")]
pub enum Application { Audio }
#[cfg(target_os = "android")]
pub enum Channels { Stereo }
#[cfg(target_os = "android")]
pub enum Bitrate { Bits(i32) }

#[cfg(target_os = "android")]
impl Encoder {
    pub fn new(_: i32, _: Channels, _: Application) -> Result<Self, ()> { Ok(Encoder) }
    pub fn set_bitrate(&mut self, _: Bitrate) -> Result<(), ()> { Ok(()) }
    pub fn encode_float(&mut self, _: &[f32], _: &mut [u8]) -> Result<usize, ()> { Ok(0) }
}

#[cfg(target_os = "android")]
impl Decoder {
    pub fn new(_: i32, _: Channels) -> Result<Self, ()> { Ok(Decoder) }
    pub fn decode_float(&mut self, _: &[u8], _: &mut [f32], _: bool) -> Result<usize, ()> { Ok(0) }
}

// --- SILENZIAMENTO ERRORI ALSA / JACK / OSS UNIVERSALE ---
#[cfg(not(target_os = "android"))]
fn disattiva_errori_alsa() {
    use std::os::raw::{c_char, c_int, c_void};

    unsafe extern "C" fn alsa_error_handler(
        _file: *const c_char,
        _line: c_int,
        _function: *const c_char,
        _err: c_int,
        _fmt: *const c_char,
    ) {}

    extern "C" {
        fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        // Funzioni primitive del kernel per manipolare i descrittori di file
        fn open(pathname: *const c_char, flags: c_int) -> c_int;
        fn dup2(oldfd: c_int, newfd: c_int) -> c_int;
    }

    const RTLD_LAZY: c_int = 1;
    const O_WRONLY: c_int = 1;

    unsafe {
        // 1. Zittisce ALSA caricando dinamicamente il modulo
        let mut lib_name = std::ffi::CString::new("libasound.so.2").unwrap();
        let mut handle = dlopen(lib_name.as_ptr(), RTLD_LAZY);
        if handle.is_null() {
            lib_name = std::ffi::CString::new("libasound.so").unwrap();
            handle = dlopen(lib_name.as_ptr(), RTLD_LAZY);
        }
        if !handle.is_null() {
            let symbol_name = std::ffi::CString::new("snd_lib_error_set_handler").unwrap();
            let symbol = dlsym(handle, symbol_name.as_ptr());
            if !symbol.is_null() {
                let snd_lib_error_set_handler: unsafe extern "C" fn(
                    Option<unsafe extern "C" fn(*const c_char, c_int, *const c_char, c_int, *const c_char)>
                ) -> c_int = std::mem::transmute(symbol);
                snd_lib_error_set_handler(Some(alsa_error_handler));
            }
        }

        // 2. Zittisce JACK e OSS reindirizzando lo stderr (fd 2) verso /dev/null
        let dev_null = std::ffi::CString::new("/dev/null").unwrap();
        let null_fd = open(dev_null.as_ptr(), O_WRONLY);
        if null_fd >= 0 {
            dup2(null_fd, 2); // Sovrascrive lo stderr di basso livello
        }
    }
}


fn pack_bin(seq: u64, ctrl: u8, data: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(9 + data.len());
    v.extend_from_slice(&seq.to_be_bytes()); v.push(ctrl); v.extend_from_slice(data); v
}

fn unpack_bin(buf: &[u8]) -> Option<(u64, u8, Vec<u8>)> {
    if buf.len() < 9 { return None; }
    let seq = u64::from_be_bytes(buf[0..8].try_into().ok()?);
    let ctrl = buf[8]; let data = buf[9..].to_vec();
    Some((seq, ctrl, data))
}

fn generate_hwid() -> String {
    #[cfg(not(target_os = "android"))]
    {
        let uid = machine_uid::get().unwrap_or_else(|_| "UNKNOWN".into());
        let mut h = Sha256::new(); h.update(uid.as_bytes());
        format!("{:x}", h.finalize())[..12].to_uppercase()
    }
    
    #[cfg(target_os = "android")]
    {
        "ANDROID_HWID".to_string()
    }
}

fn verify_license_static(hwid: &str, name: &str, key: &str) -> bool {
    if name.is_empty() || key.is_empty() { return false; }
    let mut h = Sha256::new();
    h.update(hwid.trim().to_uppercase().as_bytes()); h.update(name.trim().as_bytes());
    h.update(b"MmcyS5isfQKEdMPnn3F6N1n4tFLbtjPjPvip6spNjg");
    let expected = format!("{:x}", h.finalize())[..16].to_uppercase();
    key.trim() == expected
}

fn wizard(h_id: &str) -> Config {
    println!("\n=== CONFIGURAZIONE AUDIOTX PRO ===\nHWID: {}", h_id);
    let mut cfg = Config { 
        target_ip: "127.0.0.1:12345".into(),
        local_port: 12345,
        buffer_size_ms: 60,
        selected_tab: Tab::TX,
        in_volume: 1.0,
        out_volume: 1.0,
        selected_in: "Default".into(),
        selected_out: "Default".into(),
        bitrate_kbps: 96,
        customer_name: "Default User".into(),
        license_key: "".into(),
        mode: "BOTH".into(),
        autostart: false,
        vpn_enabled: false,
        vpn_private_key: "".into(),
        vpn_peer_public_key: "".into(),
        vpn_endpoint: "".into(),
        vpn_local_ip: "10.0.0.2".into(),
        vpn_allowed_ips: "10.0.0.0/24".into(), 
    };
    use std::io::{stdin, stdout, Write};
    print!("Nome Cliente: "); stdout().flush().ok();
    let mut n = String::new(); stdin().read_line(&mut n).ok(); cfg.customer_name = n.trim().to_string();
    print!("Chiave Licenza: "); stdout().flush().ok();
    let mut k = String::new(); stdin().read_line(&mut k).ok(); cfg.license_key = k.trim().to_string();
    let _ = std::fs::write("config.json", serde_json::to_string_pretty(&cfg).unwrap());
    cfg
}
fn start_audio_engine(
    mut cfg: Config, 
    cmd_rx: mpsc::Receiver<AudioCommand>, 
    jitter_tx: mpsc::Sender<f64>,
    input_lvls: [Arc<AtomicU32>; 2], 
    output_lvls: [Arc<AtomicU32>; 2],
    tk: Arc<AtomicU32>, 
    rk: Arc<AtomicU32>, 
    vi: Arc<AtomicU32>, 
    vo: Arc<AtomicU32>,
    ra: Arc<Mutex<String>>, 
    lp: Arc<Mutex<Instant>>,
    vpn_status: Arc<std::sync::atomic::AtomicU32>,
    vpn_log_buffer: Arc<std::sync::Mutex<Vec<String>>>,
) {
    std::thread::spawn(move || {
        let host = cpal::default_host();
        let log_vpn = |msg: &str| {
            if let Ok(mut buf) = vpn_log_buffer.lock() {
                buf.push(format!("{}", msg));
                if buf.len() > 100 { buf.remove(0); }
            }
        };
        use std::net::ToSocketAddrs;
        let endpoint_risolto_numerico = match cfg.vpn_endpoint.to_socket_addrs() {
            Ok(mut addrs) => addrs.next(),
            Err(_) => None,
        };

        let socket = UdpSocket::bind(format!("0.0.0.0:{}", cfg.local_port)).expect("Porta occupata");
        socket.set_nonblocking(true).ok();

        if cfg.vpn_enabled {
            if let Some(ip_puro) = endpoint_risolto_numerico {
                if let Err(e) = socket.connect(ip_puro) {
                    log_vpn(&format!("⚠️ Errore binding hardware socket: {:?}", e));
                } else {
                    log_vpn(&format!("🔒 Socket simmetrico ancorato correttamente su IP: {}", ip_puro));
                }
            } else {
                log_vpn("❌ Impossibile avviare VPN: Risoluzione DNS dell'endpoint fallita.");
                vpn_status.store(3, std::sync::atomic::Ordering::Relaxed);
            }
        }

        let socket_sender = socket.try_clone().expect("Impossibile clonare il socket hardware");
        let current_socket = Arc::new(Mutex::new(socket));


        // --- RIGENERAZIONE WIREGUARD USERSPACE API 0.6 CON HANDSHAKE RIGIDO ---
        let mut wg_tunnel = if cfg.vpn_enabled {
            use base64::{Engine as _, engine::general_purpose};
            
            let decode_base64_bytes = |k: &str| -> Option<[u8; 32]> {
                let dec = general_purpose::STANDARD.decode(k.trim()).ok()?;
                if dec.len() == 32 {
                    let mut buf = [0u8; 32];
                    buf.copy_from_slice(&dec);
                    Some(buf)
                } else { None }
            };

            let priv_bytes = decode_base64_bytes(&cfg.vpn_private_key);
            let pub_bytes = decode_base64_bytes(&cfg.vpn_peer_public_key);
            
            if let (Some(p_bytes), Some(s_bytes)) = (priv_bytes, pub_bytes) {
				log_vpn("🔒 Inizializzazione modulo VPN WireGuard...");
				vpn_status.store(1, std::sync::atomic::Ordering::Relaxed); // Giallo: Collegamento in corso
				
				let priv_k: boringtun::x25519::StaticSecret = p_bytes.into();
				let pub_k: boringtun::x25519::PublicKey = s_bytes.into();

				let mut tunnel = boringtun::noise::Tunn::new(priv_k, pub_k, None, None, 1, None).expect("Reset fallito");
				let subnet_gateway = std::net::Ipv4Addr::new(192, 168, 11, 0);

				// --- NOTA: IL BLOCCO ALLOWED IP ORFANO È STATO COMPLETAMENTE RIMOSSO DA QUI ---
				log_vpn(&format!("🔒 [VPN] Registro rotta dinamica di ascolto: {}", cfg.vpn_allowed_ips));

				let mut initial_handshake_buf = [0u8; 1420];
				if let boringtun::noise::TunnResult::WriteToNetwork(handshake_data) = tunnel.update_timers(&mut initial_handshake_buf) {
					if let Ok(sock) = current_socket.lock() {
						let _ = sock.send_to(handshake_data, &cfg.vpn_endpoint);
						log_vpn("🚀 [VPN] Primo pacchetto di Handshake inviato al MikroTik.");
					}
				}
				Some(tunnel)

			} else {
				log_vpn("⚠️ Errore crittografico: Chiavi non conformi.");
				vpn_status.store(3, std::sync::atomic::Ordering::Relaxed);
				None
			}
        } else {
            vpn_status.store(0, std::sync::atomic::Ordering::Relaxed);
            None
        };

        let host_hwid = generate_hwid();
        let mut is_licensed_active = verify_license_static(&host_hwid, &cfg.customer_name, &cfg.license_key);
        let (mut target, mut delay_pkts, mut active) = (cfg.target_ip.clone(), (cfg.buffer_size_ms / 20) as usize, cfg.autostart);

        let current_prod_in = Arc::new(Mutex::new(None));
        let current_cons_in = Arc::new(Mutex::new(None));
        let current_prod_out = Arc::new(Mutex::new(None));
        let current_cons_out = Arc::new(Mutex::new(None));

        let mut _s_in: Option<cpal::Stream> = None;
        let mut _s_out: Option<cpal::Stream> = None;

        let mut enc = Encoder::new(48000, Channels::Stereo, Application::Audio).unwrap();
        let mut dec = Decoder::new(48000, Channels::Stereo).unwrap();

        let (mut seq_tx, mut j_map, mut last_bw, mut last_wg_tick) = (0u64, BTreeMap::new(), Instant::now(), Instant::now());
        let (mut b_tx, mut b_rx) = (0, 0);
        let (mut pin, mut pout) = (Some(cfg.selected_in.clone()), Some(cfg.selected_out.clone()));
		let porta_destinazione_dinamica: u16 = cfg.target_ip
			.split(':')
			.nth(1)
			.and_then(|p| p.parse::<u16>().ok())
			.unwrap_or(12345);


        loop {
            // Se l'utente ha la VPN abilitata e non siamo in errore, forziamo lo stato "Tenta Connessione"
            if cfg.vpn_enabled && wg_tunnel.is_some() {
                if vpn_status.load(std::sync::atomic::Ordering::Relaxed) == 0 {
                    vpn_status.store(1, std::sync::atomic::Ordering::Relaxed);
                }
            }

            while let Ok(cmd) = cmd_rx.try_recv() {
                match cmd {
                    AudioCommand::SetTransmitting(s) => {
                        if !s && active {
                            let last_pkt = pack_bin(0, 1, &[]);
                            if let Ok(sock) = current_socket.lock() {
                                if cfg.vpn_enabled && wg_tunnel.is_some() {
                                    let mut wg_buf = [0u8; 2048];
                                    if let Some(ref mut tunnel) = wg_tunnel {
                                        if let boringtun::noise::TunnResult::WriteToNetwork(wg_data) = tunnel.encapsulate(&last_pkt, &mut wg_buf) {
                                            sock.send_to(wg_data, &cfg.vpn_endpoint).ok();
                                        }
                                    }
                                } else {
                                    sock.send_to(&last_pkt, &target).ok();
                                }
                            }
                            j_map.clear();
                        }
                        active = s;
                    },
                    AudioCommand::UpdateTarget(t) => target = t,
                    AudioCommand::UpdateBitrate(b) => { let _ = enc.set_bitrate(Bitrate::Bits(b as i32 * 1024)); },
                    AudioCommand::UpdateBufferSize(ms) => { delay_pkts = (ms / 20) as usize; j_map.clear(); },
                    AudioCommand::UpdateInputDevice(n) => pin = Some(n),
                    AudioCommand::UpdateOutputDevice(n) => pout = Some(n),
                    AudioCommand::UpdateAuth(s) => is_licensed_active = s,
                    AudioCommand::UpdateLocalPort(p) => {
                        if let Ok(nuovo_socket) = UdpSocket::bind(format!("0.0.0.0:{}", p)) {
                            nuovo_socket.set_nonblocking(true).ok();
                            if let Ok(mut sock_guard) = current_socket.lock() { *sock_guard = nuovo_socket; }
                        }
                    },
                    
                    AudioCommand::UpdateVpnEnabled(stato) => {
                        cfg.vpn_enabled = stato;
                        if !stato {
                            wg_tunnel = None;
                            vpn_status.store(0, std::sync::atomic::Ordering::Relaxed); // LED GRIGIO
                            if let Ok(mut buf) = vpn_log_buffer.lock() {
                                buf.clear();
                                buf.push("⚪ Interfaccia VPN disattivata dall'utente. Tunnel rimosso.".to_string());
                            }
                        } else {
                            // Cambia IMMEDIATAMENTE la spia a GIALLO per confermare la ricezione del comando
                            vpn_status.store(1, std::sync::atomic::Ordering::Relaxed); 
                            
                            if let Ok(mut buf) = vpn_log_buffer.lock() {
                                buf.push("🔄 [VPN] Thread risvegliato. Caricamento identità crittografica...".to_string());
                            }

                            // Rileggiamo in sicurezza config.json per catturare le stringhe digitate nella GUI
                            if let Ok(file_content) = std::fs::read_to_string("config.json") {
                                if let Ok(cfg_aggiornata) = serde_json::from_str::<Config>(&file_content) {
                                    cfg.vpn_private_key = cfg_aggiornata.vpn_private_key;
                                    cfg.vpn_peer_public_key = cfg_aggiornata.vpn_peer_public_key;
                                    cfg.vpn_endpoint = cfg_aggiornata.vpn_endpoint;
                                    cfg.vpn_local_ip = cfg_aggiornata.vpn_local_ip;
                                    cfg.vpn_allowed_ips = cfg_aggiornata.vpn_allowed_ips;
                                }
                            }

                            use base64::{Engine as _, engine::general_purpose};
                            let decode_bytes = |k: &str| -> Option<[u8; 32]> {
                                let dec = general_purpose::STANDARD.decode(k.trim()).ok()?;
                                if dec.len() == 32 {
                                    let mut b = [0u8; 32]; b.copy_from_slice(&dec); Some(b)
                                } else { None }
                            };
							let priv_bytes = decode_bytes(&cfg.vpn_private_key);
                            let pub_bytes = decode_bytes(&cfg.vpn_peer_public_key); // <-- AGGIUNTO PUNTO E VIRGOLA FISSO

                            if let (Some(p_bytes), Some(s_bytes)) = (priv_bytes, pub_bytes) {
                                let priv_k: boringtun::x25519::StaticSecret = p_bytes.into();
                                let pub_k: boringtun::x25519::PublicKey = s_bytes.into();

                                let index_casuale = rand::Rng::gen_range(&mut rand::thread_rng(), 1000..16777215);                                
                                let mut tunnel = boringtun::noise::Tunn::new(priv_k, pub_k, None, None, index_casuale, None).expect("Reset fallito");
                                
                                if let Ok(mut buf) = vpn_log_buffer.lock() {
                                    buf.push(format!("🔒 [VPN] Controllo e validazione rotte operative: {}", cfg.vpn_allowed_ips));
                                }

                                let mut initial_handshake_buf = [0u8; 1420];
                                if let boringtun::noise::TunnResult::WriteToNetwork(handshake_data) = tunnel.update_timers(&mut initial_handshake_buf) {
                                    let _ = socket_sender.send_to(handshake_data, &cfg.vpn_endpoint);
                                    
                                    if let Ok(mut buf) = vpn_log_buffer.lock() {
                                        buf.push("🚀 [VPN] Inviato Handshake Initiation al MikroTik.".to_string());
                                    }
                                }
                                wg_tunnel = Some(tunnel);
                            } else {
                                if let Ok(mut buf) = vpn_log_buffer.lock() {
                                    buf.push("❌ Impossibile attivare VPN: Verificare formato chiavi Base64.".to_string());
                                }
                                vpn_status.store(3, std::sync::atomic::Ordering::Relaxed); // FORZA LED ROSSO IN CASO DI CHIAVI ERRATE
                            }

                        }
                    },
                }
            }

            if let Some(n) = pin.take() {
                if let Some(d) = host.input_devices().unwrap().find(|x| x.name().unwrap_or_default() == n).or(host.default_input_device()) {
                    let conf = cpal::StreamConfig { channels: 2, sample_rate: cpal::SampleRate(48000), buffer_size: cpal::BufferSize::Default };
                    let (pi, ci) = HeapRb::<f32>::new(192000).split();
                    *current_prod_in.lock().unwrap() = Some(pi); *current_cons_in.lock().unwrap() = Some(ci);
                    let v = vi.clone(); let lv = input_lvls.clone(); let p_in_ptr = current_prod_in.clone();                    
					_s_in = d.build_input_stream(&conf, move |data: &[f32], _| {
						if let Some(ref mut pi) = *p_in_ptr.lock().unwrap() {
							let vol = f32::from_bits(v.load(Ordering::Relaxed)); 
							let mut pks = [0.0f32; 2]; // <--- QUI CREI UN ARRAY LOCALE DI 2 ELEMENTI PER I PICCHI

							for (i, &s) in data.iter().enumerate() {
								let sa = s * vol; 
								let canale_idx = i % 2; // 0 per Left, 1 per Right
								if sa.abs() > pks[canale_idx] { pks[canale_idx] = sa.abs(); }
								let _ = pi.push(sa);
							}
							
							// ❌ IL BUG DI INDICIZZAZIONE:
							lv[0].store(pks[0].to_bits(), Ordering::Relaxed); 
							lv[1].store(pks[1].to_bits(), Ordering::Relaxed);
						}
					}, |_|{}, None).ok();

                    if let Some(ref s) = _s_in { let _ = s.play(); }
                }
            }

            if let Some(n) = pout.take() {
                if let Some(d) = host.output_devices().unwrap().find(|x| x.name().unwrap_or_default() == n).or(host.default_output_device()) {
                    let conf = cpal::StreamConfig { channels: 2, sample_rate: cpal::SampleRate(48000), buffer_size: cpal::BufferSize::Default };
                    let (pi, co) = HeapRb::<f32>::new(192000).split();
                    *current_prod_out.lock().unwrap() = Some(pi); *current_cons_out.lock().unwrap() = Some(co);
                    let v = vo.clone(); let lv = output_lvls.clone(); let c_out_ptr = current_cons_out.clone();
                    
                    _s_out = d.build_output_stream(&conf, move |data: &mut [f32], _| {
                        if let Some(ref mut co) = *c_out_ptr.lock().unwrap() {
                            let vol = f32::from_bits(v.load(Ordering::Relaxed)); let mut pks = [0.0f32; 2];
                            for (i, sa) in data.iter_mut().enumerate() {
                                let s = co.pop().unwrap_or(0.0) * vol; *sa = s; let channel_idx = i % 2;
                                if s.abs() > pks[channel_idx] { pks[channel_idx] = s.abs(); }
                            }
                            lv[0].store(pks[0].to_bits(), Ordering::Relaxed); lv[1].store(pks[1].to_bits(), Ordering::Relaxed);
                        }
                    }, |_|{}, None).ok();
                    if let Some(ref s) = _s_out { s.play().ok(); }
                }
            }

            if active && is_licensed_active {
                if let Some(ref mut ci) = *current_cons_in.lock().unwrap() {
                    if ci.len() >= 1920 {
                        let mut pcm = [0f32; 1920]; for s in pcm.iter_mut() { *s = ci.pop().unwrap_or(0.0); }
                        let mut out_b = [0u8; 1500];
                        #[cfg(not(target_os = "android"))] let encode_res = enc.encode_float(&pcm, &mut out_b);
                        #[cfg(target_os = "android")] let encode_res = enc.encode_float(&pcm, &mut out_b).map_err(|_| opus::Error::from_code(0));
                        // =========================================================================
                        // --- ENVELOPE IP/UDP CON PORTA DINAMICA ESTRATTA DAL CONFIG ---
                        // =========================================================================
                        if let Ok(sz) = encode_res {
                            let pkt = pack_bin(seq_tx, 0, &out_b[..sz]);
                            
                            if cfg.vpn_enabled && wg_tunnel.is_some() {
                                let mut wg_packet_buf = [0u8; 2048];
                                if let Some(ref mut tunnel) = wg_tunnel {
                                    
                                    // 1. Alloca il buffer fisso da 28 byte per gli header di rete
                                    let mut header_28b = [0u8; 28];
                                    let total_len = 28 + pkt.len();
                                    let udp_len = 8 + pkt.len();
                                    
                                    // Costruzione HEADER IPv4 (Primi 20 byte)
                                    header_28b[0] = 0x45; // IPv4
                                    header_28b[2] = ((total_len >> 8) & 0xFF) as u8;
                                    header_28b[3] = (total_len & 0xFF) as u8;
                                    header_28b[8] = 64;   // TTL
                                    header_28b[9] = 17;   // Protocollo UDP
                                    
                                    // IP Sorgente dal file JSON
                                    if let Ok(src_ip) = cfg.vpn_local_ip.parse::<std::net::Ipv4Addr>() {
                                        header_28b[12..16].copy_from_slice(&src_ip.octets());
                                    }
                                    
                                    // IP Destinazione forzato sul Gateway del MikroTik per l'Allowed IP userspace
                                    let gateway_mikrotik = std::net::Ipv4Addr::new(192, 168, 11, 1);
                                    header_28b[16..20].copy_from_slice(&gateway_mikrotik.octets());
                                    
                                    // 2. SCRITTURA PORTE DINAMICHE DA CONFIGURAZIONE (Nessun calcolo a caldo nel loop!)
                                    let porta_sorgente: u16 = cfg.local_port as u16; // Es: 3333 dal JSON
                                    
                                    header_28b[20..22].copy_from_slice(&porta_sorgente.to_be_bytes());
                                    header_28b[22..24].copy_from_slice(&porta_destinazione_dinamica.to_be_bytes());
                                    header_28b[24..26].copy_from_slice(&(udp_len as u16).to_be_bytes());
                                    // header_28b[26..28] (Checksum UDP) rimane 0
                                    
                                    // Assemblaggio finale del frame IP/UDP + Audio Opus custom
                                    let mut frame_completo = Vec::with_capacity(total_len);
                                    frame_completo.extend_from_slice(&header_28b);
                                    frame_completo.extend_from_slice(&pkt);
                                    
                                    if let boringtun::noise::TunnResult::WriteToNetwork(wg_data) = tunnel.encapsulate(&frame_completo, &mut wg_packet_buf) {
                                        let _ = socket_sender.send_to(wg_data, &cfg.vpn_endpoint);
                                    }
                                }
                            } else {
                                if let Ok(sock) = current_socket.lock() { sock.send_to(&pkt, &target).ok(); }
                            }
                            b_tx += pkt.len(); seq_tx += 1;
                        }

                    }
                }
            } else if active && !is_licensed_active {
                if let Some(ref mut ci) = *current_cons_in.lock().unwrap() { ci.clear(); }
            }
            
            // =========================================================================
            // --- TICK WIREGUARD CON COPIA RIGIDA FLUSSO RETE (NO ALLINEAMENTI) ---
            // =========================================================================
            if cfg.vpn_enabled && wg_tunnel.is_some() && last_wg_tick.elapsed() >= Duration::from_millis(1000) {
                let mut wg_tick_buf = [0u8; 1420]; 
                if let Some(ref mut tunnel) = wg_tunnel {
                    match tunnel.update_timers(&mut wg_tick_buf) {
                        boringtun::noise::TunnResult::WriteToNetwork(wg_data) => {
                            let byte_reali = wg_data.len();
                            
                            log_vpn(&format!("🚀 Invio pacchetto Handshake/Keepalive al MikroTik ({} byte)", byte_reali));
                            let mut pacchetto_puro = vec![0u8; byte_reali];
                            pacchetto_puro.copy_from_slice(&wg_tick_buf[..byte_reali]);
                            let _ = socket_sender.send(&pacchetto_puro);
                        },
                        boringtun::noise::TunnResult::Err(err) => {
                            log_vpn(&format!("❌ Errore crittografico: {:?}", err));
                            vpn_status.store(3, std::sync::atomic::Ordering::Relaxed);
                        },
                        _ => {}
                    }
                }
                last_wg_tick = Instant::now();
            }

            // =========================================================================
            // --- CORE RICEZIONE RETE USERSPACE CON AGGANCIO IP VIRTUAL GATEWAY ---
            // =========================================================================
            let mut buf = [0u8; 2048];
            let mut read_res = None;
            if let Ok(sock) = current_socket.lock() { read_res = sock.recv_from(&mut buf).ok(); }
            
            if let Some((n, addr)) = read_res {
                let mut pacchetto_elaborabile = Some(buf[..n].to_vec());
                
                if cfg.vpn_enabled && wg_tunnel.is_some() {
                    let mut decrypted_buf = [0u8; 2048];
                    if let Some(ref mut tunnel) = wg_tunnel {
                        
                        // SBLOCCO CRITTOGRAFICO ASSOLUTO: Forziamo l'IP virtuale interno del MikroTik (192.168.11.1)
                        // Questo bypassa i vincoli di anti-spoofing di boringtun, indicandogli che il mittente 
                        // appartiene legalmente alla classe autorizzata /24, sbloccando la decodifica dell'audio!
                        let ip_virtuale_mikrotik = std::net::IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 11, 1));

                        match tunnel.decapsulate(Some(ip_virtuale_mikrotik), &buf[..n], &mut decrypted_buf) {
                            boringtun::noise::TunnResult::WriteToTunnelV4(dec_data, _) => {
                                // RICEZIONE CONFERMATA: Il tunnel ha estratto l'audio in chiaro!
                                if vpn_status.load(std::sync::atomic::Ordering::Relaxed) != 2 {
                                    vpn_status.store(2, std::sync::atomic::Ordering::Relaxed);
                                }
                                pacchetto_elaborabile = Some(dec_data.to_vec());
                            },
                            boringtun::noise::TunnResult::WriteToNetwork(handshake_response) => {
                                vpn_status.store(2, std::sync::atomic::Ordering::Relaxed);
                                let _ = socket_sender.send_to(handshake_response, &cfg.vpn_endpoint);
                                pacchetto_elaborabile = None;
                            },
                            boringtun::noise::TunnResult::Done => {
                                if vpn_status.load(std::sync::atomic::Ordering::Relaxed) != 2 {
                                    vpn_status.store(2, std::sync::atomic::Ordering::Relaxed);
                                }
                                pacchetto_elaborabile = None;
                            },
                            boringtun::noise::TunnResult::Err(err) => {
                                // Filtra l'errore di timeout periodico per non sporcare la console
                                if !format!("{:?}", err).contains("ConnectionExpired") {
                                    log_vpn(&format!("❌ Errore crittografico: {:?}", err));
                                }
                                pacchetto_elaborabile = None;
                            },
                            _ => { pacchetto_elaborabile = None; }
                        }
                    }
                }

                // --- PIPELINE DI DISTRIBUZIONE ALL'ENGINE AUDIO ---
                if let Some(pkt_raw) = pacchetto_elaborabile {
                    // Quando boringtun estrae il pacchetto nello stato WriteToTunnelV4, 
                    // restituisce solo l'UDP finto (8 byte). Saltiamo questi 8 byte per leggere unpack_bin!
                    let dati_audio_reali = if cfg.vpn_enabled && pkt_raw.len() > 8 {
                        &pkt_raw[8..]
                    } else {
                        &pkt_raw[..]
                    };

                    if let Some((seq, ctrl, data)) = unpack_bin(dati_audio_reali) {
                        // INCREMENTO TELEMETRIA: Alimenta i kbps dell'interfaccia grafica uscando dallo zero!
                        b_rx += n; 
                        
                        match ctrl {
                            0 => {
                                // Flusso audio Opus valido: inserimento nel Jitter Buffer
                                let ora = Instant::now(); 
                                let mut lp_lock = lp.lock().unwrap();
                                let diff = ora.duration_since(*lp_lock).as_secs_f64() * 1000.0; 
                                *lp_lock = ora;
                                
                                if let Ok(mut ra_guard) = ra.lock() {
                                    *ra_guard = addr.to_string(); // Mostra l'IP sorgente reale nei log
                                }
                                j_map.insert(seq, data); 
                                let _ = jitter_tx.send(diff.min(100.0));
                            },
                            1 => { j_map.clear(); },
                            _ => {}
                        }
                    }
                }
            }

            if j_map.len() >= delay_pkts {
                if j_map.len() > delay_pkts + 3 { j_map.pop_first(); }
                if let Some((_, data)) = j_map.pop_first() {
                    if is_licensed_active {
                        if let Some(ref mut po) = *current_prod_out.lock().unwrap() {
                            let mut opcm = [0f32; 1920]; let res = dec.decode_float(&data, &mut opcm, false);
                            if let Ok(sz) = res { for &s in &opcm[..sz * 2] { let _ = po.push(s); } }
                        }
                    }
                }
            } else if !is_licensed_active { j_map.clear(); }

            if last_bw.elapsed() >= Duration::from_secs(1) {
                // Aggiorna i bitrate storici esistenti
                tk.store((b_tx * 8 / 1024) as u32, Ordering::Relaxed); 
                rk.store((b_rx * 8 / 1024) as u32, Ordering::Relaxed);
                b_tx = 0; b_rx = 0; last_bw = Instant::now();

                // Se l'applicazione è attiva (o la VPN è verde), spariamo il nostro PING con ctrl == 2
                let pkt_ping = pack_bin(0, 2, b"PING_AUDIO_TX");

                if cfg.vpn_enabled && wg_tunnel.is_some() {
                    let mut wg_p_buf = [0u8; 2048];
                    if let Some(ref mut tunnel) = wg_tunnel {
                        // Creiamo l'envelope IP userspace per bypassare la RAM di boringtun
                        let mut ip_frame = vec![0u8; 20 + pkt_ping.len()];
                        ip_frame[0] = 0x45;
                        let t_len = ip_frame.len();
                        ip_frame[2] = ((t_len >> 8) & 0xFF) as u8; ip_frame[3] = (t_len & 0xFF) as u8;
                        ip_frame[8] = 64; ip_frame[9] = 17;
                        if let (Ok(src_ip), Ok(dst_ip)) = (cfg.vpn_local_ip.parse::<std::net::Ipv4Addr>(), target.split(':').next().unwrap_or("127.0.0.1").parse::<std::net::Ipv4Addr>()) {
                            ip_frame[12..16].copy_from_slice(&src_ip.octets()); ip_frame[16..20].copy_from_slice(&dst_ip.octets());
                        }
                        ip_frame[20..].copy_from_slice(&pkt_ping);

                        if let boringtun::noise::TunnResult::WriteToNetwork(wg_data) = tunnel.encapsulate(&ip_frame, &mut wg_p_buf) {
                            let _ = socket_sender.send(wg_data);
                        }
                    }
                } else {
                    // Trasmissione standard diretta su IP chiaro senza VPN
                    let _ = socket_sender.send_to(&pkt_ping, &target);
                }
			}
            std::thread::sleep(Duration::from_nanos(10));
        }
    });
}

// =========================================================================
// --- GENERATORE AUTOMATICO IDENTITÀ CRITTOGRAFICA WIREGUARD ---
// =========================================================================
fn verifica_e_genera_chiavi_vpn(cfg: &mut Config) {
    if cfg.vpn_private_key.is_empty() {
        use base64::{Engine as _, engine::general_purpose};
        use rand::RngCore;
        let mut private_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut private_bytes);
        private_bytes[0] &= 248;
        private_bytes[31] &= 127;
        private_bytes[31] |= 64;
		cfg.vpn_private_key = general_purpose::STANDARD.encode(&private_bytes);
        if let Ok(json_str) = serde_json::to_string_pretty(cfg) {
            let _ = std::fs::write("config.json", json_str);
        }
        println!("🔑 Nuova identità VPN WireGuard generata ed archiviata in config.json.");
    }
}



// =========================================================================
// VU-METER SU PORTE GPIO LIBERE
// =========================================================================
#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
mod rpi_gpio {
    use std::fs::File;
    use std::io::Write;
    use std::path::Path;

    // Pin BCM sicuri e liberi che non interferiscono con il bus I2S/I2C di HifiBerry
    pub const PIN_L: [u32; 4] = [22, 23, 24, 25];
    pub const PIN_R: [u32; 4] = [17, 27, 5, 26]; // Sostituito il pin 18 con il pin 5

    pub fn inizializza_gpio() {
        for &pin in PIN_L.iter().chain(PIN_R.iter()) {
            let export_path = "/sys/class/gpio/export";
            let pin_dir = format!("/sys/class/gpio/gpio{}", pin);
            
            if !Path::new(&pin_dir).exists() {
                if let Ok(mut f) = File::create(export_path) {
                    let _ = write!(f, "{}", pin);
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            
            let direction_path = format!("{}/direction", pin_dir);
            if let Ok(mut f) = File::create(direction_path) {
                let _ = write!(f, "out");
            }
        }
    }

    pub fn aggiorna_led(val_bits: u32, pins: &[u32; 4]) {
        let val = f32::from_bits(val_bits).clamp(0.0, 1.0);
        
        let led_accesi = if val > 0.8 { 4 }
                         else if val > 0.5 { 3 }
                         else if val > 0.2 { 2 }
                         else if val > 0.05 { 1 }
                         else { 0 };

        for i in 0..4 {
            let value_path = format!("/sys/class/gpio/gpio{}/value", pins[i]);
            if let Ok(mut f) = File::create(value_path) {
                let stato = if i < led_accesi { "1" } else { "0" };
                let _ = write!(f, "{}", stato);
            }
        }
    }
}

// =========================================================================
// VUMETER TESTUALE PER RPI CONSOLE
// =========================================================================
#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
fn genera_vumeter_string(val_bits: u32, width: usize) -> String {
    let val = f32::from_bits(val_bits).clamp(0.0, 1.0);
    let ticks = (val * width as f32) as usize;
    
    let mut bar = String::with_capacity(width);
    for i in 0..width {
        if i < ticks {
            if i > (width * 80 / 100) { bar.push('!'); }
            else { bar.push('|'); }
        } else {
            bar.push('.');
        }
    }
    bar
}


// =========================================================================
// PUNTO DI INGRESSO DESKTOP CON INTERFACCIA GRAFICA (Linux x86_64, Windows)
// =========================================================================
#[cfg(target_arch = "x86_64")]
pub fn main() -> Result<(), eframe::Error> {
    let args: Vec<String> = std::env::args().collect();
    let hwid = generate_hwid();

    let file_content = if std::path::Path::new("config.json").exists() {
        std::fs::read_to_string("config.json").unwrap_or_default()
    } else {
        String::new()
    };

    let mut cfg: Config = serde_json::from_str(&file_content).unwrap_or_else(|_| {
        Config { 
            target_ip: "127.0.0.1:12345".into(),
            local_port: 12345,
            buffer_size_ms: 60,
            selected_tab: Tab::TX,
            in_volume: 1.0,
            out_volume: 1.0,
            selected_in: "Default".into(),
            selected_out: "Default".into(),
            bitrate_kbps: 96,
            customer_name: "Default User".into(),
            license_key: "".into(),
            mode: "BOTH".into(),
            autostart: false,
            vpn_enabled: false,
            vpn_private_key: "".into(),
            vpn_peer_public_key: "".into(),
            vpn_endpoint: "".into(),
            vpn_local_ip: "10.0.0.2".into(),
            vpn_allowed_ips: "10.0.0.0/24".into(),
        }
    });

    // Genera l'identità VPN al boot se mancante nel file JSON
    verifica_e_genera_chiavi_vpn(&mut cfg);

    let auth = verify_license_static(&hwid, &cfg.customer_name, &cfg.license_key);

    let host = cpal::default_host();
    let elenco_in: Vec<String> = host.input_devices().unwrap().map(|d| d.name().unwrap_or_default()).collect();
    let elenco_out: Vec<String> = host.output_devices().unwrap().map(|d| d.name().unwrap_or_default()).collect();

    let (cmd_tx, cmd_rx) = mpsc::channel();    
    let (jitter_tx, jitter_rx) = mpsc::channel();

    let input_lvls = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let output_lvls = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let tx_k = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let rx_k = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let v_in = Arc::new(std::sync::atomic::AtomicU32::new((1.0f32).to_bits()));
    let v_out = Arc::new(std::sync::atomic::AtomicU32::new((1.0f32).to_bits()));
    let r_addr = Arc::new(Mutex::new("Nessuno".into()));
    let l_pkt = Arc::new(Mutex::new(Instant::now()));
    
    // 1. Inizializzazione pulita dei canali della VPN per x86_64
    let vpn_st = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let vpn_log_buf = Arc::new(std::sync::Mutex::new(Vec::new()));

    // 2. CREAZIONE DEI CLONI ESPLICITI PER LA GUI (Isolamento dei puntatori)
    let input_lvls_app = input_lvls.clone();
    let output_lvls_app = output_lvls.clone();
    let v_in_app = v_in.clone();
    let v_out_app = v_out.clone();
    let tx_k_app = tx_k.clone();
    let rx_k_app = rx_k.clone();
    let r_addr_app = r_addr.clone();
    let l_pkt_app = l_pkt.clone();
    
    // I cloni espliciti per la telemetria della GUI Windows/Linux Tab
    let vpn_st_app = vpn_st.clone();
    let vpn_log_buf_app = vpn_log_buf.clone();

    println!("\n🚀 Avvio del core di trasmissione AudioTX PRO...");

    // 3. MOTORE AUDIO: Riceve i puntatori nativi originali
    start_audio_engine(
        cfg.clone(),
        cmd_rx,
        jitter_tx,
        [input_lvls, Arc::new(std::sync::atomic::AtomicU32::new(0))],   // Canale L agganciato nativo
        [output_lvls, Arc::new(std::sync::atomic::AtomicU32::new(0))], // Canale L agganciato nativo
        tx_k,
        rx_k,
        v_in,
        v_out,
        r_addr,
        l_pkt,
        vpn_st,       // Passa l'istanza atomica su cui boringtun scriverà lo stato
        vpn_log_buf,  // Passa l'istanza nativa su cui boringtun scriverà i log
    );

    let mut options = eframe::NativeOptions::default();
    options.viewport = eframe::egui::ViewportBuilder::default().with_inner_size([550.0, 850.0]);
    
    // 4. GUI INTERFACCIA: Riceve i cloni protetti speculari
    eframe::run_native("AudioTX PRO", options, Box::new(move |cc| {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        Box::new(AudioApp {
            input_levels: [input_lvls_app.clone(), input_lvls_app], // Duplicato per accendere L e R simmetrici
            output_levels: [output_lvls_app.clone(), output_lvls_app], // Duplicato per accendere L e R simmetrici
            in_volume: v_in_app,
            out_volume: v_out_app,
            tx_kbps: tx_k_app,
            rx_kbps: rx_k_app,
            selected_tab: cfg.selected_tab,
            input_devices: elenco_in,   
            output_devices: elenco_out,
            selected_in: cfg.selected_in,
            selected_out: cfg.selected_out, 
            buffer_size_ms: cfg.buffer_size_ms,
            target_ip: cfg.target_ip,
            local_port: cfg.local_port, 
            bitrate_kbps: cfg.bitrate_kbps,
            is_transmitting: cfg.autostart,
            command_tx: cmd_tx, 
            latency_history: std::collections::VecDeque::new(),
            hwid,
            remote_addr: r_addr_app, 
            last_packet_time: l_pkt_app,
            session_start: None,
            jitter_rx, 
            customer_name: cfg.customer_name,
            license_key: cfg.license_key,
            is_licensed: auth,
            mode: cfg.mode,
            autostart: cfg.autostart,
            vpn_enabled: cfg.vpn_enabled,
            vpn_private_key: cfg.vpn_private_key.clone(),
            vpn_peer_public_key: cfg.vpn_peer_public_key.clone(),
            vpn_endpoint: cfg.vpn_endpoint.clone(),
            vpn_local_ip: cfg.vpn_local_ip.clone(),
            vpn_allowed_ips: cfg.vpn_allowed_ips.clone(),
            vpn_status: vpn_st_app,
            vpn_log_buffer: vpn_log_buf_app,
        })
    }))
}

// =========================================================================
// PUNTO DI INGRESSO HEADLESS PER RASPBERRY PI (ARM64)
// =========================================================================
#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
fn main() -> Result<(), ()> {
    disattiva_errori_alsa();
    rpi_gpio::inizializza_gpio();
    let hwid = generate_hwid();
    let args: Vec<String> = std::env::args().collect();

    // --- 1. PARAMETRI BULLET-PROOF AMMESSI SEMPRE ---
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("\n=== AUDIOTX PRO - HELP STRUMENTI COMMAND LINE ===");
        println!("Utilizzo: ./audio_bridge [OPZIONI]");
        println!("\nOpzioni operative (valide SOLO con licenza attiva):");
        println!("  --volume-in <v>    Imposta il volume di input (Gain del microfono, range: 0 - 100%)");
        println!("  --volume-out <v>   Imposta il volume di output (range: 0 - 100%)");
        println!("  --url-tx <ip:p>    Imposta l'indirizzo IP e la porta di destinazione della trasmissione");
        println!("  --port-rx <p>      Imposta la porta UDP locale in ascolto per la ricezione");
        println!("  --buffer <ms>      Imposta la dimensione del buffer di jitter in millisecondi");
        println!("  --set-audio-tx <n> Imposta la scheda audio fissa da usare per l'input (TX)");
        println!("  --set-audio-rx <n> Imposta la scheda audio fissa da usare per l'output (RX)");
        println!("  --autostart <t/f>  Abilita (true) o disabilita (false) l'avvio automatico della trasmissione");
        println!("\nOpzioni di diagnostica:");
        println!("  --list-tx          Mostra l'elenco dei dispositivi di input audio disponibili");
        println!("  --list-rx          Mostra l'elenco dei dispositivi di output audio disponibili\n");
        println!("\n=== AUDIOTX PRO - HELP STRUMENTI COMMAND LINE ===");
        println!("Utilizzo: ./audio_bridge [OPZIONI]");
        println!("  --autostart <t/f>       Abilita o disabilita l'avvio automatico");
        println!("  --vpn <true/false>      Attiva o disattiva il tunnel crittografico Userspace");
        println!("  --vpn-endpoint <ip:p>   Configura l'endpoint pubblico del Server (Es: 1.2.3.4:51820)");
        println!("  --vpn-local-ip <ip>     Configura l'IP interno virtuale del tunnel (Es: 10.0.0.2)");
        println!("  --vpn-allowed-ips <net/mask> Configura le rotte consentite in RAM (Es: 10.0.0.0/24)");
		println!("  --vpn-peer-pub <key>    Incolla la chiave pubblica del Server centrale");
        return Ok(());
    }

    if args.iter().any(|arg| arg == "--list-tx") {
        println!("\n--- DISPOSITIVI DI INPUT DISPONIBILI (TX) ---");
        if let Ok(devices) = cpal::default_host().input_devices() {
            for d in devices { println!(" - {}", d.name().unwrap_or_else(|_| "Sconosciuto".into())); }
        }
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--list-rx") {
        println!("\n--- DISPOSITIVI DI OUTPUT DISPONIBILI (RX) ---");
        if let Ok(devices) = cpal::default_host().output_devices() {
            for d in devices { println!(" - {}", d.name().unwrap_or_else(|_| "Sconosciuto".into())); }
        }
        return Ok(());
    }

    // --- 2. CARICAMENTO CONFIGURAZIONE BASE ---
    let mut cfg = if std::path::Path::new("config.json").exists() {
        serde_json::from_str(&std::fs::read_to_string("config.json").unwrap_or_default()).unwrap_or_else(|_| wizard(&hwid))
    } else { 
        wizard(&hwid) 
    };

    // --- 3. VERIFICA LICENZA BLOCCANTE ---
    let mut auth = verify_license_static(&hwid, &cfg.customer_name, &cfg.license_key);
    if !auth {
        cfg = wizard(&hwid);
        auth = verify_license_static(&hwid, &cfg.customer_name, &cfg.license_key);
        if !auth { return Err(()); }
    }

        // --- 4. PARSING ED ELABORAZIONE PARAMETRI OPERATIVI DA CLI (CON TRAIT FLEX PER bool) ---
    let mut agg_cfg = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--volume-in" if i + 1 < args.len() => { if let Ok(v) = args[i + 1].parse::<f32>() { cfg.in_volume = (v.clamp(0.0, 200.0)) / 100.0; agg_cfg = true; } i += 2; }
            "--volume-out" if i + 1 < args.len() => { if let Ok(v) = args[i + 1].parse::<f32>() { cfg.out_volume = (v.clamp(0.0, 100.0)) / 100.0; agg_cfg = true; } i += 2; }
            "--url-tx" if i + 1 < args.len() => { cfg.target_ip = args[i + 1].clone(); agg_cfg = true; i += 2; }
            "--port-rx" if i + 1 < args.len() => { if let Ok(p) = args[i + 1].parse::<u16>() { cfg.local_port = p; agg_cfg = true; } i += 2; }
            "--buffer" if i + 1 < args.len() => { if let Ok(b) = args[i + 1].parse::<u32>() { cfg.buffer_size_ms = b; agg_cfg = true; } i += 2; }
            "--set-audio-tx" if i + 1 < args.len() => { cfg.selected_in = args[i + 1].clone(); agg_cfg = true; i += 2; }
            "--set-audio-rx" if i + 1 < args.len() => { cfg.selected_out = args[i + 1].clone(); agg_cfg = true; i += 2; }
            
            // Gestisce in modo flessibile sia true/false che t/f
            "--autostart" if i + 1 < args.len() => {
                let val = args[i + 1].to_lowercase();
                cfg.autostart = val == "true" || val == "t" || val == "1";
                agg_cfg = true;
                i += 2;
            }
            "--vpn" if i + 1 < args.len() => { 
                let val = args[i + 1].to_lowercase();
                cfg.vpn_enabled = val == "true" || val == "t" || val == "1";
                agg_cfg = true; 
                i += 2; 
            }
            "--vpn-endpoint" if i + 1 < args.len() => { cfg.vpn_endpoint = args[i + 1].clone(); agg_cfg = true; i += 2; }
            "--vpn-local-ip" if i + 1 < args.len() => { cfg.vpn_local_ip = args[i + 1].clone(); agg_cfg = true; i += 2; }
            "--vpn-allowed-ips" if i + 1 < args.len() => { cfg.vpn_allowed_ips = args[i + 1].clone(); agg_cfg = true; i += 2; }
            "--vpn-peer-pub" if i + 1 < args.len() => { cfg.vpn_peer_public_key = args[i + 1].clone(); agg_cfg = true; i += 2; }
            _ => i += 1,
        }
    }

    if agg_cfg { 
        let _ = std::fs::write("config.json", serde_json::to_string_pretty(&cfg).unwrap()); 
    }

    // --- 5. AUTOMATIZZAZIONE GENERAZIONE IDENTITÀ NATIVA ---
    verifica_e_genera_chiavi_vpn(&mut cfg);

    // --- 6. DERIVAZIONE CRITTOGRAFICA CHIAVE PUBBLICA PER IL LOG ---
    let chiave_pubblica_headless = if !cfg.vpn_private_key.is_empty() {
        use base64::{Engine as _, engine::general_purpose};
        if let Ok(dec_bytes) = general_purpose::STANDARD.decode(cfg.vpn_private_key.trim()) {
            if dec_bytes.len() == 32 {
                let mut priv_arr = [0u8; 32];
                priv_arr.copy_from_slice(&dec_bytes);
                let priv_k: boringtun::x25519::StaticSecret = priv_arr.into();
                let pub_k = boringtun::x25519::PublicKey::from(&priv_k);
                general_purpose::STANDARD.encode(pub_k.as_ref())
            } else { "Errore: Chiave privata corrotta".into() }
        } else { "Errore: Base64 non valido".into() }
    } else {
        "Nessuna".into()
    };

    // --- 7. INIZIALIZZAZIONE STRUTTURE ATOMICHE (PULITE) ---
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (jitter_tx, _jitter_rx) = mpsc::channel();
    let input_lvls = [Arc::new(AtomicU32::new(0)), Arc::new(AtomicU32::new(0))];
    let output_lvls = [Arc::new(AtomicU32::new(0)), Arc::new(AtomicU32::new(0))];
    let tx_k = Arc::new(AtomicU32::new(0)); let rx_k = Arc::new(AtomicU32::new(0));
    let v_in = Arc::new(AtomicU32::new(cfg.in_volume.to_bits()));
    let v_out = Arc::new(AtomicU32::new(cfg.out_volume.to_bits()));
    let r_addr = Arc::new(Mutex::new("Nessuno".into()));
    let l_pkt = Arc::new(Mutex::new(Instant::now()));

    // RIMOZIONE LOG VECCHI DUPLICATI: Sostituiti con questo blocco diagnostico unificato unico
    println!("\n🚀 AVVIO DEL CORE AUDIO BRIDGE PER RASPBERRY PI...");
    println!(" -> Periferica Input (TX)   : {}", cfg.selected_in);
    println!(" -> Periferica Output (RX)  : {}", cfg.selected_out);
    println!(" -> Destinazione Remota     : {}", cfg.target_ip);
    println!(" -> Ascolto UDP Locale      : {}", cfg.local_port);
    println!(" -> Buffer ritardo in ms    : {}", cfg.buffer_size_ms);
    println!(" -> Avvio Automatico (Boot) : {}", cfg.autostart);
    println!(" -> Stato VPN WireGuard     : {}", if cfg.vpn_enabled { "ATTIVATA" } else { " DISATTIVATA" });
    
        if cfg.vpn_enabled {
        println!(" -> VPN Endpoint Server     : {}", cfg.vpn_endpoint);
        println!(" -> VPN IP Locale Virtuale  : {}", cfg.vpn_local_ip);
        println!(" -> VPN Allowed IPs (RAM)   : {}", cfg.vpn_allowed_ips);
        println!(" -> 🔑 WG CHIAVE PUBBLICA   : \x1b[93m{}\x1b[0m", chiave_pubblica_headless); // Stampa in giallo brillante
    }
    println!("===========================================================\n");

    // --- ALLINEAMENTO CHIRURGICO DEI PARAMETRI MANCANTI (13 ARGOMENTI) ---
    let vpn_st_headless = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let vpn_log_buf_headless = Arc::new(std::sync::Mutex::new(Vec::new()));

    // MODIFICA: Aggiunto .clone() sui due parametri VPN per evitare il problema del move (E0382)
    start_audio_engine(
        cfg.clone(), 
        cmd_rx, 
        jitter_tx, 
        input_lvls.clone(), 
        output_lvls.clone(), 
        tx_k.clone(), 
        rx_k.clone(), 
        v_in.clone(), 
        v_out.clone(), 
        r_addr.clone(), 
        l_pkt.clone(),
        vpn_st_headless.clone(),     // .clone() risolve E0382
        vpn_log_buf_headless.clone() // .clone() per sicurezza
    );

        let _ = cmd_tx.send(AudioCommand::SetTransmitting(cfg.autostart));

    use std::io::{stdout, Write};

    // =========================================================================
    // --- MONITORAGGIO DINAMICO CON BARRE TESTUALI RIPRISTINATE ---
    // =========================================================================
    let mut last_print = Instant::now();

    loop {
        std::thread::sleep(Duration::from_millis(100)); // 100ms per fluidità visiva delle barre
        
        if last_print.elapsed() >= Duration::from_millis(200) {
            let tk_val = tx_k.load(Ordering::Relaxed);
            let rk_val = rx_k.load(Ordering::Relaxed);
            
            // Estrazione dei segnali dei VU-Meter
            let bits_in_l = input_lvls[0].load(Ordering::Relaxed);
            let bits_in_r = input_lvls[1].load(Ordering::Relaxed);
            let bits_out_l = output_lvls[0].load(Ordering::Relaxed);
            let bits_out_r = output_lvls[1].load(Ordering::Relaxed);

            // Generazione grafica delle barre tramite la tua funzione nativa
            let bar_in_l = genera_vumeter_string(bits_in_l, 12);
            let bar_in_r = genera_vumeter_string(bits_in_r, 12);
            let bar_out_l = genera_vumeter_string(bits_out_l, 12);
            let bar_out_r = genera_vumeter_string(bits_out_r, 12);
            
            // Lettura dinamica dello stato reale della VPN
            let stato_vpn_reale = vpn_st_headless.load(Ordering::Relaxed);
            let vpn_str = match stato_vpn_reale {
                1 => "\x1b[93m⏳ COLLEGAMENTO\x1b[0m", // Giallo
                2 => "\x1b[92m🟢 VPN CONNESSA\x1b[0m",  // Verde
                3 => "\x1b[91m❌ ERRORE CRITICO\x1b[0m",// Rosso
                _ => "\x1b[90m⚪ DISATTIVATA\x1b[0m",   // Grigio
            };

            // Stampa unificata a schermo (\r)
            print!(
                "\rTX L:[{}] R:[{}] {:4} kbps | RX L:[{}] R:[{}] {:4} kbps | VPN: {}         ", 
                bar_in_l, bar_in_r, tk_val, bar_out_l, bar_out_r, rk_val, vpn_str
            );
            stdout().flush().ok();
            
            // Aggiornamento concomitante dei LED sulle porte GPIO fisiche dell'RPI
            rpi_gpio::aggiorna_led(bits_in_l, &rpi_gpio::PIN_L);
            rpi_gpio::aggiorna_led(bits_in_r, &rpi_gpio::PIN_R);
            
            last_print = Instant::now();
        }
    }
    Ok(())
}


// =========================================================================
// ENTRYPOINT ANDROID (APK)
// =========================================================================
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: eframe::egui::AndroidApp) {
    let hwid = generate_hwid();
    let cfg = Config::default(); 
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (jitter_tx, jitter_rx) = mpsc::channel();
    let input_lvls = [Arc::new(AtomicU32::new(0)), Arc::new(AtomicU32::new(0))];
    let output_lvls = [Arc::new(AtomicU32::new(0)), Arc::new(AtomicU32::new(0))];
    let tx_k = Arc::new(AtomicU32::new(0)); let rx_k = Arc::new(AtomicU32::new(0));
    let v_in = Arc::new(AtomicU32::new(cfg.in_volume.to_bits()));
    let v_out = Arc::new(AtomicU32::new(cfg.out_volume.to_bits()));
    let r_addr = Arc::new(Mutex::new("Nessuno".into()));
    let l_pkt = Arc::new(Mutex::new(Instant::now()));

    println!("\n🚀 Avvio dell'interfaccia grafica AudioTX PRO...");

	// --- ALLINEAMENTO PARAMETRI JNI (Sostituisci l'invocazione finale di start_audio_engine) ---
	let _ = cmd_tx.send(AudioCommand::SetTransmitting(cfg.autostart));
	let _ = ANDROID_STOP_TX.set(cmd_tx);

	// Canali atomici locali necessari per soddisfare la firma a 13 argomenti
	let vpn_st_android = Arc::new(std::sync::atomic::AtomicU32::new(0));
	let vpn_log_buf_android = Arc::new(std::sync::Mutex::new(Vec::new()));
	
    start_audio_engine(
        cfg.clone(),
        cmd_rx,
        jitter_tx,
        [input_lvls_app.clone(), input_lvls[1].clone()],
        [output_lvls_app.clone(), output_lvls[1].clone()],
        tx_k,
        rx_k,
        v_in,
        v_out,
        r_addr,
        l_pkt,
        vpn_st.clone(),
        vpn_log_buf.clone(),
    );
    
    let mut options = eframe::NativeOptions::default();
    options.viewport = eframe::egui::ViewportBuilder::default().with_inner_size([550.0, 850.0]);
    
    eframe::run_native("AudioTX PRO", options, Box::new(move |cc| {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        Box::new(AudioApp {
            input_levels: input_lvls_app, output_levels: output_lvls_app, in_volume: v_in_app, out_volume: v_out_app,
            tx_kbps: tx_k_app, rx_kbps: rx_k_app, selected_tab: cfg.selected_tab, input_devices: elenco_in,   
            output_devices: elenco_out, selected_in: cfg.selected_in, selected_out: cfg.selected_out, 
            buffer_size_ms: cfg.buffer_size_ms, target_ip: cfg.target_ip, local_port: cfg.local_port, 
            bitrate_kbps: cfg.bitrate_kbps, is_transmitting: cfg.autostart, command_tx: cmd_tx, 
            latency_history: std::collections::VecDeque::new(), hwid, remote_addr: r_addr_app, 
            last_packet_time: l_pkt_app, session_start: None, jitter_rx, 
            customer_name: cfg.customer_name, license_key: cfg.license_key, is_licensed: auth, mode: cfg.mode,
            autostart: cfg.autostart,
            vpn_enabled: cfg.vpn_enabled,
            vpn_private_key: cfg.vpn_private_key,
            vpn_peer_public_key: cfg.vpn_peer_public_key,
            vpn_endpoint: cfg.vpn_endpoint,
            vpn_local_ip: cfg.vpn_local_ip,
            vpn_status: vpn_st,              
            vpn_log_buffer: vpn_log_buf,     
        })
    }))
}

// =========================================================================
// INTERFACCIA JNI PER FOREGROUND SERVICE ANDROID
// =========================================================================
#[cfg(target_os = "android")]
pub mod android_jni {
    use super::*;
    use std::sync::OnceLock;

    // Canale statico per poter controllare l'engine in background
    static ANDROID_STOP_TX: OnceLock<mpsc::Sender<AudioCommand>> = OnceLock::new();

    #[no_mangle]
    pub unsafe extern "C" fn Java_com_audiotx_pro_AudioBridgeService_startAudioEngineNative(
        _env: *mut std::ffi::c_void,
        _class: *mut std::ffi::c_void,
    ) {
        // 1. CARICAMENTO CONFIGURAZIONE SU ANDROID
        // Legge il JSON salvato nella memoria interna dell'app Android
        let cfg = if std::path::Path::new("config.json").exists() {
            let file_content = std::fs::read_to_string("config.json").unwrap_or_default();
            serde_json::from_str(&file_content).unwrap_or_else(|_| Config::default())
        } else {
            Config::default() // Utilizza l'autostart di default se il file non esiste ancora
        };
        verifica_e_genera_chiavi_vpn(&mut cfg);
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (jitter_tx, _jitter_rx) = mpsc::channel();
        let input_lvls = [Arc::new(AtomicU32::new(0)), Arc::new(AtomicU32::new(0))];
        let output_lvls = [Arc::new(AtomicU32::new(0)), Arc::new(AtomicU32::new(0))];
        let tx_k = Arc::new(AtomicU32::new(0)); let rx_k = Arc::new(AtomicU32::new(0));
        let v_in = Arc::new(AtomicU32::new(cfg.in_volume.to_bits()));
        let v_out = Arc::new(AtomicU32::new(cfg.out_volume.to_bits()));
        let r_addr = Arc::new(Mutex::new("Nessuno".into()));
        let l_pkt = Arc::new(Mutex::new(Instant::now()));

        // MODIFICA: Applica l'autostart letto dal JSON all'avvio su Android
        let _ = cmd_tx.send(AudioCommand::SetTransmitting(cfg.autostart));
        
        let _ = ANDROID_STOP_TX.set(cmd_tx);

        start_audio_engine(cfg, cmd_rx, jitter_tx, input_lvls, output_lvls, 
                           tx_k, rx_k, v_in, v_out, r_addr, l_pkt);
    }

    // --- IL TUO METODO ORIGINALE PER COMPLETARE LO STOP NATIVO ---
    #[no_mangle]
    pub unsafe extern "C" fn Java_com_audiotx_pro_AudioBridgeService_stopAudioEngineNative(
        _env: *mut std::ffi::c_void,
        _class: *mut std::ffi::c_void,
    ) {
        if let Some(tx) = ANDROID_STOP_TX.get() {
            let _ = tx.send(AudioCommand::SetTransmitting(false));
        }
    }

    // --- NUOVO METODO: PERMETTE ALL'INTERFACCIA ANDROID DI ACCENDERE/SPEGNERE IL FLUSSO A CALDO ---
    #[no_mangle]
    pub unsafe extern "C" fn Java_com_audiotx_pro_AudioBridgeService_setTransmittingNative(
        _env: *mut std::ffi::c_void,
        _class: *mut std::ffi::c_void,
        state: bool,
    ) {
        if let Some(tx) = ANDROID_STOP_TX.get() {
            let _ = tx.send(AudioCommand::SetTransmitting(state));
        }
    }
}
