use std::{
    io::Read,
    path::{Path, PathBuf},
};

use bytes::Buf as _;
use tokio::runtime::Handle;
use typst::{
    Library,
    diag::FileResult,
    foundations::{Bytes, Datetime, Duration},
    syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot},
    text::{Font, FontBook},
    utils::LazyHash,
};
use typst_kit::{
    datetime::Time,
    files::{FileStore, FsRoot, SystemFiles},
    fonts::FontStore,
    packages::SystemPackages,
};

struct Downloader {
    client: reqwest::Client,
}

impl typst_kit::downloader::Downloader for Downloader {
    fn stream(
        &self,
        _: &dyn std::any::Any,
        url: &str,
    ) -> std::io::Result<(Option<usize>, Box<dyn std::io::Read>)> {
        Handle::current().block_on(async move {
            let resp = self
                .client
                .get(url)
                .send()
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

pub struct TemporaryWorld<'main, 'context, 'library> {
    pub main: &'main VirtualPath,
    pub context: &'context GlobalContext,
    pub library: &'library LazyHash<Library>,
}

pub struct GlobalContext {
    root: PathBuf,
    fonts: FontStore,
    files: FileStore<SystemFiles>,
}

impl GlobalContext {
    pub fn new(root: PathBuf) -> Self {
        let mut fonts = FontStore::new();
        fonts.extend(typst_kit::fonts::embedded());
        fonts.extend(typst_kit::fonts::system());

        let files = FileStore::new(SystemFiles::new(
            FsRoot::new(root.clone()),
            SystemPackages::new(Downloader {
                client: reqwest::Client::new(),
            }),
        ));

        Self { root, fonts, files }
    }

    pub fn directory(&self) -> &Path {
        &self.root
    }

    pub fn files_mut(&mut self) -> &mut FileStore<SystemFiles> {
        &mut self.files
    }
}

impl typst::World for TemporaryWorld<'_, '_, '_> {
    fn library(&self) -> &LazyHash<Library> {
        self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        self.context.fonts.book()
    }

    fn main(&self) -> FileId {
        FileId::new(RootedPath::new(VirtualRoot::Project, self.main.clone()))
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        self.context.files.source(id)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.context.files.file(id)
    }

    fn font(&self, id: usize) -> Option<Font> {
        self.context.fonts.font(id)
    }

    fn today(&self, offset: Option<Duration>) -> Option<Datetime> {
        Time::system().today(offset)
    }
}

impl typst_kit::diagnostics::DiagnosticWorld for TemporaryWorld<'_, '_, '_> {
    fn name(&self, id: FileId) -> String {
        id.vpath().get_without_slash().to_string()
    }
}
