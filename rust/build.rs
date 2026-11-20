use miniz_oxide::deflate::compress_to_vec;
use std::{env, fs, io::Write, path::PathBuf};

#[path = "src/icon_sources.rs"]
mod icon_sources;

macro_rules! sources {
    ($($name:literal),+ $(,)?) => {
        &[
            $(($name, concat!("../assets/icons/", $name, ".svg"), None, &[20, 24, 28, 36]),)+
            ("chevron-down", "../assets/icons/tabler/chevron-down.svg", Some(icon_sources::CHEVRON_COLOR), &[16]),
        ]
    };
}

#[cfg(feature = "ocr")]
macro_rules! ocr_sources {
    ($($variant:ident, $name:literal, $size:literal;)+) => {
        &[
            $(($name, concat!("../assets/icons/tabler/", $name, ".svg"), None, &[$size]),)+
        ]
    };
}

fn main() {
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bin=YiSou=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-bin=YiSou=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }
    let destination = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    write_settings_translations(&destination);
    let mut data = fs::File::create(destination.join("icons.bin")).expect("create icon data");
    let mut index =
        fs::File::create(destination.join("icons_index.rs")).expect("create icon index");
    let mut offset = 0;
    writeln!(
        index,
        "fn icon_range(id: &str, edge: u32) -> Option<std::ops::Range<usize>> {{\n    match (id, edge) {{"
    ).unwrap();
    type Source = (&'static str, &'static str, Option<u32>, &'static [u32]);
    let sources: &[Source] = icon_sources::engine_icons!(sources);
    #[cfg(feature = "ocr")]
    let ocr_sources: &[Source] = icon_sources::ocr_icons!(ocr_sources);
    #[cfg(not(feature = "ocr"))]
    let ocr_sources: &[Source] = &[];
    for &(id, source, color, sizes) in sources.iter().chain(ocr_sources) {
        println!("cargo:rerun-if-changed={source}");
        let svg = fs::read(source).expect(source);
        let mut edges = std::collections::BTreeSet::new();
        for dpi in [96, 120, 144, 168, 192] {
            edges.extend(sizes.iter().map(|size| (size * dpi + 48) / 96));
        }
        for edge in edges {
            let pixels = yisou_svg::render(&svg, edge, color).expect(source);
            let bytes: Vec<_> = pixels.into_iter().flat_map(u32::to_le_bytes).collect();
            let compressed = compress_to_vec(&bytes, 6);
            data.write_all(&compressed).unwrap();
            let end = offset + compressed.len();
            writeln!(index, "        ({id:?}, {edge}) => Some({offset}..{end}),").unwrap();
            offset = end;
        }
    }
    writeln!(index, "        _ => None,\n    }}\n}}").unwrap();
    println!("cargo:rerun-if-changed=src/icon_sources.rs");
}

fn write_settings_translations(destination: &std::path::Path) {
    let source = "../src/platform/strings.inc";
    println!("cargo:rerun-if-changed={source}");
    let strings = fs::read_to_string(source).expect("read shared UI strings");
    let mut translations = std::collections::BTreeMap::new();
    for line in strings.lines() {
        let Some((_, values)) = line.split_once(", L\"") else {
            continue;
        };
        let Some((chinese, english)) = values
            .strip_suffix("\")")
            .and_then(|s| s.split_once("\", L\""))
        else {
            continue;
        };
        if !chinese.contains('\\') && !english.contains('\\') {
            translations.entry(chinese).or_insert(english);
        }
    }
    let mut file = fs::File::create(destination.join("settings_strings.rs"))
        .expect("create translation output");
    writeln!(file, "fn english(text: &str) -> &str {{\n    match text {{").unwrap();
    for (chinese, english) in translations {
        writeln!(file, "        {chinese:?} => {english:?},").unwrap();
    }
    writeln!(file, "        _ => text,\n    }}\n}}").unwrap();
}
