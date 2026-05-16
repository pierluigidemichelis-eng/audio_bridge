#[cfg(feature = "gui")]
use eframe::egui;
#[cfg(feature = "gui")]
use egui_plot::{Line, Plot, PlotPoints};
use std::sync::{Arc, Mutex, mpsc::Receiver, mpsc::Sender};
use std::sync::atomic::{AtomicU32, Ordering};
use std::collections::VecDeque;
use sha2::{Sha256, Digest};
use serde::{Serialize, Deserialize};

#[derive(PartialEq, Serialize, Deserialize, Clone, Copy)]
pub enum Tab { TX, RX, VPN }

pub enum AudioCommand {
    UpdateTarget(String), UpdateBufferSize(u32), UpdateLocalPort(u16),
    SetTransmitting(bool), UpdateInputDevice(String), UpdateOutputDevice(String),
    UpdateBitrate(u32), UpdateAuth(bool),UpdateVpnEnabled(bool),
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    pub target_ip: String,
    pub local_port: u16,
    pub buffer_size_ms: u32,
    pub selected_tab: Tab,
    pub in_volume: f32,
    pub out_volume: f32,
    pub selected_in: String,
    pub selected_out: String,
    pub bitrate_kbps: u32,
    pub customer_name: String,
    pub license_key: String,
    pub mode: String,
    pub autostart: bool, 
      // --- CAMPI TUNNEL VPN WIREGUARD ---
    pub vpn_enabled: bool,
    pub vpn_private_key: String,      // Chiave privata Base64 locale (X25519)
    pub vpn_peer_public_key: String,  // Chiave pubblica Base64 del Server VPN centrale
    pub vpn_endpoint: String,         // SERVER_IP:PORTA del Server WireGuard fisico
    pub vpn_local_ip: String,  
    pub vpn_allowed_ips: String, 
}

#[cfg(feature = "gui")]
pub struct AudioApp {
    pub input_levels: [Arc<AtomicU32>; 2],
    pub output_levels: [Arc<AtomicU32>; 2],
    pub in_volume: Arc<AtomicU32>,
    pub out_volume: Arc<AtomicU32>,
    pub tx_kbps: Arc<AtomicU32>,
    pub rx_kbps: Arc<AtomicU32>,
    pub selected_tab: Tab,
    pub input_devices: Vec<String>,
    pub output_devices: Vec<String>,
    pub selected_in: String,
    pub selected_out: String,
    pub buffer_size_ms: u32,
    pub target_ip: String,
    pub local_port: u16,
    pub bitrate_kbps: u32,
    pub is_transmitting: bool,
    pub command_tx: Sender<AudioCommand>,
    pub latency_history: VecDeque<f64>,
    pub hwid: String,
    pub remote_addr: Arc<Mutex<String>>,
    pub last_packet_time: Arc<Mutex<std::time::Instant>>,
    pub session_start: Option<std::time::Instant>,
    pub jitter_rx: Receiver<f64>,
    pub customer_name: String,
    pub license_key: String,
    pub is_licensed: bool,
    pub mode: String,
    pub autostart: bool,
    pub vpn_enabled: bool,
	pub vpn_private_key: String,
	pub vpn_peer_public_key: String,
	pub vpn_endpoint: String,
	pub vpn_local_ip: String,
	pub vpn_allowed_ips: String, 
	pub vpn_status: Arc<std::sync::atomic::AtomicU32>,
    pub vpn_log_buffer: Arc<std::sync::Mutex<Vec<String>>>,
}


#[cfg(feature = "gui")]
impl eframe::App for AudioApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. ALLOCAZIONE BARRA DI STATO INFERIORE FISSA (Dimensioni Maggiorate)
                
        // =========================================================================
        // --- BARRA INFERIORE CON DOPPIA SPIA DIAGNOSTICA TX / RX ---
        // =========================================================================
        egui::TopBottomPanel::bottom("status_bar_vpn")
            .resizable(false)
            .min_height(32.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    
                    // SPIA 1: CONNETTIVITÀ INTERFACCIA CRITTOGRAFICA WIREGUARD
                    let stato_led = self.vpn_status.load(std::sync::atomic::Ordering::Relaxed);
                    let (colore_vpn, testo_vpn) = match stato_led {
                        1 => (egui::Color32::from_rgb(230, 230, 50), "⏳ WG Link..."),
                        2 => (egui::Color32::from_rgb(46, 204, 113), "🟢 WG Criptato"),
                        3 => (egui::Color32::from_rgb(231, 76, 60), "❌ Errore VPN"),
                        _ => (egui::Color32::from_rgb(149, 165, 166), "⚪ VPN Off"),
                    };
                    let (rect_v, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect_v.center(), 6.0, colore_vpn);
                    ui.label(egui::RichText::new(testo_vpn).strong().color(colore_vpn));
                    
                    ui.separator();
                    
                    // SPIA 2: TELEMETRIA DI TRASMISSIONE ED ECO-ACK (TX STATE)
                    let valore_tx = self.tx_kbps.load(std::sync::atomic::Ordering::Relaxed);
                    // Se stiamo trasmettendo chilobit, accendiamo la spia TX di verde/azzurro
                    let colore_tx_spia = if valore_tx > 0 { egui::Color32::from_rgb(52, 152, 219) } else { egui::Color32::GRAY };
                    let (rect_tx, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect_tx.center(), 6.0, colore_tx_spia);
                    
                    ui.label(egui::RichText::new("TX:").strong());
                    ui.label(egui::RichText::new(format!("{:4} kbps", valore_tx))
                        .font(egui::FontId::monospace(12.0))
                        .color(colore_tx_spia));
                        
                    ui.add_space(10.0);
                    
                    // SPIA 3: TELEMETRIA DI RICEZIONE REALE (RX STATE)
                    let valore_rx = self.rx_kbps.load(std::sync::atomic::Ordering::Relaxed);
                    let colore_rx_spia = if valore_rx > 0 { egui::Color32::from_rgb(46, 204, 113) } else { egui::Color32::GRAY };
                    let (rect_rx, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect_rx.center(), 6.0, colore_rx_spia);
                    
                    ui.label(egui::RichText::new("RX:").strong());
                    ui.label(egui::RichText::new(format!("{:4} kbps", valore_rx))
                        .font(egui::FontId::monospace(12.0))
                        .color(colore_rx_spia));
                    
					ui. separator();
					ui. label( egui:: RichText:: new( format!("HWID: {}", self. hwid)). weak());
					});
				});

				egui:: CentralPanel:: default(). show( ctx, |ui| {
				// Qui dentro risiede la tua gestione dei Tab (TX, RX, VPN)
				// ... il resto del tuo layout originale ...
				});

				ctx. set_visuals( egui:: Visuals:: dark());

				while let Ok( v) = self. jitter_rx. try_recv() {
					if v < 0.0 {
						self. latency_history. clear();
					} else {
						self. latency_history. push_back( v);
						if self. latency_history. len() > 300 { self. latency_history. pop_front(); }
					}
				}

			egui:: CentralPanel:: default(). show( ctx, |ui| {

            ui.vertical_centered(|ui| {
                ui.add(egui::Image::new(egui::include_image!("../logo.png")).max_width(240.0));
                ui.add_space(5.0);
                ui.heading(egui::RichText::new("AudioTX PRO").strong().color(egui::Color32::LIGHT_BLUE));
            });
            ui.add_space(10.0);

            if !self.is_licensed {
                ui.group(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.label("🔒 ATTIVAZIONE RICHIESTA");
                        if ui.button(format!("HWID: {}", self.hwid)).clicked() { ui.output_mut(|o| o.copied_text = self.hwid.clone()); }
                        ui.horizontal(|ui| { ui.label("Cliente:"); ui.text_edit_singleline(&mut self.customer_name); });
                        ui.horizontal(|ui| { ui.label("Chiave:"); ui.text_edit_singleline(&mut self.license_key); });
                        if ui.button("ATTIVA").clicked() { 
                            if self.verify_license() { self.is_licensed = true; let _ = self.command_tx.send(AudioCommand::UpdateAuth(true)); self.force_save(); }
                        }
                    });
                });
            } else {
				ui.columns(3, |cols| {
					cols[0].vertical_centered(|ui| { ui.selectable_value(&mut self.selected_tab, Tab::TX, "📤 TX"); });
					cols[1].vertical_centered(|ui| { ui.selectable_value(&mut self.selected_tab, Tab::RX, "📥 RX"); });
					cols[2].vertical_centered(|ui| { ui.selectable_value(&mut self.selected_tab, Tab::VPN, "🔒 VPN"); }); // <--- Terzo tasto
				});

				ui.add_space(10.0);

				match self.selected_tab {
					Tab::TX => self.show_tx(ui),
					Tab::RX => self.show_rx(ui),
					Tab::VPN => self.show_vpn(ui),
				}
            }
        });
        ctx.request_repaint();
    }
}

#[cfg(feature = "gui")]
impl AudioApp {
        pub fn force_save(&mut self) {
        let cfg = Config {
            target_ip: self.target_ip.clone(),
            local_port: self.local_port,
            buffer_size_ms: self.buffer_size_ms,
            selected_tab: self.selected_tab,
            in_volume: f32::from_bits(self.in_volume.load(std::sync::atomic::Ordering::Relaxed)),
            out_volume: f32::from_bits(self.out_volume.load(std::sync::atomic::Ordering::Relaxed)),
            selected_in: self.selected_in.clone(),
            selected_out: self.selected_out.clone(),
            bitrate_kbps: self.bitrate_kbps,
            customer_name: self.customer_name.clone(),
            license_key: self.license_key.clone(),
            mode: self.mode.clone(),
            autostart: self.autostart,
            // --- BLOCCO PERSISTENZA RIGIDA VPN ---
            vpn_enabled: self.vpn_enabled,
            vpn_private_key: self.vpn_private_key.clone(),
            vpn_peer_public_key: self.vpn_peer_public_key.clone(),
            vpn_endpoint: self.vpn_endpoint.clone(),
            vpn_local_ip: self.vpn_local_ip.clone(),
            vpn_allowed_ips: self.vpn_allowed_ips.clone(),
        };

        // Scrittura immediata, sincrona e bloccante sul file system dell'host
        if let Ok(json_str) = serde_json::to_string_pretty(&cfg) {
            if let Ok(mut file) = std::fs::File::create("config.json") {
                use std::io::Write;
                let _ = file.write_all(json_str.as_bytes());
                let _ = file.sync_all(); // Forza il kernel di Linux a svuotare i buffer della cache sul disco
            }
        }
    }


    pub fn verify_license(&self) -> bool {
        if self.customer_name.is_empty() || self.license_key.is_empty() { return false; }
        let mut hasher = Sha256::new();
        hasher.update(self.hwid.trim().to_uppercase().as_bytes());
        hasher.update(self.customer_name.trim().as_bytes());
        hasher.update(b"MmcyS5isfQKEdMPnn3F6N1n4tFLbtjPjPvip6spNjg"); 
        let expected = format!("{:x}", hasher.finalize())[..16].to_uppercase();
        self.license_key.trim() == expected
    }
    pub fn show_vpn(&mut self, ui: &mut egui::Ui) {
        // 1. GESTIONE REPAINT DINAMICO E RICHIESTA REFRESH COERENTE
        // Forza egui a ridisegnare lo schermo a 60 FPS se la VPN è attiva o in handshake
        let stato_attuale = self.vpn_status.load(std::sync::atomic::Ordering::Relaxed);
        if stato_attuale == 1 || stato_attuale == 2 {
            ui.ctx().request_repaint();
        }

        // --- INTERRUTTORE TUNNEL WIREGUARD REATTIVO ---
        if ui.checkbox(&mut self.vpn_enabled, "ATTIVA TUNNEL WIREGUARD").changed() {
            // Svuota preventivamente i log storici per accogliere la nuova transazione in RAM
            if let Ok(mut logs) = self.vpn_log_buffer.lock() {
                logs.clear();
                if self.vpn_enabled {
                    logs.push("🔄 [GUI] Inviato comando di attivazione tunnel...".to_string());
                } else {
                    logs.push("⚪ [GUI] Inviato comando di smantellamento tunnel.".to_string());
                }
            }
            // Invia l'impulso nel canale per distruggere/creare l'istanza boringtun
            let _ = self.command_tx.send(AudioCommand::UpdateVpnEnabled(self.vpn_enabled));
            self.force_save();
            ui.ctx().request_repaint();
        }

        ui.add_space(15.0);

        // --- GRIGLIA IMPOSTAZIONI RETE RETTIFICATA ---
        let mut testo_modificato = false;
        egui::Grid::new("vpn_settings_grid")
            .num_columns(2)
            .spacing([10.0, 15.0])
            .show(ui, |ui| {
                ui.label("Endpoint Server:");
                let r1 = ui.add(egui::TextEdit::singleline(&mut self.vpn_endpoint).desired_width(250.0));
                if r1.changed() { testo_modificato = true; }
                ui.end_row();

                ui.label("IP Locale Tunnel:");
                let r2 = ui.add(egui::TextEdit::singleline(&mut self.vpn_local_ip).desired_width(250.0));
                if r2.changed() { testo_modificato = true; }
                ui.end_row();
                
                ui.label("VPN Allowed IPs:");
				let r5 = ui.add(egui::TextEdit::singleline(&mut self.vpn_allowed_ips)
					.hint_text("Es: 10.0.0.0/24")
					.desired_width(250.0));
				if r5.changed() { testo_modificato = true; }
				ui.end_row();

                ui.label("Chiave Pubblica Server:");
                let r3 = ui.add(egui::TextEdit::singleline(&mut self.vpn_peer_public_key).desired_width(250.0));
                if r3.changed() { testo_modificato = true; }
                ui.end_row();

                ui.label("Chiave Privata Locale:");
                let r4 = ui.add(egui::TextEdit::singleline(&mut self.vpn_private_key).password(true).desired_width(250.0));
                if r4.changed() { testo_modificato = true; }
                ui.end_row();
            });

        // Se l'utente modifica i testi, salviamo e notifichiamo l'engine audio per prevenire cache orfane
        if testo_modificato {
            self.force_save();
            // Forza il coordinamento crittografico inviando lo stato aggiornato
            let _ = self.command_tx.send(AudioCommand::UpdateVpnEnabled(self.vpn_enabled));
        }

        ui.add_space(20.0);
        ui.separator();
        ui.add_space(10.0);

        // --- CALCOLO CHIAVE PUBBLICA VIA BORINGTUN IN RAM ---
        ui.label(egui::RichText::new("🔑 CHIAVE PUBBLICA DA AUTORIZZARE SUL SERVER:").strong());
        
        let chiave_pubblica_calcolata = if !self.vpn_private_key.is_empty() {
            use base64::{Engine as _, engine::general_purpose};
            if let Ok(dec_bytes) = general_purpose::STANDARD.decode(self.vpn_private_key.trim()) {
                if dec_bytes.len() == 32 {
                    let mut priv_arr = [0u8; 32];
                    priv_arr.copy_from_slice(&dec_bytes);
                    let priv_k: boringtun::x25519::StaticSecret = priv_arr.into();
                    let pub_k = boringtun::x25519::PublicKey::from(&priv_k);
                    general_purpose::STANDARD.encode(pub_k.as_ref())
                } else { "Chiave privata non valida (lunghezza errata)...".into() }
            } else { "Chiave privata non in formato Base64 valido...".into() }
        } else {
            "Chiave privata non impostata...".into()
        };

        // --- BOX EXPOSURE CHIAVE PUBBLICA ---
        ui.horizontal(|ui| {
            ui.set_row_height(30.0);
            let width_disponibile = ui.available_width() - 80.0;
            
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(width_disponibile);
                ui.label(egui::RichText::new(&chiave_pubblica_calcolata).code().color(egui::Color32::YELLOW));
            });

            if !chiave_pubblica_calcolata.starts_with("Chiave") {
                if ui.button("📋 Copia").clicked() {
                    ui.output_mut(|o| o.copied_text = chiave_pubblica_calcolata.clone());
                }
            }
        });
        
        ui.add_space(15.0);
        ui.separator();
        ui.add_space(10.0);

        // --- RETTANGOLO DI DIAGNOSTICA TERMINALE COERENTE ANCORATO ---
        ui.label(egui::RichText::new("📋 LOG E DIAGNOSTICA DI CONNESSIONE VPN:").strong());
        ui.add_space(5.0);

        // Calcola lo spazio verticale rimasto libero fino al bordo inferiore del form
        let spazio_residuo_vpn = ui.available_size();
        let larghezza_box_dinamica = spazio_residuo_vpn.x;
        let altezza_box_dinamica = (spazio_residuo_vpn.y - 10.0).max(120.0);

        egui::Frame::canvas(ui.style())
            .fill(egui::Color32::from_rgb(10, 10, 12))
            .inner_margin(8.0) 
            .show(ui, |ui| {
                ui.set_min_size(egui::vec2(larghezza_box_dinamica, altezza_box_dinamica));
                ui.set_max_size(egui::vec2(larghezza_box_dinamica, altezza_box_dinamica));

                if let Ok(logs) = self.vpn_log_buffer.lock() {
                    if logs.is_empty() {
                        ui.centered_and_justified(|ui| {
                            ui.label(
                                egui::RichText::new("Nessun evento registrato in RAM. Attiva la VPN per avviare il tracciamento...")
                                    .font(egui::FontId::proportional(11.0))
                                    .color(egui::Color32::DARK_GRAY)
                            );
                        });
                    } else {
                        egui::ScrollArea::vertical()
                            .id_source("vpn_terminal_scroll")
                            .max_height(ui.available_height())
                            .stick_to_bottom(true) 
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                
                                for log_line in logs.iter() {
                                    let colore_riga = if log_line.contains("❌") {
                                        egui::Color32::from_rgb(240, 100, 100)
                                    } else if log_line.contains("🔄") || log_line.contains("🔒") || log_line.contains("🟢") {
                                        egui::Color32::from_rgb(100, 240, 100)
                                    } else {
                                        egui::Color32::from_rgb(180, 180, 190)
                                    };

                                    ui.label(
                                        egui::RichText::new(log_line)
                                            .font(egui::FontId::monospace(10.0))
                                            .color(colore_riga)
                                    );
                                }
                            });
                    }
                }
            });
    }


    pub fn show_tx(&mut self, ui: &mut egui::Ui) {
        // Contenitore unificato con scroll verticale per evitare ritagli grafici
        egui::ScrollArea::vertical().id_source("tx_scroll_area").show(ui, |ui| {
            ui.vertical(|ui| {
                let larghezza_utile_form = (ui.available_width() - 20.0).max(200.0);
                let slider_target_width = (larghezza_utile_form - 160.0).max(150.0);

                let (txt, col) = if self.is_transmitting { 
                    ("🔴 STOP TX", egui::Color32::from_rgb(150, 0, 0)) 
                } else { 
                    ("🟢 START TX", egui::Color32::from_rgb(0, 120, 0)) 
                };
                
                if ui.add_sized([ui.available_width(), 40.0], egui::Button::new(txt).fill(col)).clicked() {
                    self.is_transmitting = !self.is_transmitting;
                    let _ = self.command_tx.send(AudioCommand::SetTransmitting(self.is_transmitting));
                }
                ui.add_space(10.0);
                
                if ui.checkbox(&mut self.autostart, "Avvio automatico trasmissione al boot").changed() {
                    self.force_save();
                }
                ui.add_space(10.0);

                ui.add_enabled_ui(!self.is_transmitting, |ui| {
                    ui.group(|ui| {
                        egui::Grid::new("tx_hardware_grid")
                            .num_columns(2)
                            .spacing([15.0, 15.0])
                            .show(ui, |ui| {
                                ui.label("Ingresso Audio (TX):");
                                egui::ComboBox::from_id_source("combo_in")
                                    .selected_text(&self.selected_in)
                                    .width(slider_target_width)
                                    .show_ui(ui, |ui| {
                                        // SOLUZIONE BORROW CHECKER: Clona il vettore locale per rilasciare self
                                        let dispositivi_input = self.input_devices.clone();
                                        for d in dispositivi_input { 
                                            if ui.selectable_value(&mut self.selected_in, d.clone(), &d).clicked() { 
                                                let _ = self.command_tx.send(AudioCommand::UpdateInputDevice(self.selected_in.clone())); 
                                                self.force_save(); 
                                            }
                                        }
                                    });
                                ui.end_row();

                                ui.label("Bitrate Audio:");
                                egui::ComboBox::from_id_source("combo_bitrate")
                                    .selected_text(format!("{}k", self.bitrate_kbps))
                                    .width(slider_target_width)
                                    .show_ui(ui, |ui| {
                                        for &r in &[64, 96, 128, 192, 256, 320] { 
                                            if ui.selectable_value(&mut self.bitrate_kbps, r, format!("{}k", r)).clicked() { 
                                                let _ = self.command_tx.send(AudioCommand::UpdateBitrate(r)); 
                                                self.force_save(); 
                                            }
                                        }
                                    });
                                ui.end_row();
                                    
                                ui.label("Destinazione Remota:");
                                let edit_ip = ui.add(egui::TextEdit::singleline(&mut self.target_ip).desired_width(slider_target_width));
                                if edit_ip.changed() {
                                    let _ = self.command_tx.send(AudioCommand::UpdateTarget(self.target_ip.clone()));
                                    self.force_save();
                                }
                                ui.end_row();
                            });
                    });
                });

                ui.add_space(10.0);

                ui.group(|ui| {
                    ui.label(egui::RichText::new(format!("🚀 TX Telemetria: {} kbps", self.tx_kbps.load(std::sync::atomic::Ordering::Relaxed))).strong());
                    ui.add_space(5.0);
                    
                    ui.horizontal(|ui| {
                        ui.label("Mic Gain:");
                        let mut v = f32::from_bits(self.in_volume.load(std::sync::atomic::Ordering::Relaxed));
                        ui.spacing_mut().slider_width = slider_target_width;
                        if ui.add(egui::Slider::new(&mut v, 0.0..=2.0)).changed() { 
                            self.in_volume.store(v.to_bits(), std::sync::atomic::Ordering::Relaxed); 
                            self.force_save(); 
                        }
                    });
                    ui.add_space(10.0);
                    
                    // Progress bar Stereo L/R giustificate in larghezza
                    let labels = ["L:", "R:"];
                    for i in 0..2 { 
                        ui.horizontal(|ui| {
                            ui.label(labels[i]);
                            ui.add(egui::ProgressBar::new(f32::from_bits(self.input_levels[i].load(std::sync::atomic::Ordering::Relaxed)).min(1.0))
                                .desired_width(larghezza_utile_form - 40.0)
                                .show_percentage()); 
                        });
                        ui.add_space(2.0);
                    }
                });
                
                ui.add_space(10.0);
                self.show_info_box(ui);
            });
        });
    }

        pub fn show_rx(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_source("rx_scroll_area").show(ui, |ui| {
            ui.vertical(|ui| {
                let larghezza_utile_form = (ui.available_width() - 20.0).max(200.0);
                let slider_target_width = (larghezza_utile_form - 160.0).max(150.0);

                ui.heading(egui::RichText::new("📥 STATO FLUSSO IN RICEZIONE (RX)").strong().color(egui::Color32::LIGHT_GREEN));
                ui.add_space(10.0);

                // --- 1. CONTROLLI HARDWARE E BUFFER RX ALLINEATI ESPANSI ---
                egui::Grid::new("rx_hardware_grid")
                    .num_columns(2)
                    .spacing([15.0, 15.0])
                    .show(ui, |ui| {
                        ui.label("Uscita Audio (RX):");
                        egui::ComboBox::from_id_source("combo_out")
                            .selected_text(&self.selected_out)
                            .width(slider_target_width)
                            .show_ui(ui, |ui| {
                                // SOLUZIONE BORROW CHECKER: Clona il vettore locale per rilasciare self
                                let dispositivi_out = self.output_devices.clone();
                                for dev in dispositivi_out {
                                    if ui.selectable_value(&mut self.selected_out, dev.clone(), &dev).clicked() {
                                        let _ = self.command_tx.send(AudioCommand::UpdateOutputDevice(self.selected_out.clone()));
                                        self.force_save();
                                    }
                                }
                            });
                        ui.end_row();

                        ui.label("Volume Cuffie:");
                        let mut vol_out_f32 = f32::from_bits(self.out_volume.load(std::sync::atomic::Ordering::Relaxed)) * 100.0;
                        ui.spacing_mut().slider_width = slider_target_width;
                        let slider_vol = egui::Slider::new(&mut vol_out_f32, 0.0..=100.0).text("%");
                        if ui.add(slider_vol).changed() {
                            let v_bits = (vol_out_f32 / 100.0).to_bits();
                            self.out_volume.store(v_bits, std::sync::atomic::Ordering::Relaxed);
                            self.force_save();
                        }
                        ui.end_row();

                        ui.label("Buffer Ritardo:");
                        ui.spacing_mut().slider_width = slider_target_width;
                        let slider_buf = egui::Slider::new(&mut self.buffer_size_ms, 20..=10000).step_by(20.0).text("ms");
                        if ui.add(slider_buf).changed() {
                            let _ = self.command_tx.send(AudioCommand::UpdateBufferSize(self.buffer_size_ms));
                            self.force_save();
                        }
                        ui.end_row();

                        ui.label("Porta UDP Locale:");
                        let mut port_str = self.local_port.to_string();
                        if ui.add(egui::TextEdit::singleline(&mut port_str).desired_width(100.0)).changed() {
                            if let Ok(p) = port_str.parse::<u16>() {
                                self.local_port = p;
                                let _ = self.command_tx.send(AudioCommand::UpdateLocalPort(p));
                                self.force_save();
                            }
                        }
                        ui.end_row();
                    });

                ui.add_space(15.0);
                ui.separator();
                ui.add_space(10.0);

                // --- 2. TELEMETRIA DI RETE ED IP SORGENTE ---
                let banda_rx = self.rx_kbps.load(std::sync::atomic::Ordering::Relaxed);
                let provenienza = self.remote_addr.lock().unwrap().clone();
                
                ui.horizontal(|ui| {
                    ui.set_width(larghezza_utile_form);
                    ui.label(egui::RichText::new("Banda Passante:").strong());
                    ui.label(format!("{} kbps", banda_rx));
                    ui.add_space(20.0);
                    ui.label(egui::RichText::new("Sorgente Remota:").strong());
                    ui.label(format!("{}", provenienza));
                });

                ui.add_space(10.0);

                // --- 3. VU-METER STEREO DELLA RICEZIONE (L/R) INDIPENDENTI ---
                let lvl_l = f32::from_bits(self.output_levels[0].load(std::sync::atomic::Ordering::Relaxed));
                let lvl_r = f32::from_bits(self.output_levels[1].load(std::sync::atomic::Ordering::Relaxed));

                ui.horizontal(|ui| {
                    ui.set_width(larghezza_utile_form);
                    ui.label("L:");
                    ui.add(egui::ProgressBar::new(lvl_l.clamp(0.0, 1.0))
                        .desired_width(larghezza_utile_form - 40.0)
                        .show_percentage());
                });
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.set_width(larghezza_utile_form);
                    ui.label("R:");
                    ui.add(egui::ProgressBar::new(lvl_r.clamp(0.0, 1.0))
                        .desired_width(larghezza_utile_form - 40.0)
                        .show_percentage());
                });

                ui.add_space(15.0);
                ui.separator();
                ui.add_space(5.0);

                // --- 4. GRAFICO CON LARGHEZZA CORRETTA ED ALTEZZA AUTOADATTIVA RESIDUA ---
                ui.horizontal(|ui| {
                    ui.set_width(larghezza_utile_form);
                    ui.label(egui::RichText::new("📊 MONITOR JITTER BUFFER DI RETE").strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("🗑️ Pulisci").clicked() { self.latency_history.clear(); }
                    });
                });
                ui.add_space(5.0);

                let spazio_residuo = ui.available_size();
                let larghezza_grafico_dinamica = (spazio_residuo.x - 10.0).max(100.0);
                let altezza_grafico_dinamica = (spazio_residuo.y - 15.0).max(120.0);

                egui::Frame::canvas(ui.style()).show(ui, |ui| {
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(larghezza_grafico_dinamica, altezza_grafico_dinamica), 
                        egui::Sense::hover()
                    );

                    ui.painter().rect_filled(rect, 4.0, egui::Color32::from_rgb(12, 12, 18));

                    // Tracciamento scale graduate allineate (Giallo)
                    let scale_valori = [
                        (0.00, "0 ms (Fondo)"),
                        (0.25, "25 ms"),
                        (0.50, "50 ms"),
                        (0.75, "75 ms"),
                        (1.00, "100 ms (Cima)"),
                    ];

                    for &(quota, testo) in &scale_valori {
                        let grid_y = rect.bottom() - (quota * altezza_grafico_dinamica);
                        
                        if quota > 0.0 && quota < 1.0 {
                            ui.painter().line_segment(
                                [egui::pos2(rect.left(), grid_y), egui::pos2(rect.right(), grid_y)],
                                egui::Stroke::new(0.5, egui::Color32::from_white_alpha(20)),
                            );
                        }

                        ui.painter().text(
                            egui::pos2(rect.left() + 8.0, grid_y - 2.0),
                            egui::Align2::LEFT_BOTTOM,
                            testo,
                            egui::FontId::proportional(10.0),
                            egui::Color32::from_rgb(220, 220, 100),
                        );
                    }

                    // Disegno delle linee oscilloscopio (Ciano)
                    let len = self.latency_history.len();
                    if len > 1 {
                        let points: Vec<egui::Pos2> = self.latency_history
                            .iter()
                            .enumerate()
                            .map(|(idx, &ms)| {
                                let x = rect.left() + (idx as f32 / (len - 1) as f32) * larghezza_grafico_dinamica;
                                let y = rect.bottom() - (ms as f32 / 100.0).clamp(0.0, 1.0) * altezza_grafico_dinamica;
                                egui::pos2(x, y)
                            })
                            .collect();

                        for window in points.windows(2) {
                            ui.painter().line_segment(
                                [window[0], window[1]],
                                egui::Stroke::new(1.5, egui::Color32::from_rgb(0, 240, 210)),
                            );
                        }
                    } else {
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "In attesa di traffico dati UDP per agganciare il grafico dinamico...",
                            egui::FontId::proportional(12.0),
                            egui::Color32::DARK_GRAY,
                        );
                    }
                });
            });
        });
    }


    fn show_latency_plot(&self, ui: &mut egui::Ui) {
        let h = ui.available_height() - 100.0;
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("NETWORK STABILITY").strong().small());
            let pts: PlotPoints = self.latency_history.iter().enumerate().map(|(i, &v)| [i as f64, v]).collect();
            Plot::new("j").height(h.max(60.0)).width(ui.available_width() * 0.95).allow_drag(false).include_y(0.0).include_y(100.0).show(ui, |p| p.line(Line::new(pts).color(egui::Color32::from_rgb(0, 162, 255)).fill(0.1)));
        });
    }

    fn show_info_box(&self, ui: &mut egui::Ui) {
        ui.add_space(20.0);
        ui.group(|ui| { ui.vertical_centered(|ui| { ui.label(egui::RichText::new("AudioTX Pro").strong()); ui.label("Release 2024.1.0"); ui.label(format!("Cliente: {}", self.customer_name)); }); });
    }
}
