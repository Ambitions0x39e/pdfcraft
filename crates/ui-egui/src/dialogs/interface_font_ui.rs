//! Preferences ▸ Interface font and ▸ Chinese, Japanese and Korean font (desktop): the default
//! faces, the well-known installed faces ([`crate::system_fonts::interface_faces`]) or any font
//! file, and Reset to default. The choice takes effect on the next frame, when
//! [`PdfCraftApp`] reinstalls the fonts; a face that can't be used is refused here, before it is
//! saved, and again there should the file change later.

use crate::PdfCraftApp;
use crate::theme::Tokens;
use crate::theme::interface_font::{self, FontChoice, Slot};

impl PdfCraftApp {
    /// Use the font file at `path` for `slot` (its face named `family`, when given and the file
    /// has one), or tell the user why it can't be used and keep what was chosen before.
    pub(crate) fn choose_interface_font(&mut self, slot: Slot, path: &str, family: Option<&str>) {
        match interface_font::choose(path, family, slot) {
            Ok(choice) => *self.interface_font_slot(slot) = Some(choice),
            Err(problem) => {
                let name = FontChoice::file(path).name;
                self.notify_fmt(
                    "Couldn't use the font {name}: {reason}. The default font is used instead.",
                    &[("name", &name), ("reason", &problem.message())],
                );
            }
        }
    }

    fn interface_font_slot(&mut self, slot: Slot) -> &mut Option<FontChoice> {
        match slot {
            Slot::Interface => &mut self.interface_fonts.ui,
            Slot::Cjk => &mut self.interface_fonts.cjk,
        }
    }

    /// Choose a font file for `slot` in the system's file dialog, starting in the font folder.
    fn pick_interface_font(&mut self, slot: Slot) {
        let mut dialog = rfd::AsyncFileDialog::new().add_filter(tl!("Fonts"), &["ttf", "otf", "ttc", "otc"]);
        if let Some(dir) = crate::system_fonts::font_folder() {
            dialog = dialog.set_directory(dir);
        }
        self.ask_one(crate::pickers::Ask::File(dialog), None, move |app, path| {
            app.choose_interface_font(slot, &path.to_string_lossy(), None);
        });
    }
}

/// What a font row asked for this frame.
enum Pick {
    Default,
    Installed(String, &'static str),
    File,
}

/// One font row: its label, a combo box of the default, the installed faces and Choose font
/// file…, and Reset to default while a face is chosen.
fn font_row(ui: &mut egui::Ui, app: &mut PdfCraftApp, slot: Slot) {
    let (title, id) = match slot {
        Slot::Interface => (tl!("Interface font"), "interface-font"),
        Slot::Cjk => (tl!("Chinese, Japanese and Korean font"), "interface-cjk-font"),
    };
    let current = app.interface_font_slot(slot).clone();
    let default = tl!("Default");
    let mut pick = None;
    ui.horizontal(|ui| {
        let label = ui.label(title);
        let selected = current.as_ref().map_or(default, |c| c.name.as_str());
        craft_ui::ComboBox::from_id_salt(id)
            .selected_text(selected)
            .show_ui(ui, |ui| {
                if ui.selectable_label(current.is_none(), default).clicked() {
                    pick = Some(Pick::Default);
                    ui.close();
                }
                for face in crate::system_fonts::interface_faces().iter().filter(|f| slot == Slot::Interface || f.cjk) {
                    let path = face.path.to_string_lossy();
                    let on = current.as_ref().is_some_and(|c| c.path == path && c.name.eq_ignore_ascii_case(face.family));
                    if ui.selectable_label(on, face.family).clicked() {
                        pick = Some(Pick::Installed(path.into_owned(), face.family));
                        ui.close();
                    }
                }
                if ui.selectable_label(false, tl!("Choose font file…")).clicked() {
                    pick = Some(Pick::File);
                    ui.close();
                }
            })
            .response
            .labelled_by(label.id);
        if current.is_some() && ui.button(tl!("Reset to default")).clicked() {
            pick = Some(Pick::Default);
        }
    });
    match pick {
        Some(Pick::Default) => *app.interface_font_slot(slot) = None,
        Some(Pick::Installed(path, family)) => app.choose_interface_font(slot, &path, Some(family)),
        Some(Pick::File) => app.pick_interface_font(slot),
        None => {}
    }
}

/// The two font rows of Preferences and what they do.
pub(crate) fn preferences_rows(ui: &mut egui::Ui, app: &mut PdfCraftApp, t: &Tokens) {
    font_row(ui, app, Slot::Interface);
    font_row(ui, app, Slot::Cjk);
    ui.label(
        egui::RichText::new(tl!("A chosen font draws the whole interface; characters it lacks are drawn with the default fonts."))
            .small()
            .color(t.text_muted),
    );
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;
    use crate::theme;

    fn scratch(name: &str, bytes: &[u8]) -> String {
        let dir = std::env::temp_dir().join(format!("pdfcraft-interface-font-ui-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path.to_string_lossy().into_owned()
    }

    /// A good file is chosen and named by its family; a bad one is refused with a notice and the
    /// earlier choice stays; Reset to default clears it.
    #[test]
    fn a_font_file_is_checked_before_it_is_chosen() {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        let good = scratch("mono.ttf", include_bytes!("../../../../assets/fonts/JetBrainsMono-Regular.ttf"));
        app.choose_interface_font(Slot::Interface, &good, None);
        let chosen = app.interface_fonts.ui.clone().unwrap();
        assert_eq!((chosen.path.as_str(), chosen.index, chosen.name.as_str()), (good.as_str(), 0, "JetBrains Mono"));

        let bad = scratch("bad.ttf", b"definitely not a font");
        app.choose_interface_font(Slot::Interface, &bad, None);
        assert_eq!(app.interface_fonts.ui.as_ref(), Some(&chosen));
        let toast = app.toast.as_ref().map(|(m, _)| m.clone()).unwrap_or_default();
        assert!(toast.contains("bad.ttf") && toast.contains("default font"), "{toast}");

        // JetBrains Mono has no Han characters, so it is no CJK face.
        app.choose_interface_font(Slot::Cjk, &good, None);
        assert_eq!(app.interface_fonts.cjk, None);

        let mut fonts = false;
        let mut h = Harness::builder().with_size(egui::vec2(640.0, 200.0)).build_ui_state(
            move |ui, app: &mut PdfCraftApp| {
                if !fonts {
                    ui.ctx().set_fonts(theme::font_definitions());
                    fonts = true;
                    return;
                }
                let t = Tokens::get(ui.ctx());
                preferences_rows(ui, app, &t);
            },
            app,
        );
        h.run();
        // One Reset button: only the interface font is chosen.
        assert_eq!(h.query_all_by_label("Reset to default").count(), 1);
        h.get_by_label("Reset to default").click();
        h.run();
        assert_eq!(h.state().interface_fonts.ui, None);
        assert_eq!(h.query_all_by_label("Reset to default").count(), 0);
    }

    /// The setting goes through the settings file and the control channel, and a file that later
    /// stops being a font falls back to the default when the fonts are installed.
    #[test]
    fn the_choice_persists_and_a_broken_file_falls_back_when_installed() {
        let good = scratch("persist.ttf", include_bytes!("../../../../assets/fonts/JetBrainsMono-Regular.ttf"));
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.set_option("interface-font", &good).unwrap();
        assert!(app.set_option("interface-cjk-font", &good).is_err(), "no Han characters");
        assert!(app.set_option("interface-font", &format!("{good}.missing")).is_err());
        assert!(app.set_option("interface-font", "").is_err());
        let saved = app.persist();

        let mut restored = PdfCraftApp::new();
        restored.restore(&saved);
        assert_eq!(restored.interface_fonts, app.interface_fonts);
        assert_eq!(restored.interface_fonts.ui.as_ref().map(|c| c.name.as_str()), Some("JetBrains Mono"));

        // The file is replaced by garbage before the next launch.
        std::fs::write(&good, b"garbage").unwrap();
        let ctx = egui::Context::default();
        restored.install_fonts(&ctx, theme::CjkPreference::Default);
        assert_eq!(restored.interface_fonts.ui, None, "falls back to the default");
        assert!(restored.toast.as_ref().is_some_and(|(m, _)| m.contains("JetBrains Mono")));
        ctx.run_ui(Default::default(), |ui| {
            ui.label("Interface");
        })
        .textures_delta
        .clear();

        restored.set_option("interface-font", "default").unwrap();
        assert_eq!(restored.interface_fonts.ui, None);
    }
}
