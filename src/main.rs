mod typst_routines;
mod typst_world;

use std::sync::Arc;

use axum::Router;
use clap::Parser;
use notify::{Event, EventKind, Watcher as _};
use tower_http::services::ServeDir;
use tower_livereload::LiveReloadLayer;
use tracing::{info, trace};
use typst::{Library, LibraryExt as _, syntax::VirtualPath, utils::LazyHash};
use typst_kit::diagnostics::{DiagnosticFormat, termcolor::StandardStream};
use walkdir::WalkDir;

use crate::{
    typst_routines::Wiki,
    typst_world::{GlobalContext, TemporaryWorld},
};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(color=clap::ColorChoice::Never)]
enum Command {
    Compile(#[command(flatten)] Compile),
    Watch(#[command(flatten)] Watch),
}

#[derive(Parser)]
#[command(about="Compile a wiki directory to static HTML pages", long_about = None)]
struct Compile {
    #[arg(help = "Root path of the wiki")]
    directory: std::path::PathBuf,
}

#[derive(Parser)]
#[command(about="Continuously watch and recompile a wiki directory", long_about = None)]
struct Watch {
    #[arg(short = 'a', long = "addr", default_value = "0.0.0.0")]
    #[arg(help = "Address to listen on", hide_default_value = true)]
    addr: std::net::IpAddr,

    #[arg(short = 'p', long = "port", default_value = "8080")]
    #[arg(help = "Port to listen on", hide_default_value = true)]
    port: u16,

    #[arg(help = "Root path of the wiki")]
    directory: std::path::PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let cmd = Command::parse();
    match cmd {
        Command::Compile(args) => {
            let context = GlobalContext::new(args.directory);
            compile(Arc::new(context))
        }
        Command::Watch(args) => watch(args).await,
    }
}

async fn watch(args: Watch) -> Result<(), Box<dyn std::error::Error>> {
    let mut context = GlobalContext::new(args.directory);

    let livereload = LiveReloadLayer::new();
    let reloader = livereload.reloader();
    let app = Router::new()
        .fallback_service(ServeDir::new(context.directory()))
        .layer(livereload);

    let (send, mut recv) = tokio::sync::mpsc::channel(100);
    send.send(Event::new(EventKind::Other)).await.ok();
    let mut watcher = notify::recommended_watcher(move |event: Result<Event, _>| {
        if let Ok(evt) = event
            && !evt.kind.is_access()
        {
            send.blocking_send(evt).ok();
        }
    })?;

    let addr: std::net::SocketAddr = (args.addr, args.port).into();
    info!("listening on: http://{}/", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tokio::spawn(async move {
        while let Some(event) = recv.recv().await {
            trace!(changed_files = ?event.paths, "recompiling");
            let context_arc = Arc::new(context);
            compile(context_arc.clone()).ok();
            reloader.reload();
            info!("reload");
            context = Arc::try_unwrap(context_arc).unwrap_or_else(|_| unreachable!());

            let (loader, deps) = context.files_mut().dependencies();
            for file_id in deps {
                loader
                    .resolve(file_id)
                    .map(|path| watcher.watch(&path, notify::RecursiveMode::NonRecursive))
                    .ok();
            }
            context.files_mut().reset();

            while let Ok(_) = recv.try_recv() {}
        }
    });

    axum::serve(listener, app).await?;

    Ok(())
}

fn compile(context: Arc<GlobalContext>) -> Result<(), Box<dyn std::error::Error>> {
    let mut stream = StandardStream::stderr(Default::default());

    let paths = WalkDir::new(context.directory())
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.path().extension().is_some_and(|ext| ext == "typ")
                && entry.metadata().is_ok_and(|m| m.is_file())
        })
        .filter_map(|entry| VirtualPath::virtualize(context.directory(), entry.path()).ok());

    let mut wiki = typst_routines::render_wiki(Wiki::from_paths(paths), &context);
    for _ in 0..1 {
        if wiki
            .diagnostics()
            .any(|diag| diag.severity == typst::diag::Severity::Error)
        {
            break;
        }
        wiki = typst_routines::render_wiki(wiki, &context);
    }

    let mut diagnostics: Vec<_> = wiki.diagnostics().cloned().collect();

    for (path, document) in wiki.pages() {
        let out_path = path.with_extension("html").realize(context.directory());
        match typst_html::html(document) {
            Ok(text) => std::fs::write(out_path, text)?,
            Err(errs) => diagnostics.extend(errs),
        }
    }

    let diagnostic_world = TemporaryWorld {
        main: &VirtualPath::new(".").unwrap_or_else(|_| unreachable!()),
        context: &context,
        library: &LazyHash::new(Library::default()),
    };

    typst_kit::diagnostics::emit(
        &mut stream,
        &diagnostic_world,
        diagnostics.iter().filter(|diag| {
            diag.message != "html export is under active development and incomplete"
        }),
        DiagnosticFormat::Human,
    )?;

    Ok(())
}
