//! Preferences ▸ Interface font: a face the user chose, from a file or from the installed faces,
//! put first in the interface's text families, and a separate face for Chinese, Japanese and
//! Korean text.
//!
//! The chosen file is untrusted input. It is read only up to [`MAX_FONT_BYTES`], and it must parse
//! with the same skrifa egui parses fonts with, at the chosen face, and map a probe character
//! ([`Slot::probe`]). Otherwise it is not installed — egui would abort on a face it cannot parse —
//! and the interface keeps the default faces ([`Problem`] says why). The default faces stay
//! behind the chosen ones, so a character the chosen face lacks is still drawn by them.
//!
//! Nothing here is embedded or shipped: the face is read at runtime from the user's own file
//! (AGENTS.md §1.4). On the web there are no font paths, so no choice is installed there.

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};
use serde::{Deserialize, Serialize};

/// Larger font files are not read. Chinese collections are big (Microsoft YaHei is about 20 MB,
/// a single-weight Source Han Sans about 17 MB, PingFang over 70 MB), so the cap is generous, but a
/// path is still untrusted input.
pub const MAX_FONT_BYTES: u64 = 96 << 20;
/// Faces tried in a collection (`.ttc`, `.otc`) when looking one up by name.
const MAX_FACES: u32 = 32;
/// A saved path or name longer than this is not a setting this app wrote.
pub const MAX_SETTING_CHARS: usize = 4096;
/// A displayed font name is cut to this many characters.
const MAX_NAME_CHARS: usize = 80;

/// The font-data name of the chosen interface face.
pub const UI_FACE: &str = "user-interface";
/// The font-data name of the chosen Chinese, Japanese and Korean face.
pub const CJK_FACE: &str = "user-cjk";

/// The faces that start a family before the chosen CJK face goes in: the chosen interface face
/// and the embedded Latin faces.
const LEADING_FACES: [&str; 5] = [UI_FACE, "Inter", "Inter-Medium", "Inter-SemiBold", "JetBrainsMono"];

/// One chosen face: a font file and the face in it (0 unless it is a collection).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FontChoice {
    pub path: String,
    #[serde(default)]
    pub index: u32,
    /// What Preferences shows for it: the face's family name, or the file's name.
    #[serde(default)]
    pub name: String,
}

impl FontChoice {
    /// A saved choice, or `None` when it isn't one this app could have written (settings are
    /// untrusted): no path, a path or name too long to be real, or a face index past
    /// [`MAX_FACES`].
    pub fn from_json(v: &serde_json::Value) -> Option<Self> {
        let mut choice = serde_json::from_value::<FontChoice>(v.clone()).ok()?;
        if choice.path.trim().is_empty() || choice.path.chars().count() > MAX_SETTING_CHARS || choice.index >= MAX_FACES {
            return None;
        }
        choice.name = display_name(&choice.name, &choice.path);
        Some(choice)
    }

    /// The choice for `path`'s first face, named after the file until the face is read.
    pub fn file(path: &str) -> Self {
        Self { path: path.to_owned(), index: 0, name: display_name("", path) }
    }
}

/// `name` cut to [`MAX_NAME_CHARS`], or the file name of `path` when `name` is blank.
fn display_name(name: &str, path: &str) -> String {
    let name = name.trim();
    let name = if name.is_empty() {
        // Both separators: a Windows path in settings read on another system still names the file.
        path.rsplit(['/', '\\']).next().unwrap_or(path)
    } else {
        name
    };
    name.chars().filter(|c| !c.is_control()).take(MAX_NAME_CHARS).collect()
}

/// Preferences ▸ Interface font and ▸ Chinese, Japanese and Korean font. `None` is the default.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InterfaceFonts {
    pub ui: Option<FontChoice>,
    pub cjk: Option<FontChoice>,
}

/// Which of the two choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// The interface face: first in every text family.
    Interface,
    /// The Chinese, Japanese and Korean face: right after the Latin faces.
    Cjk,
}

impl Slot {
    /// The character a face must map to be used for this slot: a Latin letter for the interface,
    /// 中 (U+4E2D, shared by Chinese, Japanese and Korean) for the CJK face.
    pub fn probe(self) -> char {
        match self {
            Self::Interface => 'a',
            Self::Cjk => '\u{4E2D}',
        }
    }
}

/// Why a chosen face is not used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The file is over [`MAX_FONT_BYTES`].
    TooLarge,
    /// The file couldn't be read (the system's reason).
    Unreadable(String),
    /// The file, or that face of it, is not a font skrifa can parse.
    NotAFont,
    /// The face has no glyph for [`Slot::probe`].
    MissingGlyphs(Slot),
    /// This build can't read font files (the web).
    Unavailable,
}

impl Problem {
    /// The reason, in the interface language, to go after "Couldn't use the font {name}:".
    pub fn message(&self) -> String {
        match self {
            Self::TooLarge => crate::i18n::fmt(tl!("the file is larger than {size} MB"), &[("size", &(MAX_FONT_BYTES >> 20).to_string())]),
            Self::Unreadable(e) => crate::i18n::fmt(tl!("the file couldn't be read ({e})"), &[("e", e)]),
            Self::NotAFont => tl!("the file is not a font that can be used").to_owned(),
            Self::MissingGlyphs(Slot::Interface) => tl!("it has no Latin letters").to_owned(),
            Self::MissingGlyphs(Slot::Cjk) => tl!("it has no Chinese, Japanese or Korean characters").to_owned(),
            Self::Unavailable => tl!("fonts can't be read from files here").to_owned(),
        }
    }
}

/// Face `index` of `bytes` as interface font data, after checking that egui will be able to parse
/// it and that it maps `slot`'s probe. Never panics, whatever the bytes.
pub fn parse(bytes: Vec<u8>, index: u32, slot: Slot) -> Result<FontData, Problem> {
    use skrifa::MetadataProvider as _;
    if bytes.len() as u64 > MAX_FONT_BYTES {
        return Err(Problem::TooLarge);
    }
    {
        // The same call epaint makes to install a face (`FontFace::new`): if it fails there, the
        // app aborts, so it must not get that far.
        let font = skrifa::FontRef::from_index(&bytes, index).map_err(|_| Problem::NotAFont)?;
        let glyph = font.charmap().map(slot.probe()).ok_or(Problem::MissingGlyphs(slot))?;
        // skrifa reads tables lazily: a cut-off file parses, so ask for the probe's outline too.
        if font.outline_glyphs().get(glyph).is_none() {
            return Err(Problem::NotAFont);
        }
    }
    let mut data = FontData::from_owned(bytes);
    data.index = index;
    Ok(match slot {
        // First in its families: placed by its own metrics.
        Slot::Interface => data,
        // A fallback behind the Latin faces: on their baseline, like every other fallback.
        Slot::Cjk => super::on_interface_baseline(data),
    })
}

/// The face of `bytes` whose family name is `family` (any language), if a face of it has one.
pub fn face_named(bytes: &[u8], family: &str) -> Option<u32> {
    (0..MAX_FACES).find(|&index| face_family(bytes, index).is_some_and(|names| names.iter().any(|n| n.eq_ignore_ascii_case(family))))
}

/// The family names face `index` of `bytes` gives itself, typographic family first.
fn face_family(bytes: &[u8], index: u32) -> Option<Vec<String>> {
    use skrifa::MetadataProvider as _;
    use skrifa::string::StringId;
    let font = skrifa::FontRef::from_index(bytes, index).ok()?;
    let names = [StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME]
        .into_iter()
        .flat_map(|id| font.localized_strings(id).map(|s| s.chars().take(MAX_NAME_CHARS).collect::<String>()).collect::<Vec<_>>())
        .filter(|n| !n.trim().is_empty())
        .collect();
    Some(names)
}

/// The English (or first) family name face `index` of `bytes` gives itself.
pub fn family_name(bytes: &[u8], index: u32) -> Option<String> {
    use skrifa::MetadataProvider as _;
    use skrifa::string::StringId;
    let font = skrifa::FontRef::from_index(bytes, index).ok()?;
    [StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME].into_iter().find_map(|id| {
        let name: String = font.localized_strings(id).english_or_first()?.chars().filter(|c| !c.is_control()).take(MAX_NAME_CHARS).collect();
        (!name.trim().is_empty()).then_some(name)
    })
}

/// The bytes of the file at `path`, read only when it is a regular file of at most
/// [`MAX_FONT_BYTES`]. The read itself stops past the cap too, in case the file grew after its
/// size was checked.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_capped(path: &std::path::Path) -> Result<Vec<u8>, Problem> {
    use std::io::Read as _;
    let meta = std::fs::metadata(path).map_err(|e| Problem::Unreadable(e.to_string()))?;
    if !meta.is_file() {
        return Err(Problem::NotAFont);
    }
    if meta.len() > MAX_FONT_BYTES {
        return Err(Problem::TooLarge);
    }
    let file = std::fs::File::open(path).map_err(|e| Problem::Unreadable(e.to_string()))?;
    let mut bytes = Vec::new();
    file.take(MAX_FONT_BYTES.saturating_add(1)).read_to_end(&mut bytes).map_err(|e| Problem::Unreadable(e.to_string()))?;
    if bytes.len() as u64 > MAX_FONT_BYTES {
        return Err(Problem::TooLarge);
    }
    Ok(bytes)
}

/// The font data for `choice`, read from its file.
pub fn load(choice: &FontChoice, slot: Slot) -> Result<Arc<FontData>, Problem> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let bytes = read_capped(std::path::Path::new(&choice.path))?;
        let data = parse(bytes, choice.index, slot)?;
        log::info!("interface font ({slot:?}): {} (face {})", choice.path, choice.index);
        Ok(Arc::new(data))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (choice, slot);
        Err(Problem::Unavailable)
    }
}

/// The choice of `path` for `slot`, after reading and checking it: the face whose family is
/// `family` when given and the file has one (an installed collection such as `msyh.ttc` holds
/// Microsoft YaHei and Microsoft YaHei UI), otherwise its first face, named by its family.
pub fn choose(path: &str, family: Option<&str>, slot: Slot) -> Result<FontChoice, Problem> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let bytes = read_capped(std::path::Path::new(path))?;
        let index = family.and_then(|f| face_named(&bytes, f)).unwrap_or(0);
        let name = family_name(&bytes, index).unwrap_or_default();
        parse(bytes, index, slot)?;
        Ok(FontChoice { path: path.to_owned(), index, name: display_name(&name, path) })
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (path, family, slot);
        Err(Problem::Unavailable)
    }
}

/// A chosen face that was not installed, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejected {
    pub slot: Slot,
    pub choice: FontChoice,
    pub problem: Problem,
}

/// Put the chosen faces into `fonts`:
/// - the interface face first in the Proportional, `medium` and `semibold` families, so the whole
///   interface uses it (code and other monospace text keep JetBrains Mono);
/// - the CJK face right after the leading Latin faces ([`LEADING_FACES`]) of every family,
///   before egui's defaults and every other fallback, so Chinese, Japanese and Korean text draws
///   in it while Latin text keeps its own face.
///
/// Every face that was there stays behind them, so a character they lack is drawn as before.
pub fn add_faces(fonts: &mut FontDefinitions, ui: Option<Arc<FontData>>, cjk: Option<Arc<FontData>>) {
    if let Some(data) = ui {
        fonts.font_data.insert(UI_FACE.to_owned(), data);
        for family in [FontFamily::Proportional, FontFamily::Name("medium".into()), FontFamily::Name("semibold".into())] {
            let stack = fonts.families.entry(family).or_default();
            stack.retain(|n| n != UI_FACE);
            stack.insert(0, UI_FACE.to_owned());
        }
    }
    if let Some(data) = cjk {
        fonts.font_data.insert(CJK_FACE.to_owned(), data);
        for stack in fonts.families.values_mut() {
            stack.retain(|n| n != CJK_FACE);
            let at = stack.iter().position(|n| !LEADING_FACES.contains(&n.as_str())).unwrap_or(stack.len());
            stack.insert(at, CJK_FACE.to_owned());
        }
    }
}

/// Load the chosen faces and add them to `fonts` ([`add_faces`]). A face that can't be used is
/// left out, so the default stays in its place, and is returned with the reason.
pub fn install(fonts: &mut FontDefinitions, choices: &InterfaceFonts) -> Vec<Rejected> {
    let mut rejected = Vec::new();
    let mut take = |choice: &Option<FontChoice>, slot: Slot| {
        let choice = choice.as_ref()?;
        match load(choice, slot) {
            Ok(data) => Some(data),
            Err(problem) => {
                log::warn!("interface font ({slot:?}) {} not used: {problem:?}", choice.path);
                rejected.push(Rejected { slot, choice: choice.clone(), problem });
                None
            }
        }
    };
    let ui = take(&choices.ui, Slot::Interface);
    let cjk = take(&choices.cjk, Slot::Cjk);
    add_faces(fonts, ui, cjk);
    rejected
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTER: &[u8] = include_bytes!("../../../../assets/fonts/Inter-Medium.ttf");
    const MONO: &[u8] = include_bytes!("../../../../assets/fonts/JetBrainsMono-Regular.ttf");

    fn medium() -> FontFamily {
        FontFamily::Name("medium".into())
    }

    fn semibold() -> FontFamily {
        FontFamily::Name("semibold".into())
    }

    #[test]
    fn garbage_truncated_and_empty_bytes_are_not_fonts() {
        for bytes in [Vec::new(), b"not a font at all".to_vec(), MONO[..MONO.len() / 3].to_vec(), vec![0u8; 4096]] {
            // Zeros read as a font with no tables: refused for lacking the glyph instead.
            let got = parse(bytes, 0, Slot::Interface);
            assert!(matches!(got, Err(Problem::NotAFont | Problem::MissingGlyphs(_))), "{got:?}");
        }
        // A face index past the end of a single-face file.
        assert!(matches!(parse(MONO.to_vec(), 3, Slot::Interface), Err(Problem::NotAFont)));
    }

    #[test]
    fn a_face_without_the_probe_is_refused_for_that_slot() {
        assert!(parse(MONO.to_vec(), 0, Slot::Interface).is_ok());
        assert_eq!(parse(MONO.to_vec(), 0, Slot::Cjk).err(), Some(Problem::MissingGlyphs(Slot::Cjk)));
    }

    #[test]
    fn oversized_bytes_are_refused_before_parsing() {
        let big = vec![0u8; usize::try_from(MAX_FONT_BYTES).unwrap_or(usize::MAX).saturating_add(1)];
        assert_eq!(parse(big, 0, Slot::Interface).err(), Some(Problem::TooLarge));
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn scratch(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("pdfcraft-interface-font-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn an_oversized_file_is_not_read() {
        let path = scratch("big.ttf", b"");
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        // Sparse: the length is over the cap without writing 96 MB.
        file.set_len(MAX_FONT_BYTES + 1).unwrap();
        assert_eq!(read_capped(&path).err(), Some(Problem::TooLarge));
        assert!(matches!(read_capped(&path.with_file_name("missing.ttf")), Err(Problem::Unreadable(_))));
        assert_eq!(read_capped(path.parent().unwrap()).err(), Some(Problem::NotAFont));
    }

    /// The bad file falls back: nothing of it is installed and the defaults are untouched.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_bad_font_file_leaves_the_default_fonts() {
        let bad = scratch("bad.ttf", b"\0\x01\0\0garbage that claims to be a TrueType font");
        let defaults = super::super::font_definitions();
        let mut fonts = defaults.clone();
        let choices = InterfaceFonts { ui: Some(FontChoice::file(&bad.to_string_lossy())), cjk: Some(FontChoice::file(&bad.to_string_lossy())) };
        let rejected = install(&mut fonts, &choices);
        assert_eq!(rejected.len(), 2);
        assert!(rejected.iter().all(|r| matches!(r.problem, Problem::NotAFont | Problem::MissingGlyphs(_))), "{rejected:?}");
        assert_eq!(fonts.families, defaults.families);
        assert!(!fonts.font_data.contains_key(UI_FACE) && !fonts.font_data.contains_key(CJK_FACE));
        // The installed set still builds in egui.
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts);
        ctx.run_ui(Default::default(), |ui| {
            ui.label("Interface 界面");
        })
        .textures_delta
        .clear();
    }

    /// A good file is first in the text families and every default face is still behind it.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_valid_font_file_goes_first_in_the_text_families() {
        let mono = scratch("mono.ttf", MONO);
        let defaults = super::super::font_definitions();
        let mut fonts = defaults.clone();
        let choices = InterfaceFonts { ui: Some(FontChoice::file(&mono.to_string_lossy())), cjk: None };
        assert!(install(&mut fonts, &choices).is_empty());
        for family in [FontFamily::Proportional, medium(), semibold()] {
            let stack = &fonts.families[&family];
            assert_eq!(stack.first().map(String::as_str), Some(UI_FACE), "{family:?}: {stack:?}");
            assert_eq!(&stack[1..], defaults.families[&family].as_slice(), "{family:?}");
        }
        assert_eq!(fonts.families[&FontFamily::Monospace], defaults.families[&FontFamily::Monospace]);
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts);
        ctx.run_ui(Default::default(), |ui| {
            ui.label("Interface");
        })
        .textures_delta
        .clear();
    }

    #[test]
    fn the_cjk_face_goes_after_the_latin_faces() {
        let defaults = super::super::font_definitions();
        let mut fonts = defaults.clone();
        let ui = Arc::new(FontData::from_static(INTER));
        let cjk = Arc::new(FontData::from_static(MONO));
        add_faces(&mut fonts, Some(ui), Some(cjk));
        let proportional = &fonts.families[&FontFamily::Proportional];
        assert_eq!(proportional.get(..3), Some(&[UI_FACE.to_owned(), "Inter".to_owned(), CJK_FACE.to_owned()][..]));
        let semibold = &fonts.families[&semibold()];
        assert_eq!(semibold.get(..4), Some(&[UI_FACE.to_owned(), "Inter-SemiBold".to_owned(), "Inter".to_owned(), CJK_FACE.to_owned()][..]));
        let mono = &fonts.families[&FontFamily::Monospace];
        assert_eq!(mono.get(..3), Some(&["JetBrainsMono".to_owned(), "Inter".to_owned(), CJK_FACE.to_owned()][..]));
        // Once each, even if added again.
        add_faces(&mut fonts, None, Some(Arc::new(FontData::from_static(MONO))));
        assert_eq!(fonts.families[&FontFamily::Proportional].iter().filter(|n| *n == CJK_FACE).count(), 1);
    }

    #[test]
    fn faces_are_found_and_named_by_family() {
        assert_eq!(family_name(MONO, 0).as_deref(), Some("JetBrains Mono"));
        assert_eq!(face_named(MONO, "jetbrains mono"), Some(0));
        assert_eq!(face_named(MONO, "Inter"), None);
        assert_eq!(family_name(b"nope", 0), None);
    }

    #[test]
    fn saved_choices_are_untrusted() {
        let ok = FontChoice::from_json(&serde_json::json!({"path": "C:\\Windows\\Fonts\\msyh.ttc", "index": 1})).unwrap();
        assert_eq!((ok.index, ok.name.as_str()), (1, "msyh.ttc"));
        assert!(FontChoice::from_json(&serde_json::json!({"path": ""})).is_none());
        assert!(FontChoice::from_json(&serde_json::json!({"path": "a.ttf", "index": 999})).is_none());
        assert!(FontChoice::from_json(&serde_json::json!({"path": "x".repeat(MAX_SETTING_CHARS + 1)})).is_none());
        assert!(FontChoice::from_json(&serde_json::json!("a.ttf")).is_none());
        assert!(FontChoice::from_json(&serde_json::Value::Null).is_none());
        let named = FontChoice::from_json(&serde_json::json!({"path": "a.ttf", "name": "N\u{7}ame".repeat(100)})).unwrap();
        assert_eq!(named.name.chars().count(), MAX_NAME_CHARS);
        assert!(!named.name.contains('\u{7}'));
    }
}
