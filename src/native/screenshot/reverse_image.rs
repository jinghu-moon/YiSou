use super::Snapshot;
use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use serde_json::Value;
use std::{
    ffi::c_void,
    io,
    ptr::{NonNull, null, null_mut},
    sync::Arc,
    thread,
};
use windows_sys::{Win32::Networking::WinHttp::*, w};
use yisou::engine::search_url;

const BOUNDARY: &str = "YiSouSearchScreenshot_72b6ac9043e8";
const MAX_RESPONSE: usize = 1024 * 1024;
const YANDEX_PATH: &str = "/images/touch/search?rpt=imageview&format=json&request=%7B%22blocks%22%3A%5B%7B%22block%22%3A%22cbir-uploader__get-cbir-id%22%7D%5D%7D";

#[derive(Clone, Copy)]
pub(super) enum Engine {
    Yandex,
    GoogleLens,
}

impl Engine {
    pub fn name(self) -> &'static str {
        match self {
            Self::Yandex => "Yandex Images",
            Self::GoogleLens => "Google Lens",
        }
    }

    fn upload(self, png: &[u8]) -> io::Result<String> {
        let (host, path) = match self {
            Self::Yandex => ("yandex.com", YANDEX_PATH),
            Self::GoogleLens => ("lens.google.com", "/v3/upload"),
        };
        self.result_url(self.post(host, 443, path, png)?)
    }

    fn post(self, host: &str, port: u16, path: &str, png: &[u8]) -> io::Result<Response> {
        let (field, extra, max_bytes) = match self {
            Self::Yandex => ("upfile", String::new(), 5 * 1024 * 1024),
            Self::GoogleLens => (
                "encoded_image",
                format!(
                    "\r\n--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"image_url\"\r\n\r\n"
                ),
                20 * 1024 * 1024,
            ),
        };
        if png.len() > max_bytes {
            return Err(io::Error::other(
                "Image exceeds the service upload limit. Select a smaller region.",
            ));
        }
        let prefix = format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"screenshot.png\"\r\nContent-Type: image/png\r\n\r\n"
        );
        let suffix = format!("{extra}\r\n--{BOUNDARY}--\r\n");
        post(
            self,
            host,
            port,
            path,
            [prefix.as_bytes(), png, suffix.as_bytes()],
        )
    }

    fn result_url(self, response: Response) -> io::Result<String> {
        match (self, response.status) {
            (Self::Yandex, 200) => {
                let json: Value = serde_json::from_slice(&response.body)?;
                let id = json
                    .pointer("/blocks/0/params/cbirId")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| io::Error::other("Yandex returned no image search result."))?;
                Ok(search_url(
                    "https://yandex.com/images/search?cbir_id=%s&rpt=imageview&tabInt=1",
                    id,
                ))
            }
            (Self::GoogleLens, 302 | 303) => response
                .location
                .filter(|url| {
                    url.starts_with("https://www.google.com/search?") && url.contains("vsrid=")
                })
                .ok_or_else(|| io::Error::other("Google Lens returned no image search result.")),
            (_, status) => Err(io::Error::other(format!(
                "Image search service returned HTTP {status}."
            ))),
        }
    }
}

#[cfg(test)]
fn search(
    engines: &[Engine],
    snapshot: Snapshot,
    report: impl Fn(Engine, io::Result<String>) + Sync,
) {
    let png = match encode_png(snapshot) {
        Ok(png) => png,
        Err(error) => {
            for &engine in engines {
                report(engine, Err(io::Error::other(error.to_string())));
            }
            return;
        }
    };
    search_png(engines, &png, report);
}

pub(super) fn search_png(
    engines: &[Engine],
    png: &[u8],
    report: impl Fn(Engine, io::Result<String>) + Sync,
) {
    let Some((&first, others)) = engines.split_first() else {
        return;
    };
    let upload = |engine: Engine| report(engine, engine.upload(png));
    thread::scope(|scope| {
        for &engine in others {
            if let Err(error) = thread::Builder::new()
                .name("image-search-provider".into())
                .spawn_scoped(scope, move || upload(engine))
            {
                report(engine, Err(error));
            }
        }
        upload(first);
    });
}

pub(super) fn encode_png(mut snapshot: Snapshot) -> io::Result<Vec<u8>> {
    let width = snapshot.width();
    let height = snapshot.height();
    for pixel in Arc::make_mut(&mut snapshot.data).chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(&snapshot.data, width, height, ExtendedColorType::Rgba8)
        .map_err(io::Error::other)?;
    Ok(png)
}

struct HttpHandle(NonNull<c_void>);

impl HttpHandle {
    fn new(handle: *mut c_void) -> io::Result<Self> {
        NonNull::new(handle)
            .map(Self)
            .ok_or_else(io::Error::last_os_error)
    }

    fn raw(&self) -> *mut c_void {
        self.0.as_ptr()
    }
}

impl Drop for HttpHandle {
    fn drop(&mut self) {
        unsafe { WinHttpCloseHandle(self.raw()) };
    }
}

struct Response {
    status: u32,
    location: Option<String>,
    body: Vec<u8>,
}

fn check(success: i32) -> io::Result<()> {
    if success == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn post(
    engine: Engine,
    host: &str,
    port: u16,
    path: &str,
    body: [&[u8]; 3],
) -> io::Result<Response> {
    let wide = super::super::wide;
    let headers = match engine {
        Engine::Yandex => format!(
            "Content-Type: multipart/form-data; boundary={BOUNDARY}\r\nX-Requested-With: XMLHttpRequest\r\nAccept: application/json\r\n"
        ),
        Engine::GoogleLens => format!("Content-Type: multipart/form-data; boundary={BOUNDARY}\r\n"),
    };
    unsafe {
        let secure = port == 443;
        let session = HttpHandle::new(WinHttpOpen(
            w!("YiSou"),
            if secure {
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY
            } else {
                WINHTTP_ACCESS_TYPE_NO_PROXY
            },
            null(),
            null(),
            0,
        ))?;
        check(WinHttpSetTimeouts(
            session.raw(),
            5_000,
            8_000,
            15_000,
            20_000,
        ))?;
        let connection =
            HttpHandle::new(WinHttpConnect(session.raw(), wide(host).as_ptr(), port, 0))?;
        let request = HttpHandle::new(WinHttpOpenRequest(
            connection.raw(),
            w!("POST"),
            wide(path).as_ptr(),
            null(),
            null(),
            null(),
            if secure { WINHTTP_FLAG_SECURE } else { 0 },
        ))?;
        let request = request.raw();
        let redirect = WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
        check(WinHttpSetOption(
            request,
            WINHTTP_OPTION_REDIRECT_POLICY,
            (&redirect as *const u32).cast(),
            size_of::<u32>() as u32,
        ))?;
        let total = body.iter().map(|part| part.len() as u32).sum();
        check(WinHttpSendRequest(
            request,
            wide(&headers).as_ptr(),
            u32::MAX,
            null_mut(),
            0,
            total,
            0,
        ))?;
        // Reuse the encoded image buffer across simultaneous requests.
        for part in body {
            let mut written = 0;
            check(WinHttpWriteData(
                request,
                part.as_ptr().cast(),
                part.len() as u32,
                &mut written,
            ))?;
            if written as usize != part.len() {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "Incomplete image upload",
                ));
            }
        }
        check(WinHttpReceiveResponse(request, null_mut()))?;
        let mut status = 0u32;
        let mut length = size_of::<u32>() as u32;
        check(WinHttpQueryHeaders(
            request,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            null(),
            (&mut status as *mut u32).cast(),
            &mut length,
            null_mut(),
        ))?;
        let location = if matches!(engine, Engine::GoogleLens) && matches!(status, 302 | 303) {
            let mut length = 0;
            WinHttpQueryHeaders(
                request,
                WINHTTP_QUERY_LOCATION,
                null(),
                null_mut(),
                &mut length,
                null_mut(),
            );
            if length == 0 || length > 32_768 {
                None
            } else {
                let mut data = vec![0u16; length.div_ceil(2) as usize];
                check(WinHttpQueryHeaders(
                    request,
                    WINHTTP_QUERY_LOCATION,
                    null(),
                    data.as_mut_ptr().cast(),
                    &mut length,
                    null_mut(),
                ))?;
                Some(String::from_utf16_lossy(&data[..(length / 2) as usize]))
            }
        } else {
            None
        };
        let mut response = Vec::new();
        if matches!(engine, Engine::Yandex) && status == 200 {
            let mut buffer = [0u8; 8192];
            loop {
                let mut length = 0;
                check(WinHttpReadData(
                    request,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    &mut length,
                ))?;
                if length == 0 {
                    break;
                }
                if response.len() + length as usize > MAX_RESPONSE {
                    return Err(io::Error::other("Image search response is too large."));
                }
                response.extend_from_slice(&buffer[..length as usize]);
            }
        }
        Ok(Response {
            status,
            location,
            body: response,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::TcpListener,
        thread,
        time::Duration,
    };
    use windows_sys::Win32::Foundation::RECT;

    fn sample() -> Snapshot {
        Snapshot {
            area: RECT {
                left: 20,
                top: 40,
                right: 21,
                bottom: 42,
            },
            data: Arc::from([19, 38, 57, 255, 60, 90, 120, 255]),
        }
    }

    #[test]
    fn image_search_posts_exact_pixels_and_parses_yandex_result() {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let worker = thread::spawn(move || {
            let (mut socket, _) = server.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(&mut socket);
            let mut headers = Vec::new();
            let mut count = 0;
            loop {
                let mut line = String::new();
                assert_ne!(reader.read_line(&mut line).unwrap(), 0);
                if line == "\r\n" {
                    break;
                }
                let header = line.trim().to_ascii_lowercase();
                if let Some(value) = header.strip_prefix("content-length:") {
                    count = value.trim().parse::<usize>().unwrap();
                }
                headers.push(header);
            }
            assert_eq!(headers[0], "post /upload http/1.1");
            assert!(
                headers
                    .iter()
                    .any(|header| header == "x-requested-with: xmlhttprequest")
            );
            let mut body = vec![0; count];
            reader.read_exact(&mut body).unwrap();
            let prefix = format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"upfile\"; filename=\"screenshot.png\"\r\nContent-Type: image/png\r\n\r\n"
            );
            let suffix = format!("\r\n--{BOUNDARY}--\r\n");
            let bytes = body
                .strip_prefix(prefix.as_bytes())
                .unwrap()
                .strip_suffix(suffix.as_bytes())
                .unwrap();
            let png = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
                .unwrap()
                .into_rgba8();
            assert_eq!(png.dimensions(), (1, 2));
            assert_eq!(png.into_raw(), [57, 38, 19, 255, 120, 90, 60, 255]);
            let response = r#"{"blocks":[{"params":{"cbirId":"7/abc&x"}}]}"#;
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",
                response.len()
            )
            .unwrap();
        });
        let engine = Engine::Yandex;
        let png = encode_png(sample()).unwrap();
        let response = engine.post("127.0.0.1", port, "/upload", &png).unwrap();
        assert_eq!(
            engine.result_url(response).unwrap(),
            "https://yandex.com/images/search?cbir_id=7%2Fabc%26x&rpt=imageview&tabInt=1"
        );
        worker.join().unwrap();
    }

    #[test]
    fn image_search_checks_google_result_and_rejects_unrelated_redirects() {
        for (url, valid) in [
            ("https://www.google.com/search?vsrid=123", true),
            ("https://www.google.com/search?q=cat", false),
            ("https://example.com/search?vsrid=123", false),
        ] {
            let response = Response {
                status: 303,
                location: Some(url.into()),
                body: Vec::new(),
            };
            assert_eq!(Engine::GoogleLens.result_url(response).is_ok(), valid);
        }
    }

    #[test]
    #[ignore = "uploads a generated fixture to the configured image search providers"]
    fn image_search_live_provider_smoke() {
        use windows_sys::Win32::Foundation::RECT;
        let data: Vec<u8> = (0..160)
            .flat_map(|y| {
                (0..160).flat_map(move |x| {
                    if (x - 70_i32).pow(2) + (y - 70_i32).pow(2) < 45_i32.pow(2) {
                        [219, 117, 31, 255]
                    } else {
                        [255, 255, 255, 255]
                    }
                })
            })
            .collect();
        let (tx, rx) = std::sync::mpsc::channel();
        search(
            &[Engine::Yandex, Engine::GoogleLens],
            Snapshot {
                area: RECT {
                    right: 160,
                    bottom: 160,
                    ..Default::default()
                },
                data: Arc::from(data),
            },
            |engine, result| tx.send((engine.name(), result)).unwrap(),
        );
        drop(tx);
        let mut hosts = Vec::new();
        for (name, result) in rx {
            let url = result.unwrap_or_else(|error| panic!("{name}: {error}"));
            hosts.push(url.split('/').nth(2).unwrap().to_owned());
        }
        hosts.sort_unstable();
        assert_eq!(hosts, ["www.google.com", "yandex.com"]);
    }
}
