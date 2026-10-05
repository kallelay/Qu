//! Tests for `extras.rs`, on tiny PDFs built here.

use super::*;
use lopdf::{dictionary, Stream};
use std::io::Cursor;

fn finish(mut doc: Document, pages_id: ObjectId, page: ObjectId) -> Vec<u8> {
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }),
    );
    let cat = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", cat);
    let mut buf = Cursor::new(Vec::new());
    doc.save_to(&mut buf).unwrap();
    buf.into_inner()
}

/// One page with Courier, the given content stream and XObjects.
fn pdf(content: &str, xobjects: Vec<(&str, Stream)>) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Courier",
    });
    let mut xo = lopdf::Dictionary::new();
    for (name, s) in xobjects {
        let id = doc.add_object(s);
        xo.set(name, id);
    }
    let res = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
        "XObject" => xo,
    });
    let cid = doc.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => cid, "Resources" => res,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    finish(doc, pages_id, page)
}

fn image(w: i64, h: i64, cs: Object, bpc: i64, data: Vec<u8>) -> Stream {
    Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => w, "Height" => h,
            "ColorSpace" => cs, "BitsPerComponent" => bpc,
        },
        data,
    )
}

fn texts(bytes: &[u8]) -> Vec<String> {
    page_lines(bytes, None).unwrap().into_iter().map(|l| l.text).collect()
}

#[test]
fn lines_group_by_baseline_and_order_top_down() {
    let b = pdf(
        "BT /F1 12 Tf 72 700 Td (Line two) Tj ET BT /F1 12 Tf 72 720 Td (Title) Tj ET \
         BT /F1 12 Tf 200 700 Td (right) Tj ET",
        vec![],
    );
    assert_eq!(texts(&b), vec!["Title", "Line two right"]);
}

#[test]
fn lines_follow_leading_operators() {
    let b = pdf("BT /F1 12 Tf 14 TL 72 720 Td (one) Tj T* (two) Tj (three) ' ET", vec![]);
    assert_eq!(texts(&b), vec!["one", "two", "three"]);
}

#[test]
fn lines_apply_the_current_transformation_matrix() {
    // A top-left-origin page: through the flip, the text drawn at y=100 is
    // HIGHER on the page than the one at y=120.
    let b = pdf(
        "q 1 0 0 -1 0 842 cm BT /F1 12 Tf 72 120 Td (second) Tj ET BT /F1 12 Tf 72 100 Td (first) Tj ET Q",
        vec![],
    );
    assert_eq!(texts(&b), vec!["first", "second"]);
}

#[test]
fn tj_word_gap_becomes_a_space() {
    let b = pdf("BT /F1 12 Tf 72 700 Td [(Hel) -20 (lo) -400 (there)] TJ ET", vec![]);
    assert_eq!(texts(&b), vec!["Hello there"]);
}

#[test]
fn normalize_expands_ligatures_and_dehyphenates() {
    assert_eq!(normalize_text("of\u{FB01}ce \u{FB03}x \u{FB00}"), "office ffix ff");
    assert_eq!(normalize_text("Fi-\nnally   done\n\n\nnext"), "Finally done\nnext");
    // Kept: capital continuation, digit hyphen.
    assert_eq!(normalize_text("well-\nKnown\n3-\n4"), "well-\nKnown\n3-\n4");
    assert_eq!(normalize_text("a\u{00A0}\u{00A0}b\tc"), "a b c");
    assert_eq!(normalize_text("\u{FB02}ow \u{FB04} \u{FB05}\u{FB06}"), "flow ffl stst");
    assert_eq!(normalize_text("inter-\nnation-\nal"), "international");
}

#[test]
fn images_list_raw_rgb_and_extract_it() {
    let b = pdf("", vec![("Im1", image(2, 1, "DeviceRGB".into(), 8, vec![255, 0, 0, 0, 0, 255]))]);
    let l = images(&b, None).unwrap();
    assert_eq!(l.len(), 1);
    assert_eq!((l[0].page, l[0].index, l[0].name.as_str()), (1, 1, "Im1"));
    assert_eq!((l[0].width, l[0].height, l[0].bits), (2, 1, 8));
    assert_eq!((l[0].colorspace.as_str(), l[0].filter.as_str(), l[0].bytes), ("DeviceRGB", "none", 6));
    assert_eq!(
        extract_image(&b, 1, 1).unwrap(),
        ExtractedImage::Rgb { width: 2, height: 1, rgb: vec![255, 0, 0, 0, 0, 255] }
    );
}

#[test]
fn indexed_four_bit_and_one_bit_gray() {
    let cs = Object::Array(vec![
        "Indexed".into(),
        "DeviceRGB".into(),
        1.into(),
        Object::string_literal(vec![10u8, 20, 30, 40, 50, 60]),
    ]);
    let g1 = image(8, 1, "DeviceGray".into(), 1, vec![0b1010_0000]);
    let ix = image(3, 1, cs, 4, vec![0x01, 0x00]);
    let b = pdf("", vec![("A", ix), ("B", g1)]);
    assert_eq!(images(&b, None).unwrap().len(), 2);
    let ExtractedImage::Rgb { rgb, .. } = extract_image(&b, 1, 1).unwrap() else { panic!() };
    assert_eq!(rgb, vec![10, 20, 30, 40, 50, 60, 10, 20, 30]);
    let ExtractedImage::Rgb { rgb, .. } = extract_image(&b, 1, 2).unwrap() else { panic!() };
    assert_eq!(rgb.iter().step_by(3).copied().collect::<Vec<_>>(), vec![255, 0, 255, 0, 0, 0, 0, 0]);
}

#[test]
fn flate_with_png_up_predictor() {
    use std::io::Write;
    // 2x2 gray, rows [10,20] and [11,22] stored as Up deltas: [10,20], [1,2].
    let raw = vec![0u8, 10, 20, 2, 1, 2];
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&raw).unwrap();
    let mut s = image(2, 2, "DeviceGray".into(), 8, z.finish().unwrap());
    s.dict.set("Filter", "FlateDecode");
    s.dict.set(
        "DecodeParms",
        dictionary! { "Predictor" => 15, "Colors" => 1, "BitsPerComponent" => 8, "Columns" => 2 },
    );
    let b = pdf("", vec![("Im1", s)]);
    assert_eq!(images(&b, None).unwrap()[0].filter, "FlateDecode");
    let ExtractedImage::Rgb { rgb, .. } = extract_image(&b, 1, 1).unwrap() else { panic!() };
    assert_eq!(rgb.iter().step_by(3).copied().collect::<Vec<_>>(), vec![10, 20, 11, 22]);
}

#[test]
fn dct_is_passed_through_and_unsupported_filters_are_named() {
    let mut j = image(1, 1, "DeviceRGB".into(), 8, vec![0xFF, 0xD8, 0xFF, 0xD9]);
    j.dict.set("Filter", "DCTDecode");
    let mut jb = image(1, 1, "DeviceGray".into(), 1, vec![0]);
    jb.dict.set("Filter", "JBIG2Decode");
    let mut cc = image(1, 1, "DeviceGray".into(), 1, vec![0]);
    cc.dict.set("Filter", "CCITTFaxDecode");
    let mut jx = image(1, 1, "DeviceGray".into(), 8, vec![0]);
    jx.dict.set("Filter", "JPXDecode");
    let b = pdf("", vec![("A", j), ("B", jb), ("C", cc), ("D", jx)]);
    assert_eq!(extract_image(&b, 1, 1).unwrap(), ExtractedImage::Jpeg(vec![0xFF, 0xD8, 0xFF, 0xD9]));
    for (i, f) in [(2, "JBIG2Decode"), (3, "CCITTFaxDecode"), (4, "JPXDecode")] {
        let e = extract_image(&b, 1, i).unwrap_err();
        assert!(e.contains(f), "{e}");
    }
}

#[test]
fn extract_image_validates_page_and_index() {
    let b = pdf("", vec![("Im1", image(1, 1, "DeviceGray".into(), 8, vec![0]))]);
    assert!(extract_image(&b, 2, 1).unwrap_err().contains("no page 2"));
    let e = extract_image(&b, 1, 2).unwrap_err();
    assert!(e.contains("1 image(s)") && e.contains("index 2"), "{e}");
    assert!(extract_image(&b, 1, 0).is_err());
    assert!(images(&b, Some(&[9])).unwrap_err().contains("images: no page 9"));
}

#[test]
fn form_xobject_images_are_found_one_level_deep() {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let img_id = doc.add_object(image(1, 1, "DeviceGray".into(), 8, vec![7]));
    let mut form = Stream::new(dictionary! { "Type" => "XObject", "Subtype" => "Form" }, b"".to_vec());
    form.dict.set("Resources", dictionary! { "XObject" => dictionary! { "Inner" => img_id } });
    let form_id = doc.add_object(form);
    let res = doc.add_object(dictionary! { "XObject" => dictionary! { "Fm1" => form_id } });
    let cid = doc.add_object(Stream::new(dictionary! {}, b"".to_vec()));
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => cid, "Resources" => res,
        "MediaBox" => vec![0.into(), 0.into(), 10.into(), 10.into()],
    });
    let b = finish(doc, pages_id, page);
    let l = images(&b, None).unwrap();
    assert_eq!(l.len(), 1);
    assert_eq!(l[0].name, "Fm1/Inner");
    let ExtractedImage::Rgb { rgb, .. } = extract_image(&b, 1, 1).unwrap() else { panic!() };
    assert_eq!(rgb, vec![7, 7, 7]);
}

#[test]
fn html_has_sections_paragraphs_and_escapes() {
    let b = pdf(
        "BT /F1 12 Tf 72 720 Td (a < b & c > d) Tj ET BT /F1 12 Tf 72 706 Td (still the same para-) Tj ET \
         BT /F1 12 Tf 72 692 Td (graph) Tj ET BT /F1 12 Tf 72 500 Td (Second paragraph) Tj ET",
        vec![("Im1", image(1, 1, "DeviceRGB".into(), 8, vec![1, 2, 3]))],
    );
    let h = to_html(&b, None, false).unwrap();
    assert_eq!(
        h,
        "<section class=\"page\" data-page=\"1\">\n<p>a &lt; b &amp; c &gt; d still the same paragraph</p>\n<p>Second paragraph</p>\n</section>\n"
    );
    let h = to_html(&b, None, true).unwrap();
    assert!(h.contains("<img alt=\"Im1\" data-index=\"1\" src=\"data:image/png;base64,iVBORw0KGgo"), "{h}");
}

#[test]
fn base64_and_crc() {
    assert_eq!(base64(b"Man"), "TWFu");
    assert_eq!(base64(b"Ma"), "TWE=");
    assert_eq!(base64(b"M"), "TQ==");
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
}

#[test]
fn html_escaping_covers_quotes_so_a_pdf_name_cannot_break_an_attribute() {
    // Review finding: an image name from the PDF went into `alt="..."`
    // with only & < > escaped; a `"` is an ordinary character in a PDF name.
    assert_eq!(crate::extras::esc("Im\"onerror=\"x"), "Im&quot;onerror=&quot;x");
    assert_eq!(crate::extras::esc("a<b>&c"), "a&lt;b&gt;&amp;c");
}
