//! A small PDF writer for the report exports (Story 5.32): A4 pages of titled tables in the
//! bundled Sora font. The font is embedded whole (`assets/Sora-Regular.ttf`, OFL) as a Type 0 /
//! CIDFontType2 with identity encoding, so a report renders with no network fetch and no system
//! font. Digits all take the width of the widest digit (tabular numerals), so columns of figures
//! line up. Text outside the font's Latin coverage prints as `?`.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::sync::OnceLock;

static SORA: &[u8] = include_bytes!("../assets/Sora-Regular.ttf");

const MARGIN: f64 = 36.0;
const A4_SHORT: f64 = 595.0;
const A4_LONG: f64 = 842.0;
const BASE_SIZE: f64 = 8.0;
const MIN_SIZE: f64 = 5.5;
const COLUMN_GAP: f64 = 10.0;

fn be16(d: &[u8], o: usize) -> Option<u16> {
    d.get(o..o + 2).map(|b| u16::from_be_bytes([b[0], b[1]]))
}

fn be32(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// The metrics and character map of the embedded TrueType font.
pub(crate) struct Font {
    units: f64,
    /// Advance per glyph id, in font units (digits already set to the widest digit).
    advances: Vec<u16>,
    cmap: HashMap<u32, u16>,
    ascent: i16,
    descent: i16,
    bbox: [i16; 4],
}

impl Font {
    /// The bundled Sora, parsed once; `None` only if the asset were damaged.
    pub(crate) fn sora() -> Option<&'static Font> {
        static FONT: OnceLock<Option<Font>> = OnceLock::new();
        FONT.get_or_init(|| Font::parse(SORA)).as_ref()
    }

    fn parse(d: &'static [u8]) -> Option<Font> {
        let count = usize::from(be16(d, 4)?);
        let table = |tag: &[u8; 4]| -> Option<usize> {
            (0..count).find_map(|i| {
                let o = 12 + i * 16;
                (d.get(o..o + 4)? == tag).then(|| be32(d, o + 8).map(|v| v as usize))?
            })
        };
        let head = table(b"head")?;
        let hhea = table(b"hhea")?;
        let hmtx = table(b"hmtx")?;
        let maxp = table(b"maxp")?;
        let cmap_at = table(b"cmap")?;
        let units = f64::from(be16(d, head + 18)?);
        let bbox = [
            be16(d, head + 36)? as i16,
            be16(d, head + 38)? as i16,
            be16(d, head + 40)? as i16,
            be16(d, head + 42)? as i16,
        ];
        let ascent = be16(d, hhea + 4)? as i16;
        let descent = be16(d, hhea + 6)? as i16;
        let metrics = usize::from(be16(d, hhea + 34)?);
        let glyphs = usize::from(be16(d, maxp + 4)?);
        let mut advances = Vec::with_capacity(glyphs);
        for g in 0..glyphs {
            let m = g.min(metrics.saturating_sub(1));
            advances.push(be16(d, hmtx + m * 4)?);
        }
        let cmap = parse_cmap(d, cmap_at)?;
        let mut font = Font {
            units,
            advances,
            cmap,
            ascent,
            descent,
            bbox,
        };
        let digits: Vec<usize> = ('0'..='9')
            .filter_map(|c| font.cmap.get(&(c as u32)).map(|g| usize::from(*g)))
            .collect();
        let widest = digits
            .iter()
            .filter_map(|g| font.advances.get(*g))
            .max()
            .copied();
        if let Some(widest) = widest {
            for g in digits {
                font.advances[g] = widest;
            }
        }
        Some(font)
    }

    fn glyph(&self, c: char) -> u16 {
        self.cmap
            .get(&(c as u32))
            .or_else(|| self.cmap.get(&('?' as u32)))
            .copied()
            .unwrap_or(0)
    }

    fn advance(&self, gid: u16) -> u16 {
        self.advances.get(usize::from(gid)).copied().unwrap_or(0)
    }

    /// The width of `text` at `size` points.
    pub(crate) fn width(&self, text: &str, size: f64) -> f64 {
        let units: f64 = text
            .chars()
            .map(|c| f64::from(self.advance(self.glyph(c))))
            .sum();
        units / self.units * size
    }

    /// `text` as the hex string a content stream shows (two bytes per glyph), as written into the
    /// PDF; the unit tests search the document for it.
    pub(crate) fn encode(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len() * 4);
        for c in text.chars() {
            let _ = write!(out, "{:04X}", self.glyph(c));
        }
        out
    }
}

/// The Unicode BMP map of the first usable `cmap` subtable (format 4, Windows or Unicode).
fn parse_cmap(d: &[u8], at: usize) -> Option<HashMap<u32, u16>> {
    let subtables = usize::from(be16(d, at + 2)?);
    let mut best = None;
    for i in 0..subtables {
        let rec = at + 4 + i * 8;
        let (platform, encoding) = (be16(d, rec)?, be16(d, rec + 2)?);
        let offset = at + be32(d, rec + 4)? as usize;
        let unicode = (platform == 3 && encoding == 1) || platform == 0;
        if unicode && be16(d, offset)? == 4 {
            best = Some(offset);
            if platform == 3 {
                break;
            }
        }
    }
    let t = best?;
    let segs = usize::from(be16(d, t + 6)?) / 2;
    let ends = t + 14;
    let starts = ends + segs * 2 + 2;
    let deltas = starts + segs * 2;
    let ranges = deltas + segs * 2;
    let mut map = HashMap::new();
    for s in 0..segs {
        let end = be16(d, ends + s * 2)?;
        let start = be16(d, starts + s * 2)?;
        let delta = be16(d, deltas + s * 2)?;
        let range_at = ranges + s * 2;
        let range = be16(d, range_at)?;
        if start == 0xFFFF {
            continue;
        }
        for c in start..=end {
            let gid = if range == 0 {
                c.wrapping_add(delta)
            } else {
                let g = be16(
                    d,
                    range_at + usize::from(range) + usize::from(c - start) * 2,
                )?;
                if g == 0 { 0 } else { g.wrapping_add(delta) }
            };
            if gid != 0 {
                map.insert(u32::from(c), gid);
            }
        }
    }
    Some(map)
}

/// One table of a report, as strings.
pub(crate) struct Table {
    pub heading: String,
    pub header: Vec<String>,
    /// Right-aligned columns (figures).
    pub right: Vec<bool>,
    pub rows: Vec<Vec<String>>,
}

/// What a PDF shows.
pub(crate) struct PdfDoc {
    pub branch: String,
    pub title: String,
    pub range: String,
    pub generated: String,
    pub tables: Vec<Table>,
}

/// The widths of a table's columns at `size`, and that size: the natural widths, shrunk to the
/// page's usable width (down to `MIN_SIZE`; text is cut with `..` beyond that).
fn column_widths(font: &Font, t: &Table, usable: f64, size: f64) -> (Vec<f64>, f64) {
    let natural = |size: f64| -> Vec<f64> {
        (0..t.header.len())
            .map(|c| {
                let cells = t.rows.iter().filter_map(|r| r.get(c));
                std::iter::once(&t.header[c])
                    .chain(cells)
                    .map(|s| font.width(s, size))
                    .fold(0.0, f64::max)
            })
            .collect()
    };
    let gaps = COLUMN_GAP * t.header.len().saturating_sub(1) as f64;
    let mut widths = natural(size);
    let total: f64 = widths.iter().sum::<f64>() + gaps;
    if total <= usable {
        return (widths, size);
    }
    let fitted = (size * (usable / total)).max(MIN_SIZE);
    widths = natural(fitted);
    let total: f64 = widths.iter().sum::<f64>() + gaps;
    if total > usable {
        // Still too wide: take the excess from the text columns, widest first.
        let mut excess = total - usable;
        let mut order: Vec<usize> = (0..widths.len()).filter(|c| !t.right[*c]).collect();
        order.sort_by(|a, b| widths[*b].total_cmp(&widths[*a]));
        for c in order {
            let take = excess.min((widths[c] - 24.0).max(0.0));
            widths[c] -= take;
            excess -= take;
            if excess <= 0.0 {
                break;
            }
        }
    }
    (widths, fitted)
}

/// `text` cut with `..` to fit `width` at `size`.
fn fit(font: &Font, text: &str, width: f64, size: f64) -> String {
    if font.width(text, size) <= width + 0.01 {
        return text.to_owned();
    }
    let mut cut: String = text.to_owned();
    while !cut.is_empty() && font.width(&format!("{cut}.."), size) > width {
        cut.pop();
    }
    format!("{cut}..")
}

struct Page {
    content: String,
}

struct Layout<'a> {
    font: &'a Font,
    width: f64,
    height: f64,
    pages: Vec<Page>,
    y: f64,
    used: BTreeMap<u16, char>,
}

impl<'a> Layout<'a> {
    fn new(font: &'a Font, landscape: bool) -> Self {
        let (width, height) = if landscape {
            (A4_LONG, A4_SHORT)
        } else {
            (A4_SHORT, A4_LONG)
        };
        let mut l = Layout {
            font,
            width,
            height,
            pages: Vec::new(),
            y: 0.0,
            used: BTreeMap::new(),
        };
        l.new_page();
        l
    }

    fn new_page(&mut self) {
        self.pages.push(Page {
            content: String::new(),
        });
        self.y = self.height - MARGIN;
    }

    fn usable(&self) -> f64 {
        self.width - 2.0 * MARGIN
    }

    /// Shows `text` with its baseline at `y`, left edge at `x` (or right edge, if `right`).
    fn text(&mut self, text: &str, x: f64, y: f64, size: f64, right: bool) {
        if text.is_empty() {
            return;
        }
        for c in text.chars() {
            let g = self.font.glyph(c);
            self.used.entry(g).or_insert(c);
        }
        let left = if right {
            x - self.font.width(text, size)
        } else {
            x
        };
        let hex = self.font.encode(text);
        if let Some(page) = self.pages.last_mut() {
            let _ = writeln!(
                page.content,
                "BT /F1 {size:.2} Tf {left:.2} {y:.2} Td <{hex}> Tj ET"
            );
        }
    }

    fn rule(&mut self, y: f64, grey: f64) {
        let (x0, x1) = (MARGIN, self.width - MARGIN);
        if let Some(page) = self.pages.last_mut() {
            let _ = writeln!(
                page.content,
                "{grey:.2} G 0.5 w {x0:.2} {y:.2} m {x1:.2} {y:.2} l S"
            );
        }
    }

    fn heading_block(&mut self, doc: &PdfDoc) {
        self.text(&doc.branch, MARGIN, self.y - 12.0, 13.0, false);
        self.y -= 18.0;
        self.text(&doc.title, MARGIN, self.y - 10.0, 11.0, false);
        self.y -= 16.0;
        self.text(&doc.range, MARGIN, self.y - 8.0, 8.5, false);
        self.y -= 12.0;
        self.text(&doc.generated, MARGIN, self.y - 8.0, 8.5, false);
        self.y -= 14.0;
        self.rule(self.y, 0.0);
        self.y -= 12.0;
    }

    fn table(&mut self, t: &Table) {
        let (widths, size) = column_widths(self.font, t, self.usable(), BASE_SIZE);
        let row_h = size * 1.6;
        let mut xs = Vec::with_capacity(widths.len());
        let mut x = MARGIN;
        for w in &widths {
            xs.push(x);
            x += w + COLUMN_GAP;
        }
        let need = 11.0 + row_h * 3.0;
        if self.y - need < MARGIN + 18.0 {
            self.new_page();
        }
        if !t.heading.is_empty() {
            self.text(&t.heading, MARGIN, self.y - 9.0, 9.5, false);
            self.y -= 15.0;
        }
        self.header_row(t, &widths, &xs, size, row_h);
        let continued = format!("{} (continued)", t.heading);
        for row in &t.rows {
            if self.y - row_h < MARGIN + 18.0 {
                self.new_page();
                self.table_rows_resume(t, row, &widths, &xs, size, row_h, &continued);
            } else {
                self.row(t, row, &widths, &xs, size, row_h);
            }
        }
        self.y -= 10.0;
    }

    /// Starts a continuation page: the heading, the header row and `first`.
    #[allow(clippy::too_many_arguments)]
    fn table_rows_resume(
        &mut self,
        t: &Table,
        first: &[String],
        widths: &[f64],
        xs: &[f64],
        size: f64,
        row_h: f64,
        heading: &str,
    ) {
        if !t.heading.is_empty() {
            self.text(heading, MARGIN, self.y - 9.0, 9.5, false);
            self.y -= 15.0;
        }
        self.header_row(t, widths, xs, size, row_h);
        self.row(t, first, widths, xs, size, row_h);
    }

    fn header_row(&mut self, t: &Table, widths: &[f64], xs: &[f64], size: f64, row_h: f64) {
        let baseline = self.y - size;
        for (c, h) in t.header.iter().enumerate() {
            let text = fit(self.font, h, widths[c], size);
            let x = if t.right[c] { xs[c] + widths[c] } else { xs[c] };
            self.text(&text, x, baseline, size, t.right[c]);
        }
        self.y -= row_h;
        self.rule(self.y + row_h * 0.25, 0.6);
    }

    fn row(
        &mut self,
        t: &Table,
        row: &[String],
        widths: &[f64],
        xs: &[f64],
        size: f64,
        row_h: f64,
    ) {
        let baseline = self.y - size;
        for (c, cell) in row.iter().enumerate().take(widths.len()) {
            let text = fit(self.font, cell, widths[c], size);
            let x = if t.right[c] { xs[c] + widths[c] } else { xs[c] };
            self.text(&text, x, baseline, size, t.right[c]);
        }
        self.y -= row_h;
    }
}

/// Renders `doc` as an A4 PDF: portrait, or landscape when a table is too wide for portrait.
/// `None` only if the bundled font could not be read.
pub(crate) fn render(doc: &PdfDoc) -> Option<Vec<u8>> {
    let font = Font::sora()?;
    let portrait_usable = A4_SHORT - 2.0 * MARGIN;
    let landscape = doc.tables.iter().any(|t| {
        let (_, size) = column_widths(font, t, portrait_usable, BASE_SIZE);
        size < BASE_SIZE * 0.8
    });
    let mut layout = Layout::new(font, landscape);
    layout.heading_block(doc);
    if doc.tables.is_empty() {
        layout.text("No data.", MARGIN, layout.y - 9.0, 9.0, false);
    }
    for t in &doc.tables {
        layout.table(t);
    }
    let pages = layout.pages.len();
    for i in 0..pages {
        let label = format!("Page {} of {pages}", i + 1);
        let x = layout.width / 2.0;
        let width = font.width(&label, 7.0);
        for c in label.chars() {
            layout.used.entry(font.glyph(c)).or_insert(c);
        }
        let hex = font.encode(&label);
        let _ = writeln!(
            layout.pages[i].content,
            "BT /F1 7.00 Tf {:.2} {:.2} Td <{hex}> Tj ET",
            x - width / 2.0,
            MARGIN / 2.0
        );
    }
    Some(assemble(font, &layout, &doc.title))
}

fn push_obj(objects: &mut Vec<Vec<u8>>, body: String) {
    objects.push(body.into_bytes());
}

fn assemble(font: &Font, layout: &Layout<'_>, title: &str) -> Vec<u8> {
    let scale = |v: f64| (v / font.units * 1000.0).round() as i64;
    let mut widths = String::new();
    for g in layout.used.keys() {
        let _ = write!(widths, "{g} [{}] ", scale(f64::from(font.advance(*g))));
    }
    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def /CMapType 2 def\n\
         1 begincodespacerange <0000> <FFFF> endcodespacerange\n",
    );
    let entries: Vec<(&u16, &char)> = layout.used.iter().collect();
    for chunk in entries.chunks(100) {
        let _ = writeln!(cmap, "{} beginbfchar", chunk.len());
        for (g, c) in chunk {
            let code = u32::from(**c).min(0xFFFF);
            let _ = writeln!(cmap, "<{g:04X}> <{code:04X}>");
        }
        cmap.push_str("endbfchar\n");
    }
    cmap.push_str("endcmap CMapName currentdict /CMap defineresource pop end end\n");

    let mut objects: Vec<Vec<u8>> = Vec::new();
    let page_ids: Vec<usize> = (0..layout.pages.len()).map(|i| 9 + i * 2).collect();
    let kids = page_ids
        .iter()
        .map(|id| format!("{id} 0 R"))
        .collect::<Vec<_>>()
        .join(" ");
    push_obj(&mut objects, "<< /Type /Catalog /Pages 2 0 R >>".to_owned());
    push_obj(
        &mut objects,
        format!(
            "<< /Type /Pages /Kids [{kids}] /Count {} >>",
            layout.pages.len()
        ),
    );
    push_obj(
        &mut objects,
        "<< /Type /Font /Subtype /Type0 /BaseFont /Sora-Regular /Encoding /Identity-H \
         /DescendantFonts [4 0 R] /ToUnicode 7 0 R >>"
            .to_owned(),
    );
    push_obj(
        &mut objects,
        format!(
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Sora-Regular \
         /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
         /FontDescriptor 5 0 R /CIDToGIDMap /Identity /DW 600 /W [{widths}] >>"
        ),
    );
    push_obj(
        &mut objects,
        format!(
            "<< /Type /FontDescriptor /FontName /Sora-Regular /Flags 32 \
         /FontBBox [{} {} {} {}] /ItalicAngle 0 /Ascent {} /Descent {} \
         /CapHeight {} /StemV 80 /FontFile2 6 0 R >>",
            scale(f64::from(font.bbox[0])),
            scale(f64::from(font.bbox[1])),
            scale(f64::from(font.bbox[2])),
            scale(f64::from(font.bbox[3])),
            scale(f64::from(font.ascent)),
            scale(f64::from(font.descent)),
            scale(f64::from(font.ascent) * 0.75),
        ),
    );
    let mut file = format!(
        "<< /Length {} /Length1 {} >>\nstream\n",
        SORA.len(),
        SORA.len()
    )
    .into_bytes();
    file.extend_from_slice(SORA);
    file.extend_from_slice(b"\nendstream");
    objects.push(file);
    push_obj(
        &mut objects,
        format!("<< /Length {} >>\nstream\n{cmap}endstream", cmap.len()),
    );
    let ascii_title: String = title
        .chars()
        .filter(|c| c.is_ascii() && *c != '(' && *c != ')' && *c != '\\')
        .collect();
    push_obj(
        &mut objects,
        format!("<< /Title ({ascii_title}) /Producer (Arena 360 Hub) >>"),
    );
    for (i, page) in layout.pages.iter().enumerate() {
        push_obj(
            &mut objects,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.0} {:.0}] \
             /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
                layout.width,
                layout.height,
                page_ids[i] + 1
            ),
        );
        push_obj(
            &mut objects,
            format!(
                "<< /Length {} >>\nstream\n{}endstream",
                page.content.len(),
                page.content
            ),
        );
    }

    let mut out = b"%PDF-1.5\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info 8 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(rows: usize) -> PdfDoc {
        PdfDoc {
            branch: "Arena Koramangala".into(),
            title: "Daily sales".into(),
            range: "2026-10-01 to 2026-10-31".into(),
            generated: "Generated 2026-11-01 09:00".into(),
            tables: vec![Table {
                heading: "Sales by type".into(),
                header: vec!["Type".into(), "Amount".into()],
                right: vec![false, true],
                rows: (0..rows)
                    .map(|i| vec![format!("Row {i}"), format!("{},234.50", i + 1)])
                    .collect(),
            }],
        }
    }

    #[test]
    fn the_bundled_font_is_read_with_digits_set_to_one_width() {
        let font = Font::sora().expect("the bundled font parses");
        for c in "0123456789.,-?".chars() {
            assert_ne!(font.glyph(c), 0, "{c} is in the font");
        }
        let widths: Vec<u16> = ('0'..='9').map(|c| font.advance(font.glyph(c))).collect();
        assert!(widths.iter().all(|w| *w == widths[0]), "tabular digits");
        assert!(font.width("1111", 10.0) - font.width("8888", 10.0) < 0.001);
    }

    #[test]
    fn a_pdf_is_a_valid_a4_document_that_shows_the_figures() {
        let bytes = render(&doc(3)).expect("renders");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.starts_with("%PDF-1.5"));
        assert!(text.trim_end().ends_with("%%EOF"));
        assert!(text.contains("/MediaBox [0 0 595 842]"));
        assert!(text.contains("/FontFile2 6 0 R"));
        let font = Font::sora().unwrap();
        assert!(text.contains(&format!("<{}>", font.encode("2,234.50"))));
        assert!(text.contains(&format!("<{}>", font.encode("Arena Koramangala"))));
        assert!(text.contains(&format!("<{}>", font.encode("Page 1 of 1"))));
    }

    #[test]
    fn a_long_table_runs_over_to_more_pages_with_its_header_repeated() {
        let bytes = render(&doc(200)).expect("renders");
        let text = String::from_utf8_lossy(&bytes);
        let pages = text.matches("/Type /Page ").count();
        assert!(pages > 1, "{pages} pages");
        let font = Font::sora().unwrap();
        assert!(text.contains(&format!(
            "<{}>",
            font.encode(&format!("Page {pages} of {pages}"))
        )));
        assert!(text.contains(&format!("<{}>", font.encode("Sales by type (continued)"))));
    }

    #[test]
    fn a_wide_table_turns_the_page_to_landscape() {
        let mut d = doc(2);
        d.tables[0].header = (0..12).map(|i| format!("Column heading {i}")).collect();
        d.tables[0].right = vec![true; 12];
        d.tables[0].rows = vec![(0..12).map(|i| format!("123,456,78{i}.00")).collect()];
        let bytes = render(&d).expect("renders");
        assert!(String::from_utf8_lossy(&bytes).contains("/MediaBox [0 0 842 595]"));
    }
}
