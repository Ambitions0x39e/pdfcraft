use super::{DocInfo, Nav, OutlineItem, Rect, Sense, Tokens, icons, pos2, theme, vec2};

/// What the user asked to do with a bookmark (applied after the panel is drawn).
#[derive(Clone, Debug, PartialEq)]
pub enum BmAction {
    /// New "Untitled" bookmark for the current page, after the selected one (⌘B).
    New,
    StartRename(Vec<usize>),
    Rename(Vec<usize>, String),
    SetToCurrentPage(Vec<usize>),
    /// Make the title bold or italic or not (`None` keeps that one).
    SetStyle(Vec<usize>, Option<bool>, Option<bool>),
    /// Colour the title; black is the default.
    SetColor(Vec<usize>, [f64; 3]),
    Delete(Vec<usize>),
    MoveUp(Vec<usize>),
    MoveDown(Vec<usize>),
    /// Make it the last child of the bookmark above it.
    Indent(Vec<usize>),
    /// Move it out to follow its parent.
    Outdent(Vec<usize>),
    /// Bookmarks from the tagged headings, under a new first "Untitled" bookmark.
    FromStructure,
    /// Open New bookmarks from text styles, to choose a style per bookmark level (#1693).
    FromTextStyles,
}

/// Matching titles plus their ancestors, keeping document paths rather than filtered indexes.
/// `query` is folded with [`pdfcraft_render::text::fold_case`], as titles are here.
pub(super) fn outline_matches(items: &[OutlineItem], query: &str) -> std::collections::HashSet<Vec<usize>> {
    let mut matches = std::collections::HashSet::new();
    if query.is_empty() {
        return matches;
    }
    let mut pending: Vec<_> = items.iter().enumerate().map(|(i, item)| (vec![i], item)).collect();
    while let Some((path, item)) = pending.pop() {
        if pdfcraft_render::text::fold_case(&item.title).contains(query) {
            let mut ancestor = path.clone();
            while !ancestor.is_empty() {
                matches.insert(ancestor.clone());
                ancestor.pop();
            }
        }
        for (i, child) in item.children.iter().enumerate() {
            let mut child_path = path.clone();
            child_path.push(i);
            pending.push((child_path, child));
        }
    }
    matches
}

/// Deeper than the panel ever lists bookmarks (33 levels), so the walk below is bounded even if a
/// caller hands it a deeper tree.
const MARK_MAX_DEPTH: usize = 64;

/// The bookmark that holds `page`: of the entries whose destination page is at most `page`, the
/// last in document order (so unsorted destinations still give one answer). Entries without a
/// destination page are skipped. When a parent of that entry is collapsed (`open` says whether a
/// bookmark's children are shown), the nearest shown ancestor is returned instead. `open` is
/// asked only about that entry's ancestors, top-down.
pub(super) fn current_bookmark(
    items: &[OutlineItem],
    page: usize,
    mut open: impl FnMut(&[usize], &OutlineItem) -> bool,
) -> Option<Vec<usize>> {
    let mut best: Option<Vec<usize>> = None;
    let mut path: Vec<usize> = Vec::new();
    let mut stack = vec![items.iter().enumerate()];
    while let Some(level) = stack.last_mut() {
        let Some((i, item)) = level.next() else {
            stack.pop();
            continue;
        };
        path.truncate(stack.len().saturating_sub(1));
        path.push(i);
        if item.page.is_some_and(|p| p <= page) {
            best = Some(path.clone());
        }
        if !item.children.is_empty() && stack.len() < MARK_MAX_DEPTH {
            stack.push(item.children.iter().enumerate());
        }
    }
    let mut best = best?;
    let mut siblings = items;
    for depth in 0..best.len().saturating_sub(1) {
        let Some(item) = best.get(depth).and_then(|&i| siblings.get(i)) else { break };
        if !open(best.get(..=depth).unwrap_or_default(), item) {
            best.truncate(depth + 1);
            break;
        }
        siblings = &item.children;
    }
    Some(best)
}

/// The row id holding a bookmark's open state; [`outline_item`] and [`marked_bookmark`] share it.
fn row_id(ui: &egui::Ui, path: &[usize]) -> egui::Id {
    ui.scope_id().with(("outline", path))
}

/// Whether a bookmark's children are shown this frame: Expand/Collapse all, else the reader's
/// toggle, else the file's /Count. A search shows every match.
fn row_open(ui: &egui::Ui, item: &OutlineItem, path: &[usize], expand: Option<usize>, searching: bool) -> bool {
    if searching {
        return true;
    }
    if let Some(levels) = expand {
        return path.len().saturating_sub(1) < levels;
    }
    ui.data(|d| d.get_temp::<bool>(row_id(ui, path))).unwrap_or(item.open)
}

/// The row to mark as the reader's place in the panel's `ui` (see [`current_bookmark`]), and
/// whether the panel should scroll it into view: only when the mark moved and the pointer is
/// not over the panel, so a reader working in the panel is never scrolled away.
pub(super) fn marked_bookmark(
    ui: &egui::Ui,
    items: &[OutlineItem],
    current: usize,
    expand: Option<usize>,
    searching: bool,
    matches: &std::collections::HashSet<Vec<usize>>,
) -> (Option<Vec<usize>>, bool) {
    let mut marked = current_bookmark(items, current, |path, item| row_open(ui, item, path, expand, searching));
    if searching && let Some(path) = marked.as_mut() {
        // A search hides what doesn't match; its matches keep their ancestors, so the longest
        // shown prefix is the nearest shown row.
        while !path.is_empty() && !matches.contains(path.as_slice()) {
            path.pop();
        }
        if path.is_empty() {
            marked = None;
        }
    }
    let last_id = ui.scope_id().with("outline-marked");
    let moved = ui.data(|d| d.get_temp::<Option<Vec<usize>>>(last_id)) != Some(marked.clone());
    if moved {
        ui.data_mut(|d| d.insert_temp(last_id, marked.clone()));
    }
    let scroll = moved && marked.is_some() && !ui.rect_contains_pointer(ui.clip_rect());
    (marked, scroll)
}

pub(super) struct OutlineCtx<'a> {
    pub(super) nav: &'a mut Option<Nav>,
    pub(super) action: &'a mut Option<BmAction>,
    pub(super) rename: &'a mut Option<(Vec<usize>, String)>,
    pub(super) editable: bool,
    pub(super) current: usize,
    /// Expand all (`Some(usize::MAX)`), collapse all (`Some(0)`) or expand to a depth, this frame.
    pub(super) expand: Option<usize>,
    pub(super) matches: &'a std::collections::HashSet<Vec<usize>>,
    pub(super) searching: bool,
    /// The reader's place ([`marked_bookmark`]), and whether to scroll it into view.
    pub(super) marked: Option<Vec<usize>>,
    pub(super) scroll_to_marked: bool,
}

/// The left gutter of every bookmark row, where the reader's place is marked.
const MARK_GUTTER: f32 = 10.0;

/// A small right-pointing triangle in the gutter of the row at `rect`, at its first line. Painted
/// as a shape so it never depends on a font having ▶.
fn paint_mark(ui: &egui::Ui, rect: Rect, color: egui::Color32) {
    let (x, y) = (rect.left() + 2.0, rect.top() + 14.0);
    let points = vec![pos2(x, y - 4.5), pos2(x + 6.0, y), pos2(x, y + 4.5)];
    ui.painter().add(egui::Shape::convex_polygon(points, color, egui::Stroke::NONE));
}

/// The destination page label gets its own right-aligned column in a bookmark row; drawing
/// tools put whole section titles in /PageLabels, so the column is bounded (#124).
const LABEL_COLUMN_MAX: f32 = 72.0;

/// More characters than ever fit [`LABEL_COLUMN_MAX`] at the label size.
const LABEL_MEASURE_CHARS: usize = 64;

/// The longest head of `text` that `fits`, plus an ellipsis when the whole string doesn't.
/// Whole characters are dropped from the end, so the string is only ever cut at a char
/// boundary, and the loop always ends (at the ellipsis alone). A label is document text: only
/// its first [`LABEL_MEASURE_CHARS`] characters are measured, so a huge one can't make each
/// frame lay out thousands of candidates.
pub(crate) fn ellipsized_prefix(text: &str, mut fits: impl FnMut(&str) -> bool) -> String {
    let mut s: String = text.chars().take(LABEL_MEASURE_CHARS).collect();
    if s.len() == text.len() && fits(text) {
        return text.to_owned();
    }
    loop {
        s.pop();
        let candidate = format!("{s}\u{2026}");
        if s.is_empty() || fits(&candidate) {
            return candidate;
        }
    }
}

/// How a bookmark's title is drawn: in its own style and colour (ISO 32000-2 §12.3.3, Tables 151
/// and 152), bold in the heavier face and italic slanted, its colour kept readable on the panel.
/// Top-level bookmarks without a style keep the medium face.
fn title_format(item: &OutlineItem, depth: usize, t: &Tokens) -> egui::TextFormat {
    let font_id = match (item.bold, depth) {
        (true, _) => theme::semibold(13.0),
        (false, 0) => theme::medium(13.0),
        (false, _) => theme::regular(13.0),
    };
    let color = item.color.map_or(t.text, |rgb| {
        // Each component is within 0–1, so the rounded value fits a byte.
        let [r, g, b] = rgb.map(|c| (c * 255.0).round() as u8);
        theme::readable(egui::Color32::from_rgb(r, g, b), t.panel, t.text)
    });
    egui::TextFormat { font_id, color, italics: item.italic, ..Default::default() }
}

/// Draw one bookmark row with expansion, inline rename and the editable context menu.
pub(super) fn outline_item(
    ui: &mut egui::Ui,
    t: &Tokens,
    info: &DocInfo,
    item: &OutlineItem,
    path: &[usize],
    siblings: usize,
    cx: &mut OutlineCtx<'_>,
) {
    if cx.searching && !cx.matches.contains(path) {
        return;
    }
    let depth = path.len() - 1;
    let indent = depth as f32 * 16.0;
    let id = row_id(ui, path);
    // A bookmark opens as the file says at every level: a positive /Count is open, a negative
    // one (or none) closed (ISO 32000-2 §12.3.3, Table 151). The reader's own toggles and a
    // search override it.
    let mut open = row_open(ui, item, path, cx.expand, cx.searching);
    if let Some(levels) = cx.expand {
        ui.data_mut(|d| d.insert_temp(id, depth < levels));
    }
    let is_marked = cx.marked.as_deref() == Some(path);
    let x_text = MARK_GUTTER + indent + 20.0;
    if let Some((rpath, text)) = cx.rename.as_mut()
        && rpath.as_slice() == path
    {
        // Inline rename: Enter commits, Escape cancels.
        let mut commit = None;
        let mut cancel = false;
        ui.horizontal(|ui| {
            ui.add_space(x_text);
            crate::widgets::begin_dialog(ui);
            let edit = crate::widgets::line(text).desired_width(ui.available_width() - 8.0).id(id.with("rename"));
            let resp = ui.add(edit);
            if !resp.has_focus() && !resp.lost_focus() {
                resp.request_focus();
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                cancel = true;
            } else if resp.lost_focus() {
                commit = Some(text.trim().to_string());
            }
        });
        if cancel {
            *cx.rename = None;
        } else if let Some(title) = commit {
            *cx.rename = None;
            if !title.is_empty() && title != item.title {
                *cx.action = Some(BmAction::Rename(path.to_vec(), title));
            }
        }
    } else {
        // The page label sits right-aligned in its own bounded column, and the title wraps to
        // what is really left of it — a long label painted over a long title was the overlap
        // of #124.
        let label_font = theme::regular(11.0);
        let label_galley = item.page.and_then(|p| info.pages.get(p)).map(|page| {
            ui.fonts_mut(|f| {
                let text =
                    ellipsized_prefix(&page.label, |s| f.layout_no_wrap(s.to_owned(), label_font.clone(), t.text_faint).size().x <= LABEL_COLUMN_MAX);
                f.layout_no_wrap(text, label_font, t.text_faint)
            })
        });
        let label_w = label_galley.as_ref().map_or(0.0, |g| g.size().x);
        let wrap_w = (ui.available_width() - MARK_GUTTER - indent - 20.0 - label_w - 12.0).max(60.0);
        let mut job = egui::text::LayoutJob::single_section(item.title.clone(), title_format(item, depth, t));
        job.wrap.max_width = wrap_w;
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        let h = (galley.size().y + 12.0).max(28.0);
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
        resp.widget_info(|| {
            let mut info = egui::WidgetInfo::labeled(egui::Role::Button, true, &item.title);
            // Only the reader's place says it is selected, so the other rows read as before.
            if is_marked {
                info.selected = Some(true);
            }
            info
        });
        theme::Press::track(ui, &resp).wash(ui, rect, 6, is_marked);
        if is_marked {
            paint_mark(ui, rect, t.accent);
            if cx.scroll_to_marked {
                ui.scroll_to_rect(rect, None);
            }
        }
        let x0 = rect.left() + MARK_GUTTER + indent;
        if !item.children.is_empty() {
            let tri = Rect::from_min_size(pos2(x0, rect.top() + 6.0), vec2(16.0, 16.0));
            icons::paint(ui, tri, if open { "chevron-down" } else { "chevron-right" }, 14.0, t.text_muted);
            if !cx.searching && ui.interact(tri, id.with("tri"), Sense::click()).clicked() {
                open = !open;
                ui.data_mut(|d| d.insert_temp(id, open));
            }
        }
        ui.painter().galley(pos2(x0 + 20.0, rect.top() + 6.0), galley, t.text);
        if let Some(g) = label_galley {
            // Right-aligned, vertically centred at the first line, inside the column the title
            // wrapped around.
            let pos = pos2(rect.right() - 6.0 - g.size().x, rect.top() + 14.0 - g.size().y / 2.0);
            ui.painter().galley(pos, g, t.text_faint);
        }
        if theme::hand(resp.clone()).clicked()
            && let Some(p) = item.page
        {
            *cx.nav = Some(Nav::Dest(p, item.view));
        }
        if resp.double_clicked() && cx.editable {
            *cx.action = Some(BmAction::StartRename(path.to_vec()));
        }
        if cx.editable
            && let Some(&i) = path.last()
        {
            let current = cx.current;
            resp.context_menu(|ui| {
                let mut pick = |ui: &mut egui::Ui, label: &str, enabled: bool, a: BmAction| {
                    if ui.add_enabled(enabled, egui::Button::new(tl!(label))).clicked() {
                        *cx.action = Some(a);
                        ui.close();
                    }
                };
                pick(ui, tl!("Rename"), true, BmAction::StartRename(path.to_vec()));
                let set_page = crate::i18n::fmt(tl!("Set to current page ({n})"), &[("n", &(current + 1).to_string())]);
                pick(ui, &set_page, true, BmAction::SetToCurrentPage(path.to_vec()));
                // The title's style and colour (ISO 32000-2 §12.3.3, Tables 151 and 152).
                let mut styled = None;
                ui.menu_button(tl!("Style"), |ui| {
                    let (mut bold, mut italic) = (item.bold, item.italic);
                    if ui.add(crate::widgets::checkbox(&mut bold, tl!("Bold"))).clicked() {
                        styled = Some(BmAction::SetStyle(path.to_vec(), Some(bold), None));
                    }
                    if ui.add(crate::widgets::checkbox(&mut italic, tl!("Italic"))).clicked() {
                        styled = Some(BmAction::SetStyle(path.to_vec(), None, Some(italic)));
                    }
                });
                ui.menu_button(tl!("Colour"), |ui| {
                    let current = item.color.map_or([0.0; 3], |c| c.map(f64::from));
                    if let Some(c) = crate::comments::swatch_grid(ui, Some(current)) {
                        styled = Some(BmAction::SetColor(path.to_vec(), c));
                    }
                });
                if styled.is_some() {
                    ui.close();
                }
                ui.separator();
                pick(ui, tl!("Move up"), i > 0, BmAction::MoveUp(path.to_vec()));
                pick(ui, tl!("Move down"), i + 1 < siblings, BmAction::MoveDown(path.to_vec()));
                pick(ui, tl!("Indent"), i > 0, BmAction::Indent(path.to_vec()));
                pick(ui, tl!("Outdent"), depth > 0, BmAction::Outdent(path.to_vec()));
                ui.separator();
                pick(ui, tl!("Delete"), true, BmAction::Delete(path.to_vec()));
                if let Some(a) = styled {
                    *cx.action = Some(a);
                }
            });
        }
    }
    if open {
        for (ci, c) in item.children.iter().enumerate() {
            let mut p = path.to_vec();
            p.push(ci);
            outline_item(ui, t, info, c, &p, item.children.len(), cx);
        }
    }
}

#[cfg(test)]
mod label_column {
    use super::ellipsized_prefix;

    /// A short page label (the usual "3", "iv", "1-1") is shown unchanged.
    #[test]
    fn short_labels_are_not_touched() {
        for label in ["3", "iv", "1-1", "A-201"] {
            assert_eq!(ellipsized_prefix(label, |s| s.len() <= 10), label);
        }
    }

    /// A label wider than its column (#124: whole section titles in /PageLabels) keeps only
    /// the head that fits, with an ellipsis, and that head is a prefix of the real label.
    #[test]
    fn long_labels_are_cut_to_their_column_with_an_ellipsis() {
        let label = "Floor Plans (001 Floor Plans): GROUND FLOOR PLAN";
        let shown = ellipsized_prefix(label, |s| s.len() <= 20);
        assert!(shown.ends_with('\u{2026}'), "{shown:?}");
        assert!(shown.len() <= 20, "{shown:?}");
        assert!(label.starts_with(shown.trim_end_matches('\u{2026}')), "{shown:?}");
    }

    /// CJK labels (three bytes per character) are cut on character boundaries too.
    #[test]
    fn multibyte_labels_are_cut_on_char_boundaries() {
        let label = "図面（一階平面図）：配置図";
        let shown = ellipsized_prefix(label, |s| s.len() <= 12);
        assert!(shown.ends_with('\u{2026}'), "{shown:?}");
        assert!(shown.len() <= 12, "{shown:?}");
        assert!(label.starts_with(shown.trim_end_matches('\u{2026}')), "{shown:?}");
    }

    /// The pathological floor: nothing fits, the loop still ends with the lone ellipsis.
    #[test]
    fn the_loop_ends_when_nothing_fits() {
        assert_eq!(ellipsized_prefix("Floor Plans", |_| false), "\u{2026}");
    }

    /// A huge label from a hostile file is measured a bounded number of times per frame.
    #[test]
    fn a_huge_label_is_measured_a_bounded_number_of_times() {
        let label = "x".repeat(1_000_000);
        let mut calls = 0;
        let shown = ellipsized_prefix(&label, |s| {
            calls += 1;
            s.len() <= 12
        });
        assert!(calls <= super::LABEL_MEASURE_CHARS + 1, "{calls} measurements");
        assert!(shown.ends_with('\u{2026}') && label.starts_with(shown.trim_end_matches('\u{2026}')), "{shown:?}");
    }
}

#[cfg(test)]
mod bookmark_filter {
    use super::{outline_matches, title_format};
    use crate::theme::{self, Tokens};
    use pdfcraft_render::OutlineItem;

    /// #3193: a bookmark's title is drawn in its own style and colour.
    #[test]
    fn bookmark_titles_take_their_style_and_colour() {
        let t = Tokens::for_kind(crate::theme::ThemeKind::Light);
        let plain = item("Plain", Vec::new());
        assert_eq!(title_format(&plain, 0, &t).font_id, theme::medium(13.0));
        assert_eq!(title_format(&plain, 1, &t).font_id, theme::regular(13.0));
        assert_eq!(title_format(&plain, 1, &t).color, t.text);
        assert!(!title_format(&plain, 1, &t).italics);
        let styled = OutlineItem { color: Some([1.0, 0.0, 0.0]), italic: true, bold: true, ..item("Styled", Vec::new()) };
        let f = title_format(&styled, 1, &t);
        assert_eq!(f.font_id, theme::semibold(13.0));
        assert!(f.italics);
        assert_eq!(f.color, egui::Color32::from_rgb(255, 0, 0));
        let half = OutlineItem { color: Some([0.0, 0.5, 0.0]), ..item("Green", Vec::new()) };
        assert_eq!(title_format(&half, 1, &t).color, egui::Color32::from_rgb(0, 128, 0));
    }

    fn item(title: &str, children: Vec<OutlineItem>) -> OutlineItem {
        OutlineItem {
            title: title.into(),
            page: Some(0),
            view: pdfcraft_render::DestView::Top,
            children,
            open: true,
            color: None,
            italic: false,
            bold: false,
        }
    }

    /// Turkish dotted and dotless i match in any case, as in Find (#892).
    #[test]
    fn titles_match_in_any_case_with_turkish_i() {
        let outline = [item("İSTANBUL ŞUBESİ", vec![item("Kış dönemi", vec![])]), item("Ankara", vec![])];
        let fold = pdfcraft_render::text::fold_case;
        assert_eq!(outline_matches(&outline, &fold("istanbul")).len(), 1);
        assert_eq!(outline_matches(&outline, &fold("şubesi")).len(), 1);
        // A child match keeps its parent: [0] and [0, 0].
        assert_eq!(outline_matches(&outline, &fold("KIŞ")).len(), 2);
        assert!(outline_matches(&outline, &fold("izmir")).is_empty());
    }
}

#[cfg(test)]
mod current_mark {
    use super::current_bookmark;
    use pdfcraft_render::OutlineItem;

    fn at(title: &str, page: Option<usize>, children: Vec<OutlineItem>) -> OutlineItem {
        OutlineItem {
            title: title.into(),
            page,
            view: pdfcraft_render::DestView::Top,
            children,
            open: true,
            color: None,
            italic: false,
            bold: false,
        }
    }

    fn all_open(_: &[usize], _: &OutlineItem) -> bool {
        true
    }

    /// The user's file: "A2-paper3- Question.pdf" (page 1) holding papers at pages 85 and 97.
    fn papers() -> Vec<OutlineItem> {
        vec![
            at("A2-paper3- Question.pdf", Some(0), vec![at("qp_31", Some(84), vec![]), at("qp_32", Some(96), vec![])]),
            at("Answers", Some(200), vec![]),
        ]
    }

    #[test]
    fn an_empty_outline_marks_nothing() {
        assert_eq!(current_bookmark(&[], 5, all_open), None);
    }

    #[test]
    fn a_page_before_the_first_bookmark_marks_nothing() {
        let outline = [at("One", Some(3), vec![]), at("Two", Some(5), vec![])];
        assert_eq!(current_bookmark(&outline, 2, all_open), None);
    }

    #[test]
    fn a_page_exactly_on_a_bookmark_marks_it() {
        assert_eq!(current_bookmark(&papers(), 84, all_open), Some(vec![0, 0]));
        assert_eq!(current_bookmark(&papers(), 200, all_open), Some(vec![1]));
    }

    #[test]
    fn a_page_between_bookmarks_marks_the_one_before_it() {
        // Page 87 of the user's file (index 86) is inside qp_31.
        assert_eq!(current_bookmark(&papers(), 86, all_open), Some(vec![0, 0]));
        assert_eq!(current_bookmark(&papers(), 50, all_open), Some(vec![0]));
        assert_eq!(current_bookmark(&papers(), 10_000, all_open), Some(vec![1]));
    }

    #[test]
    fn nested_children_are_marked_at_their_depth() {
        let outline = [at("Part", Some(0), vec![at("Chapter", Some(2), vec![at("Section", Some(4), vec![])])])];
        assert_eq!(current_bookmark(&outline, 5, all_open), Some(vec![0, 0, 0]));
        assert_eq!(current_bookmark(&outline, 3, all_open), Some(vec![0, 0]));
    }

    #[test]
    fn a_collapsed_parent_is_marked_in_place_of_its_hidden_child() {
        let outline = [at("Part", Some(0), vec![at("Chapter", Some(2), vec![at("Section", Some(4), vec![])])])];
        let mut asked = Vec::new();
        let chapter_closed = |path: &[usize], _: &OutlineItem| {
            asked.push(path.to_vec());
            path != [0, 0]
        };
        assert_eq!(current_bookmark(&outline, 5, chapter_closed), Some(vec![0, 0]));
        // Only the ancestors are asked, top-down.
        assert_eq!(asked, vec![vec![0], vec![0, 0]]);
        assert_eq!(current_bookmark(&outline, 5, |_, _| false), Some(vec![0]));
    }

    #[test]
    fn unsorted_destinations_mark_the_last_in_document_order() {
        let outline = [at("Late", Some(9), vec![]), at("Early", Some(1), vec![]), at("Middle", Some(5), vec![])];
        assert_eq!(current_bookmark(&outline, 6, all_open), Some(vec![2]));
        assert_eq!(current_bookmark(&outline, 3, all_open), Some(vec![1]));
        assert_eq!(current_bookmark(&outline, 9, all_open), Some(vec![2]));
    }

    #[test]
    fn bookmarks_without_a_page_are_skipped() {
        let outline = [at("One", Some(0), vec![at("Nowhere", None, vec![])]), at("Also nowhere", None, vec![])];
        assert_eq!(current_bookmark(&outline, 7, all_open), Some(vec![0]));
        assert_eq!(current_bookmark(&[at("Nowhere", None, vec![])], 0, all_open), None);
    }

    /// A tree far deeper than the panel lists is walked to a bounded depth, without overflow.
    #[test]
    fn a_very_deep_tree_is_walked_to_a_bounded_depth() {
        let mut item = at("leaf", Some(0), vec![]);
        for _ in 0..1_000 {
            item = at("level", None, vec![item]);
        }
        let outline = [item];
        assert_eq!(current_bookmark(&outline, 0, all_open), None);
        let mut item = at("leaf", Some(0), vec![]);
        for _ in 0..100 {
            item = at("level", Some(0), vec![item]);
        }
        let marked = current_bookmark(&[item], 0, all_open).unwrap_or_default();
        assert_eq!(marked.len(), super::MARK_MAX_DEPTH);
        // Dropping the item tree recursively is the test's own cost, not the walk's.
    }
}
