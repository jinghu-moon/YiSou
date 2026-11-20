#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use serde::Deserialize;
    use std::{path::PathBuf, time::Instant};
    use yisou::ocr::{Model, PixelFormat, Pixels, Recognizer};

    #[derive(Deserialize)]
    struct Sample {
        image: PathBuf,
        text: String,
        #[serde(default)]
        exact_tokens: Vec<String>,
    }

    let mut args = std::env::args_os().skip(1);
    let root = PathBuf::from(args.next().ok_or("expected model directory")?);
    let model = match args
        .next()
        .and_then(|arg| arg.into_string().ok())
        .as_deref()
    {
        Some("tiny") => Model::Tiny,
        Some("small") => Model::Small,
        _ => return Err("expected tiny or small".into()),
    };
    let manifest = PathBuf::from(args.next().ok_or("expected sample manifest")?);
    let samples: Vec<Sample> = serde_json::from_slice(&std::fs::read(&manifest)?)?;
    if samples.is_empty() {
        return Err("empty sample manifest".into());
    }
    let directory = manifest
        .parent()
        .ok_or("expected manifest parent directory")?;
    let start = Instant::now();
    let mut recognizer = Recognizer::open(&root, model)?;
    println!("init_ms={:.1}", start.elapsed().as_secs_f64() * 1e3);
    let mut timings = Vec::new();
    let (mut edits, mut characters, mut exact, mut tokens) = (0, 0, 0, 0);
    for (index, sample) in samples.iter().enumerate() {
        let image = image::open(directory.join(&sample.image))?.into_rgb8();
        let (width, height) = image.dimensions();
        let pixels = Pixels {
            width,
            height,
            stride: width as usize * 3,
            format: PixelFormat::Rgb8,
            bottom_up: false,
            data: image.into_raw().into(),
        };
        let start = Instant::now();
        let recognized = recognizer.recognize(pixels.clone())?;
        let elapsed = start.elapsed().as_secs_f64() * 1e3;
        if index == 0 {
            println!("first_inference_ms={elapsed:.1}");
        }
        for _ in 0..3 {
            let start = Instant::now();
            recognizer.recognize(pixels.clone())?;
            timings.push(start.elapsed().as_secs_f64() * 1e3);
        }
        let expected: Vec<_> = sample.text.chars().filter(|c| !c.is_whitespace()).collect();
        let actual: Vec<_> = recognized.chars().filter(|c| !c.is_whitespace()).collect();
        let errors = distance(&expected, &actual);
        let matched = sample
            .exact_tokens
            .iter()
            .filter(|token| recognized.contains(token.as_str()))
            .count();
        println!(
            "{} {}x{} edits={errors}/{} tokens={matched}/{}",
            sample.image.display(),
            width,
            height,
            expected.len(),
            sample.exact_tokens.len()
        );
        edits += errors;
        characters += expected.len();
        exact += matched;
        tokens += sample.exact_tokens.len();
    }
    timings.sort_by(f64::total_cmp);
    let sample = |fraction: f64| timings[(fraction * timings.len() as f64).ceil() as usize - 1];
    println!(
        "model={} CER={:.2}% exact_tokens={exact}/{tokens} warm_p50_ms={:.1} warm_p95_ms={:.1}",
        model.directory(),
        edits as f64 * 100.0 / characters.max(1) as f64,
        sample(0.5),
        sample(0.95)
    );
    println!(
        "peak_working_set_mib={:.1}",
        rapid_structure_rs::peak_working_set_bytes().unwrap_or(0) as f64 / 1048576.0
    );
    Ok(())
}

#[cfg(windows)]
fn distance(expected: &[char], actual: &[char]) -> usize {
    let mut previous: Vec<_> = (0..=actual.len()).collect();
    let mut current = previous.clone();
    for (i, expected) in expected.iter().enumerate() {
        current[0] = i + 1;
        for (j, actual) in actual.iter().enumerate() {
            current[j + 1] = (previous[j] + usize::from(expected != actual))
                .min(current[j] + 1)
                .min(previous[j + 1] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[actual.len()]
}

#[cfg(not(windows))]
fn main() {
    eprintln!("OCR is available on Windows x64 MSVC");
}
