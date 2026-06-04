mod typst_routines;
mod typst_world;

use std::sync::Arc;

use clap::Parser;
use typst::{Library, LibraryExt, syntax::VirtualPath};
use typst_kit::{
    diagnostics::{DiagnosticFormat, termcolor::StandardStream},
    files::FsRoot,
};
use typst_utils::LazyHash;
use typst_world::{GlobalContext, TemporaryWorld};
use walkdir::WalkDir;

use crate::typst_routines::Wiki;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(color=clap::ColorChoice::Never)]
struct Command {
    #[arg(short = 'a', long = "addr", default_value = "0.0.0.0")]
    #[arg(help = "Address to listen on", hide_default_value = true)]
    addr: std::net::IpAddr,

    #[arg(short = 'p', long = "port", default_value = "8080")]
    #[arg(help = "Port to listen on", hide_default_value = true)]
    port: u16,

    #[arg(help = "Path to serve as HTTP root")]
    directory: std::path::PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Command::parse();
    let mut stream = StandardStream::stderr(Default::default());

    let context = Arc::new(GlobalContext::new(FsRoot::new(args.directory.clone())));

    let paths = WalkDir::new(&args.directory)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.path().extension().map_or(false, |ext| ext == "typ")
                && entry.metadata().is_ok_and(|m| m.is_file())
        })
        .filter_map(|entry| VirtualPath::virtualize(&args.directory, entry.path()).ok());

    let wiki = typst_routines::render_wiki(Wiki::from_paths(paths), &context);
    let wiki = typst_routines::render_wiki(wiki, &context);

    for page in wiki.pages() {
        let out_path = page.path.with_extension("html").realize(&args.directory);
        match typst_html::html(&page.document) {
            Ok(text) => std::fs::write(out_path, text)?,
            Err(errs) => todo!(),
        }
    }

    let diagnostic_world = TemporaryWorld {
        main: &VirtualPath::new(".").expect("this to be a valid path"),
        context: &context,
        library: &LazyHash::new(Library::default()),
    };

    typst_kit::diagnostics::emit(
        &mut stream,
        &diagnostic_world,
        wiki.diagnostics().filter(|diag| {
            diag.message != "html export is under active development and incomplete"
        }),
        DiagnosticFormat::Human,
    )?;

    Ok(())
}
