mod text_selection;

use rapid_structure_rs::{
    EngineConfig, ImageInput, OcrEngine, OcrOutput, OcrRequest, OwnedPixelBuffer, Polygon,
    PreprocessPolicy, ProviderPreference, RapidOcrEngine, RapidOcrError, RecognitionPolicy,
    RectU32, TextOrder, WordOutputMode,
};
pub use text_selection::{Quad, TextPage};

pub const TINY_DOWNLOADS: [&str; 3] = [
    "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.1/onnx/PP-OCRv6/det/PP-OCRv6_det_tiny.onnx",
    "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.1/onnx/PP-OCRv6/rec/PP-OCRv6_rec_tiny.onnx",
    "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.9.1/paddle/PP-OCRv6/rec/PP-OCRv6_rec_tiny/ppocrv6_tiny_dict.txt",
];
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy)]
pub enum Model {
    Tiny,
    Small,
}

impl Model {
    pub fn directory(self) -> &'static str {
        match self {
            Self::Tiny => "tiny",
            Self::Small => "small",
        }
    }

    fn assets(self) -> [(&'static str, &'static str); 3] {
        match self {
            Self::Tiny => [
                (
                    "PP-OCRv6_det_tiny.onnx",
                    "f42c0fbd294d95eac1a550e131b277dac97462c8025fa4b6c3cec1b7894bd3d5",
                ),
                (
                    "PP-OCRv6_rec_tiny.onnx",
                    "e16e242de5937ad92609223f19bc2aff3727ee40b095f996907c24749bad251b",
                ),
                (
                    "ppocrv6_tiny_dict.txt",
                    "c5cbe34ef40c29c4df07ed012bf96569cb69a2d2a01a07027e9f13cb832bd9cd",
                ),
            ],
            Self::Small => [
                (
                    "PP-OCRv6_det_small.onnx",
                    "090f04abcd9d9a7498bc4ebf677e4cb9bdce1fe4197ddb7e529f1ef44e1ff94f",
                ),
                (
                    "PP-OCRv6_rec_small.onnx",
                    "6f327246b50388f3c176ae304bd95767ea6dc0c9ae92153ef8cbe210b3c14884",
                ),
                (
                    "ppocrv6_dict.txt",
                    "b5f2bfe2bdd9448429e3e82b51c789775d9b42f2403d082b00662eb77e401c5d",
                ),
            ],
        }
    }

    pub fn validate_directory(self, directory: &Path) -> Result<(), Error> {
        self.checked_files(directory).map(|_| ())
    }

    fn checked_files(self, directory: &Path) -> Result<[PathBuf; 3], Error> {
        let check = |(name, expected): (&str, &str)| {
            let path = directory.join(name);
            let mut hash = Sha256::new();
            File::open(&path)
                .and_then(|mut file| io::copy(&mut file, &mut hash))
                .map_err(|source| Error::ModelFile {
                    path: path.clone(),
                    source,
                })?;
            if format!("{:x}", hash.finalize()) != expected {
                return Err(Error::Checksum(path));
            }
            Ok(path)
        };
        let [detector, recognizer, dictionary] = self.assets();
        Ok([check(detector)?, check(recognizer)?, check(dictionary)?])
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{path}: {source}")]
    ModelFile { path: PathBuf, source: io::Error },
    #[error("model checksum mismatch: {0}")]
    Checksum(PathBuf),
    #[error(transparent)]
    Recognition(#[from] RapidOcrError),
}

pub struct Recognizer(RapidOcrEngine);

impl Recognizer {
    pub fn open(root: &Path, model: Model) -> Result<Self, Error> {
        Self::open_directory(&root.join(model.directory()), model)
    }

    pub fn open_directory(directory: &Path, model: Model) -> Result<Self, Error> {
        let [detector, recognizer, dictionary] = model.checked_files(directory)?;
        let mut config = EngineConfig::default();
        config.global.use_cls = false;
        config.global.max_side_len = 1600;
        config.runtime.provider_preference = ProviderPreference::Cpu;
        config.runtime.intra_threads = Some(4);
        config.runtime.inter_threads = Some(1);
        config.runtime.rayon_threads = Some(2);
        config.runtime.enable_cpu_mem_arena = false;
        config.det.limit_type = "max".into();
        config.det.model_path = Some(detector);
        config.det.allow_download = false;
        config.rec.model.model_path = Some(recognizer);
        config.rec.model.rec_keys_path = Some(dictionary);
        config.rec.model.allow_download = false;
        Ok(Self(RapidOcrEngine::new(config)?))
    }

    pub fn recognize(&mut self, pixels: OwnedPixelBuffer) -> Result<String, Error> {
        let output = self.run(pixels, WordOutputMode::Off, None)?;
        Ok(output.plain_text(TextOrder::Reading).trim().to_owned())
    }

    pub fn recognize_text_page(&mut self, pixels: OwnedPixelBuffer) -> Result<TextPage, Error> {
        let output = match self.run(pixels.clone(), WordOutputMode::Chars, None) {
            Err(error) if outside_image(&error) => self.recover_text_page(pixels)?,
            result => result?,
        };
        Ok(TextPage::from_output(output))
    }

    fn recover_text_page(&mut self, pixels: OwnedPixelBuffer) -> Result<OcrOutput, Error> {
        // RapidStructure validates character polygons against the full image. Add a margin
        // for validation while the ROI keeps the pixels passed to the models identical.
        if let Some((padded, roi)) = pad_for_character_boxes(&pixels) {
            match self.run(padded, WordOutputMode::Chars, Some(roi)) {
                Ok(mut output) => {
                    for region in &mut output.regions {
                        if let Some(polygon) = &mut region.polygon {
                            clip_to_roi(polygon, roi);
                        }
                        if let Some(words) =
                            region.recognition.as_mut().and_then(|r| r.words.as_mut())
                        {
                            for word in words {
                                clip_to_roi(&mut word.polygon, roi);
                            }
                        }
                    }
                    output.image.original_size.width = roi.width;
                    output.image.original_size.height = roi.height;
                    return Ok(output);
                }
                Err(error) if !outside_image(&error) => return Err(error),
                Err(_) => {}
            }
        }
        self.run(pixels, WordOutputMode::Off, None)
    }

    fn run(
        &mut self,
        pixels: OwnedPixelBuffer,
        words: WordOutputMode,
        roi: Option<RectU32>,
    ) -> Result<OcrOutput, Error> {
        Ok(self.0.recognize(OcrRequest {
            input: ImageInput::Pixels(pixels),
            roi,
            scale_hint: None,
            stages: Default::default(),
            preprocess: Default::default(),
            detection: Default::default(),
            recognition: RecognitionPolicy { words },
            output: Default::default(),
            formula: Default::default(),
            table: Default::default(),
        })?)
    }
}

fn outside_image(error: &Error) -> bool {
    matches!(error, Error::Recognition(RapidOcrError::InvalidInput(message))
        if message.starts_with("polygon point ") && message.contains(" is outside image "))
}

fn pad_for_character_boxes(pixels: &OwnedPixelBuffer) -> Option<(OwnedPixelBuffer, RectU32)> {
    const MARGIN: u32 = 32;
    let width = pixels.width.checked_add(2 * MARGIN)?;
    let height = pixels.height.checked_add(2 * MARGIN)?;
    if u64::from(width) * u64::from(height) > PreprocessPolicy::default().max_decode_pixels {
        return None;
    }
    let channels = pixels.format.bytes_per_pixel();
    let row_bytes = pixels.width as usize * channels;
    let stride = width as usize * channels;
    let mut data = vec![0; stride * height as usize];
    let inset = MARGIN as usize * stride + MARGIN as usize * channels;
    for row in 0..pixels.height as usize {
        let src = row * pixels.stride;
        let dst = inset + row * stride;
        data[dst..dst + row_bytes].copy_from_slice(&pixels.data[src..src + row_bytes]);
    }
    let padded = OwnedPixelBuffer {
        width,
        height,
        stride,
        format: pixels.format,
        bottom_up: pixels.bottom_up,
        data: data.into(),
    };
    let roi = RectU32 {
        x: MARGIN,
        y: MARGIN,
        width: pixels.width,
        height: pixels.height,
    };
    Some((padded, roi))
}

fn clip_to_roi(polygon: &mut Polygon, roi: RectU32) {
    for [x, y] in &mut polygon.points {
        *x = (*x - roi.x as f32).clamp(0.0, roi.width as f32);
        *y = (*y - roi.y as f32).clamp(0.0, roi.height as f32);
    }
}

pub use rapid_structure_rs::{OwnedPixelBuffer as Pixels, PixelFormat};

#[cfg(test)]
#[test]
fn text_at_the_screenshot_boundary_remains_searchable() {
    let Some(directory) = std::env::var_os("YISOU_TEST_OCR_MODELS") else {
        return;
    };
    let mut recognizer = Recognizer::open_directory(Path::new(&directory), Model::Tiny).unwrap();
    let image = image::load_from_memory(include_bytes!("../tests/fixtures/ocr-text-at-bottom.png"))
        .unwrap()
        .into_rgb8();
    let (width, height) = image.dimensions();
    let page = recognizer
        .recognize_text_page(Pixels {
            width,
            height,
            stride: width as usize * 3,
            format: PixelFormat::Rgb8,
            bottom_up: false,
            data: image.into_raw().into(),
        })
        .unwrap();
    assert_eq!(page.text(None), "Read the docs\nRust error E0425");
    let hit = page.hit([278.0, 345.0]).expect("selectable bottom line");
    assert_eq!(page.text(Some(&page.word(hit))), "E0425");
}
