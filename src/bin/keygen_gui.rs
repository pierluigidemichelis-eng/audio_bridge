use eframe::egui;
use sha2::{Sha256, Digest};

fn main() -> Result<(), eframe::Error> {
    let mut options = eframe::NativeOptions::default();
    options.viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([450.0, 300.0])
        .with_resizable(false);

    eframe::run_native(
        "AudioTX PRO - License Generator",
        options,
        Box::new(|_cc| Box::new(KeygenApp::default())),
    )
}

struct KeygenApp {
    hwid: String,
    customer_name: String,
    generated_key: String,
}

impl Default for KeygenApp {
    fn default() -> Self {
        Self {
            hwid: "".into(),
            customer_name: "".into(),
            generated_key: "Inserisci i dati per generare...".into(),
        }
    }
}

impl eframe::App for KeygenApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.set_visuals(egui::Visuals::dark());

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(10.0);
                ui.heading(egui::RichText::new("🔑 KEYGEN AUDIOTX PRO").strong().color(egui::Color32::LIGHT_BLUE));
                ui.add_space(15.0);
            });

            egui::Grid::new("keygen_grid")
                .num_columns(2)
                .spacing([10.0, 15.0])
                .show(ui, |ui| {
                    ui.label("HWID Target:");
                    ui.add(egui::TextEdit::singleline(&mut self.hwid)
                        .hint_text("Es: 76A18EAF2DE8")
                        .desired_width(280.0));
                    ui.end_row();

                    ui.label("Nome Cliente:");
                    ui.add(egui::TextEdit::singleline(&mut self.customer_name)
                        .hint_text("Es: Raspberry 01")
                        .desired_width(280.0));
                    ui.end_row();
                });

            ui.add_space(20.0);

            ui.vertical_centered(|ui| {
                if ui.add_sized([200.0, 35.0], egui::Button::new("GENERA CHIAVE").fill(egui::Color32::from_rgb(0, 100, 150))).clicked() {
                    if !self.hwid.is_empty() && !self.customer_name.is_empty() {
                        let mut hasher = Sha256::new();
                        hasher.update(self.hwid.trim().to_uppercase().as_bytes());
                        hasher.update(self.customer_name.trim().as_bytes());
                        hasher.update(b"MmcyS5isfQKEdMPnn3F6N1n4tFLbtjPjPvip6spNjg");
                        
                        self.generated_key = format!("{:x}", hasher.finalize())[..16].to_uppercase();
                    } else {
                        self.generated_key = "ERRORE: Campi vuoti!".into();
                    }
                }

                ui.add_space(20.0);

                ui.group(|ui| {
                    ui.set_width(400.0);
                    ui.horizontal(|ui| {
                        ui.label("Licenza:");
                        ui.selectable_label(true, &self.generated_key);
                        if !self.generated_key.starts_with("Inserisci") && !self.generated_key.starts_with("ERRORE") {
                            if ui.button("📋 Copia").clicked() {
                                ui.output_mut(|o| o.copied_text = self.generated_key.clone());
                            }
                        }
                    });
                });
            });
        });
    }
}
