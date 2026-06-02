use std::{io::Read, sync::Arc};

use bytes::Buf as _;
use tokio::runtime::Handle;
use typst::{
    Features, Library, LibraryExt,
    diag::{FileError, FileResult},
    foundations::{Bytes, Datetime, Duration},
    syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot},
    text::{Font, FontBook},
    utils::LazyHash,
};
use typst_kit::{files::FsRoot, fonts::FontStore, packages::SystemPackages};

struct Downloader;

impl typst_kit::downloader::Downloader for Downloader {
    fn stream(
        &self,
        _: &dyn std::any::Any,
        url: &str,
    ) -> std::io::Result<(Option<usize>, Box<dyn std::io::Read>)> {
        Handle::current().block_on(async move {
            let resp = reqwest::get(url)
                .await
                .map_err(|err| std::io::Error::other(err))?;
            let bytes = resp
                .bytes()
                .await
                .map_err(|err| std::io::Error::other(err))?;

            Ok((Some(bytes.len()), Box::new(bytes.reader()) as Box<dyn Read>))
        })
    }
}

pub struct TypstWorld {
    pub main: VirtualPath,
    pub context: Arc<TypstWorldContext>,
}

pub struct TypstWorldContext {
    root: FsRoot,
    library: LazyHash<Library>,
    fonts: FontStore,
    packages: SystemPackages,
}

impl TypstWorldContext {
    pub fn new(root: FsRoot) -> Self {
        let mut fonts = FontStore::new();
        fonts.extend(typst_kit::fonts::embedded());
        fonts.extend(typst_kit::fonts::system());

        Self {
            library: LazyHash::new(Library::builder().with_features(Features::all()).build()),
            fonts,
            root,
            packages: SystemPackages::new(Downloader),
        }
    }
}

impl typst::World for TypstWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.context.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        self.context.fonts.book()
    }

    fn main(&self) -> FileId {
        FileId::new(RootedPath::new(VirtualRoot::Project, self.main.clone()))
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        let text = self
            .file(id)?
            .into_string()
            .map_err(|_| FileError::InvalidUtf8)?;
        Ok(Source::new(id, text))
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        let root = match id.root() {
            VirtualRoot::Project => self.context.root.clone(),
            VirtualRoot::Package(spec) => self.context.packages.obtain(spec)?,
        };

        root.load(id.vpath())
    }

    fn font(&self, id: usize) -> Option<Font> {
        self.context.fonts.font(id)
    }

    fn today(&self, offset: Option<Duration>) -> Option<Datetime> {
        let offset = offset.unwrap_or(Duration::construct(0, 0, 0, 0, 0));
        let time = time::OffsetDateTime::now_local().ok()? + time::Duration::from(offset);
        Some(Datetime::Date(time.date()))
    }
}
