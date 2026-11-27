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
            "x",
            "youtube",
            "zhihu"
        }
    };
}

macro_rules! color_engine_icons {
    ($callback:ident) => {
        $callback! {
            "amazon", "baidu", "bilibili", "bing", "google", "reddit",
            "stackoverflow", "taobao", "youtube", "zhihu"
        }
    };
}

pub(crate) use color_engine_icons;
pub(crate) use engine_icons;
pub(crate) const CHEVRON_COLOR: u32 = 0x536273;

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

pub(crate) use ocr_icons;
