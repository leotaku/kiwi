use std::{
    path::{Path, PathBuf},
    sync::LazyLock,
};

use bytes::Buf as _;
use tokio::runtime::Handle;
use typst::{
    Library,
    diag::FileResult,
    foundations::{Bytes, Datetime, Duration, Repr as _},
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
use walkdir::WalkDir;

static VIRTUAL_MAIN: LazyLock<FileId> = LazyLock::new(|| {
    FileId::unique(RootedPath::new(
        VirtualRoot::Project,
        VirtualPath::new("virtual").unwrap_or_else(|_| unreachable!()),
    ))
});

struct Downloader {
    client: reqwest::Client,
}

impl typst_kit::downloader::Downloader for Downloader {
    fn stream(
        &self,
        _: &dyn std::any::Any,
        url: &str,
    ) -> std::io::Result<(Option<usize>, Box<dyn std::io::Read>)> {
        let bytes = tokio::task::block_in_place(move || {
            Handle::current().block_on(async move {
                let resp = self
                    .client
                    .get(url)
                    .send()
                    .await
                    .map_err(std::io::Error::other)?;
                resp.bytes().await.map_err(std::io::Error::other)
            })
        })?;

        Ok((Some(bytes.len()), Box::new(bytes.reader())))
    }
}

pub struct AutoIncludeWorld {
    library: LazyHash<Library>,
    fonts: FontStore,
    files: FileStore<SystemFiles>,
    virtual_main_content: String,
}

impl AutoIncludeWorld {
    pub fn new(root_path: PathBuf, library: LazyHash<Library>) -> Self {
        let mut include_statements: Vec<_> = find_typst_paths(&root_path)
            .map(|path| format!(r#"#include({})"#, path.get_with_slash().repr()))
            .collect();
        include_statements.push("".to_owned());
        let virtual_main_content = include_statements.join("\n");

        let mut fonts = FontStore::new();
        fonts.extend(typst_kit::fonts::embedded());
        fonts.extend(typst_kit::fonts::system());

        let files = FileStore::new(SystemFiles::new(
            FsRoot::new(root_path),
            SystemPackages::new(Downloader {
                client: reqwest::Client::new(),
            }),
        ));

        Self {
            library,
            fonts,
            files,
            virtual_main_content,
        }
    }

    pub fn files_mut(&mut self) -> &mut FileStore<SystemFiles> {
        &mut self.files
    }

    pub fn root(&self) -> FsRoot {
        self.files
            .loader()
            .root(*VIRTUAL_MAIN)
            .unwrap_or_else(|_| unreachable!())
    }
}

impl typst::World for AutoIncludeWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        self.fonts.book()
    }

    fn main(&self) -> FileId {
        *VIRTUAL_MAIN
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == *VIRTUAL_MAIN {
            return Ok(Source::new(id, self.virtual_main_content.clone()));
        }
        self.files.source(id)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if id == *VIRTUAL_MAIN {
            return Ok(Bytes::from_string(self.virtual_main_content.clone()));
        }
        self.files.file(id)
    }

    fn font(&self, id: usize) -> Option<Font> {
        self.fonts.font(id)
    }

    fn today(&self, offset: Option<Duration>) -> Option<Datetime> {
        Time::system().today(offset)
    }
}

impl typst_kit::diagnostics::DiagnosticWorld for AutoIncludeWorld {
    fn name(&self, id: FileId) -> String {
        id.vpath().get_without_slash().to_string()
    }
}

fn find_typst_paths(root_path: &Path) -> impl Iterator<Item = VirtualPath> {
    WalkDir::new(root_path)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.path().extension().is_some_and(|ext| ext == "typ")
                && entry.metadata().is_ok_and(|m| m.is_file())
                && !path_is_hidden(entry.path())
        })
        .filter_map(|entry| VirtualPath::virtualize(root_path, entry.path()).ok())
}

fn path_is_hidden(path: &std::path::Path) -> bool {
    path.iter()
        .any(|segment| segment.as_encoded_bytes().starts_with(".".as_ref()))
}
