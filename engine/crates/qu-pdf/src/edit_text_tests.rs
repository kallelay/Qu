//! Tests for `edit_text.rs` (`replace_text`, `add_text`), on tiny PDFs
//! whose content streams are written out by hand.

use super::*;
use lopdf::{dictionary, Stream};
use std::io::Cursor;

/// One page per content string; every page has /F1 = Courier WinAnsi and
/// /F2 = a Type0 font.
fn build(base_font: &str, contents: &[&str]) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let f1 = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => Object::Name(base_font.as_bytes().to_vec()),
        "Encoding" => "WinAnsiEncoding",
    });
    let desc = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "CIDFontType2", "BaseFont" => "Cid" });
    let f2 = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "Cid", "Encoding" => "Identity-H",
        "DescendantFonts" => vec![Object::Reference(desc)],
    });
    let res = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => f1, "F2" => f2 } });
    let mut kids = Vec::new();
    for c in contents {
        let cid = doc.add_object(Stream::new(dictionary! {}, c.as_bytes().to_vec()));
        let pid = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => cid, "Resources" => res,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        kids.push(Object::Reference(pid));
    }
    let n = kids.len() as u32;
    doc.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => n }));
    let cat = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", cat);
    let mut buf = Cursor::new(Vec::new());
    doc.save_to(&mut buf).unwrap();
    buf.into_inner()
}

const P1: &str = "BT /F1 12 Tf 72 700 Td (Hello World) Tj ET";

fn text(b: &[u8]) -> String {
    extract_text(b, None).unwrap()
}

#[test]
fn replaces_a_simple_string() {
    let out = replace_text(&build("Courier", &[P1]), "World", "Qu", None, None, true).unwrap();
    let t = text(&out);
    assert!(t.contains("Hello Qu"), "{t}");
    assert!(!t.contains("World"), "{t}");
}

#[test]
fn replaces_across_tj_pieces_and_word_gaps() {
    let c = "BT /F1 12 Tf 72 700 Td [(Hel) -20 (lo Wor) 30 (ld)] TJ ET";
    let out = replace_text(&build("Courier", &[c]), "Hello World", "Howdy", None, None, true).unwrap();
    let t = text(&out);
    assert!(t.contains("Howdy") && !t.contains("Hello"), "{t}");
    // a -300 adjustment is a word gap, i.e. a space for matching purposes
    let c = "BT /F1 12 Tf 72 700 Td [(Hello) -300 (World)] TJ ET";
    let out = replace_text(&build("Courier", &[c]), "Hello World", "Hi", None, None, true).unwrap();
    assert_eq!(text(&out).trim(), "Hi");
}

#[test]
fn replaces_in_hex_strings_and_across_consecutive_tj() {
    let c = "BT /F1 12 Tf 72 700 Td <48656C6C6F20> Tj (World) Tj ET";
    let out = replace_text(&build("Courier", &[c]), "Hello World", "Bye", None, None, true).unwrap();
    assert_eq!(text(&out).trim(), "Bye");
}

#[test]
fn page_count_and_case_options() {
    let pdf = build("Courier", &[P1, P1, P1]);
    let out = replace_text(&pdf, "Hello", "Yo", Some(2), None, true).unwrap();
    let t2 = extract_text(&out, Some(&[2])).unwrap();
    let t1 = extract_text(&out, Some(&[1])).unwrap();
    assert!(t2.contains("Yo World") && t1.contains("Hello World"));
    // other pages' content is untouched byte for byte
    let (a, b) = (load(&pdf).unwrap(), load(&out).unwrap());
    let ids_a = a.get_pages();
    let ids_b = b.get_pages();
    assert_eq!(a.get_page_content(ids_a[&1]), b.get_page_content(ids_b[&1]));
    assert_ne!(a.get_page_content(ids_a[&2]), b.get_page_content(ids_b[&2]));
    // count=1 over all pages replaces only the first hit
    let out = replace_text(&pdf, "Hello", "Yo", None, Some(1), true).unwrap();
    let all = text(&out);
    assert_eq!(all.matches("Yo").count(), 1, "{all}");
    assert_eq!(all.matches("Hello").count(), 2, "{all}");
    // case-insensitive
    let out = replace_text(&build("Courier", &[P1]), "hello world", "X", None, None, false).unwrap();
    assert_eq!(text(&out).trim(), "X");
    assert!(replace_text(&build("Courier", &[P1]), "hello world", "X", None, None, true).is_err());
}

#[test]
fn composite_font_is_refused_with_the_reason() {
    let c = "BT /F2 12 Tf 72 700 Td <00480065> Tj ET BT /F1 12 Tf 72 600 Td (other) Tj ET";
    let err = replace_text(&build("Courier", &[c]), "He", "Xx", None, None, true).unwrap_err();
    assert!(err.contains("composite") && err.contains("/F2"), "{err}");
}

#[test]
fn not_found_and_unencodable_are_named_errors() {
    let pdf = build("Courier", &[P1]);
    let err = replace_text(&pdf, "nothing here", "x", None, None, true).unwrap_err();
    assert!(err.contains("not found"), "{err}");
    let err = replace_text(&pdf, "World", "Wor\u{0142}d", None, None, true).unwrap_err();
    assert!(err.contains("U+0142") && err.contains("/F1"), "{err}");
    let err = replace_text(&pdf, "World", "x", Some(9), None, true).unwrap_err();
    assert!(err.contains("no page 9"), "{err}");
    assert!(replace_text(&pdf, "", "x", None, None, true).is_err());
}

#[test]
fn subset_fonts_only_allow_glyphs_already_shown() {
    let pdf = build("ABCDEF+Courier", &[P1]);
    // 'H','e','l','o',' ','W','r','d' exist; 'z' does not
    let out = replace_text(&pdf, "World", "Held", None, None, true).unwrap();
    assert!(text(&out).contains("Hello Held"));
    let err = replace_text(&pdf, "World", "zoo", None, None, true).unwrap_err();
    assert!(err.contains("subset"), "{err}");
}

#[test]
fn other_content_is_kept() {
    let c = "q 1 0 0 rg 10 10 100 100 re f Q BT /F1 12 Tf 72 700 Td (Hello World) Tj ET";
    let out = replace_text(&build("Courier", &[c]), "World", "Qu", None, None, true).unwrap();
    let d = load(&out).unwrap();
    let page = d.get_pages()[&1];
    let s = String::from_utf8_lossy(&d.get_page_content(page)).into_owned();
    assert!(s.contains("re") && s.contains("(Hello Qu)"), "{s}");
}

#[test]
fn add_text_stamps_visible_text_and_keeps_the_page() {
    let pdf = build("Courier", &[P1, P1]);
    let out = add_text(&pdf, 2, 100.0, 500.0, "Stamp (A) \u{e9}\u{20ac}", 14.0, [1.0, 0.0, 0.0], "Helvetica").unwrap();
    let t2 = extract_text(&out, Some(&[2])).unwrap();
    assert!(t2.contains("Hello World") && t2.contains("Stamp (A)"), "{t2}");
    assert!(!extract_text(&out, Some(&[1])).unwrap().contains("Stamp"));
    // the stamp itself can be edited later, since it is a simple font
    let again = replace_text(&out, "Stamp", "Seal", Some(2), None, true).unwrap();
    assert!(extract_text(&again, Some(&[2])).unwrap().contains("Seal"));
    assert!(add_text(&pdf, 1, 0.0, 0.0, "\u{4e2d}", 12.0, [0.0; 3], "Helvetica").is_err());
    assert!(add_text(&pdf, 3, 0.0, 0.0, "x", 12.0, [0.0; 3], "Helvetica").is_err());
    assert!(add_text(&pdf, 1, 0.0, 0.0, "x", 0.0, [0.0; 3], "Helvetica").is_err());
    assert!(add_text(&pdf, 1, 0.0, 0.0, "x", 12.0, [0.0; 3], "Comic").is_err());
}
