//! `pdf.extract_text(normalize=, lines=)`, `pdf.images`, `pdf.extract_image`
//! and `pdf.to_html`, through the language, on PDFs built here.
#![cfg(feature = "pdf")]

use lopdf::{dictionary, Document, Object, Stream};
use qu_interp::{Interp, Value};

fn tmp(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("qu_pdf_ti_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name).to_string_lossy().replace('\\', "/")
}

/// One page: Courier text from `content`, a 2x1 RGB image `Im1` and a
/// JBIG2-tagged image `Im2`.
fn make(path: &str, content: &str) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Courier" });
    let im1 = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => 2, "Height" => 1,
            "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8,
        },
        vec![255, 0, 0, 0, 0, 255],
    ));
    let im2 = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => 1, "Height" => 1,
            "ColorSpace" => "DeviceGray", "BitsPerComponent" => 1, "Filter" => "JBIG2Decode",
        },
        vec![0],
    ));
    let res = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font },
        "XObject" => dictionary! { "Im1" => im1, "Im2" => im2 },
    });
    let cid = doc.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => cid, "Resources" => res,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }),
    );
    let cat = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", cat);
    doc.save(path).unwrap();
}

const TEXT: &str = "BT /F1 12 Tf 72 720 Td (Fi-) Tj ET BT /F1 12 Tf 72 706 Td (nally  a < b) Tj ET \
                    BT /F1 12 Tf 72 400 Td (Far away) Tj ET";

fn run(src: &str) -> Interp {
    let mut it = Interp::new();
    it.run(src).unwrap_or_else(|e| panic!("run failed: {e}\nsrc:\n{src}"));
    it
}

fn err(src: &str) -> String {
    let mut it = Interp::new();
    it.run(src).expect_err("expected an error").to_string()
}

fn strs(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::List(xs)) => xs
            .iter()
            .map(|x| match x {
                Value::Str(s) => s.clone(),
                o => panic!("{o:?}"),
            })
            .collect(),
        o => panic!("{o:?}"),
    }
}

#[test]
fn extract_text_defaults_are_unchanged_and_options_work() {
    let p = tmp("t.pdf");
    make(&p, TEXT);
    let it = run(&format!(
        "import pdf\nflat = pdf.extract_text(\"{p}\")\nnorm = pdf.extract_text(\"{p}\", normalize=true)\n\
         ls = pdf.extract_text(\"{p}\", lines=true)\nln = pdf.extract_text(\"{p}\", 1, lines=true, normalize=true)"
    ));
    let Some(Value::Str(flat)) = it.get("flat") else { panic!() };
    assert!(flat.contains("Fi-") && flat.contains("nally  a < b"), "{flat:?}");
    let Some(Value::Str(norm)) = it.get("norm") else { panic!() };
    assert!(norm.contains("Finally a < b"), "{norm:?}");
    assert_eq!(strs(it.get("ls")), vec!["Fi-", "nally  a < b", "Far away"]);
    assert_eq!(strs(it.get("ln")), vec!["Finally a < b", "Far away"]);
}

#[test]
fn images_and_extract_image() {
    let p = tmp("i.pdf");
    make(&p, TEXT);
    let it = run(&format!(
        "import pdf\nl = pdf.images(\"{p}\")\nimg = pdf.extract_image(\"{p}\", 1, 1)\nn = length(l)"
    ));
    let Some(Value::Num(n)) = it.get("n") else { panic!() };
    assert_eq!(*n, 2.0);
    let Some(Value::List(l)) = it.get("l") else { panic!() };
    let Value::Record(r) = &l[0] else { panic!() };
    let get = |k: &str| r.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap();
    assert_eq!(format!("{:?}", get("name")), format!("{:?}", Value::Str("Im1".into())));
    assert_eq!(format!("{:?}", get("width")), format!("{:?}", Value::Num(2.0)));
    assert_eq!(format!("{:?}", get("colorspace")), format!("{:?}", Value::Str("DeviceRGB".into())));
    assert_eq!(format!("{:?}", get("filter")), format!("{:?}", Value::Str("none".into())));
    let Some(Value::Image(img)) = it.get("img") else { panic!() };
    assert_eq!((img.width, img.height), (2, 1));
    assert_eq!(img.pixels, vec![255, 0, 0, 0, 0, 255]);
}

#[test]
fn unsupported_filter_and_bad_arguments_name_the_function() {
    let p = tmp("e.pdf");
    make(&p, TEXT);
    let e = err(&format!("import pdf\nx = pdf.extract_image(\"{p}\", 1, 2)"));
    assert!(e.contains("JBIG2Decode"), "{e}");
    let e = err(&format!("import pdf\nx = pdf.extract_image(\"{p}\", 1)"));
    assert!(e.contains("extract_image") && e.contains("index"), "{e}");
    let e = err(&format!("import pdf\nx = pdf.extract_image(\"{p}\", 1, 9)"));
    assert!(e.contains("extract_image") && e.contains("2 image(s)"), "{e}");
    let e = err(&format!("import pdf\nx = pdf.extract_image(\"{p}\", 1, 0)"));
    assert!(e.contains("extract_image") && e.contains("image index"), "{e}");
    let e = err(&format!("import pdf\nx = pdf.extract_text(\"{p}\", lines=\"yes\")"));
    assert!(e.contains("extract_text") && e.contains("lines=") && e.contains("true or false"), "{e}");
    let e = err("import pdf\nx = pdf.images(3)");
    assert!(e.contains("images"), "{e}");
    let e = err(&format!("import pdf\nx = pdf.to_html(\"{p}\", images=1)"));
    assert!(e.contains("to_html") && e.contains("images="), "{e}");
    let e = err(&format!("import pdf\nx = pdf.to_html(\"{p}\", 4)"));
    assert!(e.contains("to_html: no page 4"), "{e}");
    let e = err(&format!("import pdf\nx = pdf.images(\"{p}\", 0)"));
    assert!(e.contains("images"), "{e}");
}

#[test]
fn to_html_gives_sections_and_paragraphs() {
    let p = tmp("h.pdf");
    make(&p, TEXT);
    let it = run(&format!("import pdf\nh = pdf.to_html(\"{p}\")\nhi = pdf.to_html(\"{p}\", images=true)"));
    let Some(Value::Str(h)) = it.get("h") else { panic!() };
    assert_eq!(
        h,
        "<section class=\"page\" data-page=\"1\">\n<p>Finally a &lt; b</p>\n<p>Far away</p>\n</section>\n"
    );
    let Some(Value::Str(hi)) = it.get("hi") else { panic!() };
    assert!(hi.contains("data:image/png;base64,") && hi.contains("<!-- image 2 skipped"), "{hi}");
}
