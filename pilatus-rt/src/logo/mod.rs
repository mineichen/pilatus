use std::{collections::HashMap, io::Read, path::Path, sync::Arc};

use minfac::{Registered, ServiceCollection};
use pilatus::{
    EncodedImage, FallbackLogo, GenericConfig, LogoQuery, LogoService, LogoServiceTrait, Name,
};

pub(super) fn register_services(c: &mut ServiceCollection) {
    c.register(|| {
        const BRIGHT: &[u8] = include_bytes!("./pilatus_bright.svg");
        FallbackLogo::with_themes(
            BRIGHT,
            &[
                ("dark", include_bytes!("./pilatus_dark.svg")),
                ("bright", BRIGHT),
            ],
        )
    });
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

impl LogoServiceTrait for LogoServiceImpl {
    fn get(&self, query: &LogoQuery) -> EncodedImage {
        if let Some(x) = query.theme.as_ref().and_then(|x| self.themes.get(x)) {
            x.clone()
        } else {
            self.main.clone()
        }
    }
}
