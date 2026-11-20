macro_rules! engine_icons {
    ($callback:ident) => {
        $callback! {
            "amazon",
            "baidu",
            "bilibili",
            "bing",
            "github",
            "google",
            "reddit",
            "stackoverflow",
            "taobao",
            "youtube",
            "zhihu"
        }
    };
}

pub(crate) use engine_icons;
pub(crate) const CHEVRON_COLOR: u32 = 0x536273;

#[cfg(feature = "ocr")]
macro_rules! ocr_icons {
    ($callback:ident) => {
        $callback! {
            Copy, "copy", 20;
            Search, "search", 20;
            Close, "x", 20;
            Check, "circle-check", 16;
            Scan, "scan", 16;
            Loader, "loader-2", 16;
        }
    };
}

#[cfg(feature = "ocr")]
pub(crate) use ocr_icons;
