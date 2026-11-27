use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DEFAULT_ENGINE: &str = "bing";
const SPACE: [char; 4] = [' ', '\t', '\r', '\n'];

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "format", content = "data")]
pub enum Icon {
    #[default]
    Default,
    Svg(String),
    Bgra32(Vec<u32>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Engine {
    pub id: String,
    pub name: String,
    pub alias: String,
    pub url: String,
    pub color: u32,
    #[serde(default)]
    pub icon: Icon,
    #[serde(default, skip_serializing_if = "Icon::is_default")]
    pub mono_icon: Icon,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mono_color: Option<u32>,
}

impl Icon {
    pub fn is_default(&self) -> bool {
        matches!(self, Self::Default)
    }
}

pub fn built_in_engines() -> Vec<Engine> {
    [
        ("bing", "必应", "bing", "https://www.bing.com/search?q=%s"),
        ("baidu", "百度", "bd", "https://www.baidu.com/s?wd=%s"),
        (
            "google",
            "Google",
            "g",
            "https://www.google.com/search?q=%s",
        ),
        (
            "bilibili",
            "哔哩哔哩",
            "bili",
            "https://search.bilibili.com/all?keyword=%s",
        ),
        (
            "youtube",
            "YouTube",
            "yt",
            "https://www.youtube.com/results?search_query=%s",
        ),
        (
            "zhihu",
            "知乎",
            "zh",
            "https://www.zhihu.com/search?type=content&q=%s",
        ),
        (
            "reddit",
            "Reddit",
            "rd",
            "https://www.reddit.com/search/?q=%s",
        ),
        ("github", "GitHub", "gh", "https://github.com/search?q=%s"),
        (
            "stackoverflow",
            "Stack Overflow",
            "so",
            "https://stackoverflow.com/search?q=%s",
        ),
        ("taobao", "淘宝", "tb", "https://s.taobao.com/search?q=%s"),
        ("amazon", "Amazon", "amz", "https://www.amazon.com/s?k=%s"),
        ("x", "X", "x", "https://x.com/search?q=%s"),
    ]
    .into_iter()
    .map(|(id, name, alias, url)| Engine {
        id: id.into(),
        name: name.into(),
        alias: alias.into(),
        url: url.into(),
        color: 0x005fb8,
        icon: Icon::Default,
        mono_icon: Icon::Default,
        mono_color: None,
    })
    .collect()
}

pub fn trim(text: &str) -> &str {
    text.trim_matches(SPACE)
}

pub fn is_web_url(url: &str) -> bool {
    let url = trim(url);
    url.encode_utf16().count() <= 2048 && is_browser_url(url)
}

pub(crate) fn is_browser_url(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        && !authority.is_empty()
        && !authority.starts_with(':')
        && !authority.contains(['@', '%'])
        && !rest
            .chars()
            .any(|ch| ch <= ' ' || matches!(ch, '\u{7f}' | '\\' | '"'))
}

pub fn is_search_template(url: &str) -> bool {
    is_web_url(url)
        && url.split_once("://").is_some_and(|(_, rest)| {
            rest.find(['/', '?', '#'])
                .is_some_and(|path| rest[path..].contains("%s"))
        })
}

pub fn search_url(template: &str, query: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(query.len());
    for byte in query.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[(byte >> 4) as usize]));
            encoded.push(char::from(HEX[(byte & 15) as usize]));
        }
    }
    template.replace("%s", &encoded)
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EngineError {
    #[error("invalid engine name")]
    Name,
    #[error("invalid search template")]
    Url,
    #[error("invalid engine alias")]
    Alias,
    #[error("invalid engine icon")]
    Icon,
    #[error("duplicate engine alias")]
    DuplicateAlias,
    #[error("invalid or duplicate engine ID")]
    Id,
    #[error("the engine operation is unavailable")]
    Unavailable,
}

impl Engine {
    pub fn is_custom(&self) -> bool {
        self.id.starts_with("custom-")
    }

    pub fn icon_for(&self, multicolor: bool) -> &Icon {
        let (preferred, fallback) = if multicolor {
            (&self.icon, &self.mono_icon)
        } else {
            (&self.mono_icon, &self.icon)
        };
        if preferred.is_default() {
            fallback
        } else {
            preferred
        }
    }

    pub fn mono_tint(&self, multicolor: bool) -> Option<u32> {
        if multicolor { None } else { self.mono_color }
    }

    pub fn normalize(&mut self) -> Result<(), EngineError> {
        self.name = trim(&self.name).into();
        self.url = trim(&self.url).into();
        if self.name.is_empty()
            || self.name.encode_utf16().count() > 48
            || self.name.chars().any(|ch| ch < ' ' || ch == '\u{7f}')
        {
            return Err(EngineError::Name);
        }
        if !is_search_template(&self.url) {
            return Err(EngineError::Url);
        }
        self.alias = normalize_alias(&self.alias)?;
        if self.color > 0x00ff_ffff
            || self.mono_color.is_some_and(|color| color > 0x00ff_ffff)
            || [&self.icon, &self.mono_icon]
                .into_iter()
                .any(|icon| match icon {
                    Icon::Default => false,
                    Icon::Svg(svg) => svg.is_empty() || svg.len() > 64 * 1024,
                    Icon::Bgra32(pixels) => {
                        pixels.len() != 32 * 32
                            || pixels.iter().any(|&p| {
                                let [b, g, r, a] = p.to_le_bytes();
                                [b, g, r].into_iter().any(|channel| channel > a)
                            })
                    }
                })
        {
            return Err(EngineError::Icon);
        }
        Ok(())
    }
}

pub(crate) fn normalize_alias(alias: &str) -> Result<String, EngineError> {
    let alias = trim(alias);
    let alias = alias
        .strip_prefix('#')
        .unwrap_or(alias)
        .to_ascii_lowercase();
    if !alias.is_empty()
        && (alias.len() > 16
            || !alias.as_bytes()[0].is_ascii_lowercase()
            || !alias
                .bytes()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || b"-_".contains(&ch)))
    {
        return Err(EngineError::Alias);
    }
    Ok(alias)
}

pub fn alias_prefix(input: &str) -> Option<(&str, &str)> {
    let start = trim(input).strip_prefix('#')?;
    let separator = start.find(SPACE).unwrap_or(start.len());
    Some((&start[..separator], trim(&start[separator..])))
}

pub fn suggestions<'a>(engines: &'a [Engine], pins: &[String], input: &str) -> Vec<&'a Engine> {
    let Some(prefix) = input.trim_start_matches(SPACE).strip_prefix('#') else {
        return Vec::new();
    };
    if prefix.contains(SPACE) {
        return Vec::new();
    }
    let prefix = prefix.to_ascii_lowercase();
    let mut matches: Vec<_> = engines
        .iter()
        .filter(|engine| {
            engine.alias.starts_with(&prefix) || engine.name.to_ascii_lowercase().contains(&prefix)
        })
        .collect();
    matches.sort_by_key(|engine| {
        if !prefix.is_empty() && engine.alias == prefix {
            0
        } else if pins.contains(&engine.id) {
            1
        } else {
            2
        }
    });
    matches
}
