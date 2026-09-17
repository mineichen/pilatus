use std::{collections::HashMap, io::Read, path::Path, sync::Arc};

use minfac::{Registered, ServiceCollection};
use pilatus::{
    EncodedImage, FallbackLogo, GenericConfig, LogoQuery, LogoService, LogoServiceTrait, Name,
};

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.register(create_fallback_logo);
    c.with::<(Registered<FallbackLogo>, Registered<GenericConfig>)>()
        .register_shared(|(fallback, generic)| {
            let (main, themes) =
                read_logo_from_path(&generic.root).unwrap_or_else(|| fallback.into());
            Arc::new(LogoServiceImpl::new(main, themes))
        })
        .alias(|s| LogoService::new(s));
}

fn read_logo_from_path(path: &Path) -> Option<(EncodedImage, HashMap<Name, EncodedImage>)> {
    let dir = std::fs::read_dir(path).ok()?;
    let path = dir
        .filter_map(|f| {
            let entry = f.ok()?;
            let filename = entry.file_name();
            let filename_str = filename.to_str()?;
            filename_str.starts_with("logo").then_some(entry.path())
        })
        .next()?;
    let mut buf = Vec::new();
    std::fs::File::open(path).ok()?.read_to_end(&mut buf).ok()?;

    Some((EncodedImage(Arc::from(buf)), Default::default()))
}

struct LogoServiceImpl {
    main: EncodedImage,
    themes: HashMap<Name, EncodedImage>,
}

impl LogoServiceImpl {
    fn new(fallback: EncodedImage, themes: HashMap<Name, EncodedImage>) -> Self {
        Self {
            main: fallback,
            themes,
        }
    }
}

/// Light (default) logo: black contours, for bright backgrounds.
const LIGHT: &[u8] = include_bytes!("pilatus_logo.svg");
const LIGHT_SVG_LEN: usize = LIGHT.len();
/// Dark logo: white contours, derived from [`LIGHT_SVG`] at compile time.
const DARK_SVG_BYTES: [u8; LIGHT_SVG_LEN] = recolor_black_to_white();
const DARK: &[u8] = &DARK_SVG_BYTES;

/// Replaces every `#000000` with `#ffffff` (same length, so in-place).
const fn recolor_black_to_white() -> [u8; LIGHT_SVG_LEN] {
    let src = LIGHT;
    let mut out = [0u8; LIGHT_SVG_LEN];
    let mut i = 0;
    while i < LIGHT_SVG_LEN {
        out[i] = src[i];
        i += 1;
    }
    let mut j = 0;
    while j + 7 <= LIGHT_SVG_LEN {
        if out[j] == b'#'
            && out[j + 1] == b'0'
            && out[j + 2] == b'0'
            && out[j + 3] == b'0'
            && out[j + 4] == b'0'
            && out[j + 5] == b'0'
            && out[j + 6] == b'0'
        {
            out[j + 1] = b'f';
            out[j + 2] = b'f';
            out[j + 3] = b'f';
            out[j + 4] = b'f';
            out[j + 5] = b'f';
            out[j + 6] = b'f';
            j += 7;
        } else {
            j += 1;
        }
    }
    out
}

fn create_fallback_logo() -> FallbackLogo {
    FallbackLogo::with_themes(LIGHT, &[("dark", DARK), ("light", LIGHT)])
}

#[cfg(feature = "unstable")]
pub fn create_default_logo_service() -> LogoService {
    let (main, themes) = create_fallback_logo().into();
    LogoService::new(Arc::new(LogoServiceImpl { themes, main }))
}

impl LogoServiceTrait for LogoServiceImpl {
    fn get(&self, query: &LogoQuery) -> EncodedImage {
        if let Some(x) = query.theme.as_ref().and_then(|x| self.themes.get(x)) {
            x.clone()
        } else {
            self.main.clone()
        }
    }
}
