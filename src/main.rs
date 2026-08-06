mod typst_routines;
mod typst_world;

use std::{collections::HashMap, sync::Arc};

use axum::{
    Router,
    extract::{Request, State},
    http,
    middleware::map_response,
    response::{IntoResponse as _, Response},
    routing::get,
};
use clap::Parser;
use notify::{Event, EventKind, Watcher as _};
use tokio::sync::RwLock;
use tower::ServiceExt as _;
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

    #[arg(short = 'o', long = "out", help = "Path to write files to")]
    output: std::path::PathBuf,
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

    #[arg(short = 's', long = "static")]
    #[arg(help = "Path to serve additional static files from")]
    r#static: Option<std::path::PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let cmd = Command::parse();
    match cmd {
        Command::Compile(args) => compile(args),
        Command::Watch(args) => watch(args).await,
    }
}

fn compile(args: Compile) -> Result<(), Box<dyn std::error::Error>> {
    let context = GlobalContext::new(args.directory);

    let mut pages = compile_to_memory(Arc::new(context));
    for (path, contents) in pages.drain() {
        let path = path.realize(&args.output)?;
        path.parent().and_then(|p| std::fs::create_dir_all(p).ok());
        std::fs::write(path, contents)?
    }

    Ok(())
}

async fn watch(args: Watch) -> Result<(), Box<dyn std::error::Error>> {
    let mut context = GlobalContext::new(args.directory);

    let pages = Arc::new(RwLock::new(HashMap::<VirtualPath, Vec<u8>>::new()));

    let (send, mut recv) = tokio::sync::mpsc::channel(100);
    send.send(Event::new(EventKind::Other)).await.ok();
    let mut watcher = notify::recommended_watcher(move |event: Result<Event, _>| {
        if let Ok(evt) = event
            && !evt.kind.is_access()
        {
            send.blocking_send(evt).ok();
        }
    })?;
    watcher.watch(context.directory(), notify::RecursiveMode::Recursive)?;

    let livereload = LiveReloadLayer::new();
    let reloader = livereload.reloader();
    let app = Router::new()
        .fallback(get(handler_with_servedir))
        .with_state((pages.clone(), args.r#static.map(ServeDir::new)))
        .layer(livereload)
        .layer(map_response(add_cache_headers));

    let addr: std::net::SocketAddr = (args.addr, args.port).into();
    info!("listening on: http://{}/", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tokio::spawn(async move {
        while let Some(event) = recv.recv().await {
            trace!(changed_files = ?event.paths, "recompiling");
            let context_arc = Arc::new(context);
            *pages.write().await = compile_to_memory(context_arc.clone());
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

            while recv.try_recv().is_ok() {}
        }
    });

    axum::serve(listener, app).await?;

    Ok(())
}

async fn add_cache_headers(mut rsp: Response) -> Response {
    let headers = rsp.headers_mut();
    headers.insert(
        http::header::CACHE_CONTROL,
        http::HeaderValue::from_static("no-cache, no-store, must-revalidate"),
    );
    headers.insert(
        http::header::PRAGMA,
        http::HeaderValue::from_static("no-cache"),
    );
    headers.insert(http::header::EXPIRES, http::HeaderValue::from_static("0"));
    rsp
}

type AppState = State<(Arc<RwLock<HashMap<VirtualPath, Vec<u8>>>>, Option<ServeDir>)>;

async fn handler_with_servedir<T: Send + 'static>(
    State((pages, serve_dir)): AppState,
    req: Request<T>,
) -> Response {
    let guard = pages.read().await;
    match (serve_dir, handler(req.uri(), &guard).await) {
        (_, Ok(rsp)) => rsp.into_response(),
        (Some(serve_dir), Err(_)) => serve_dir.oneshot(req).await.into_response(),
        (_, Err(err)) => err,
    }
}

#[expect(clippy::result_large_err)]
async fn handler(
    uri: &axum::http::Uri,
    pages: &HashMap<VirtualPath, Vec<u8>>,
) -> Result<Response, Response> {
    let path = VirtualPath::new(uri.path()).map_err(|_| {
        (
            http::StatusCode::INTERNAL_SERVER_ERROR,
            axum::response::Html("<code>weird path error</code>"),
        )
            .into_response()
    })?;

    let (path, content) = pages
        .get(&path)
        .map(|content| (path.clone(), content))
        .or_else(|| {
            path.join("index.html")
                .ok()
                .and_then(|path| pages.get(&path).map(|content| (path, content)))
        })
        .ok_or_else(|| {
            (
                http::StatusCode::NOT_FOUND,
                axum::response::Html("<code>page not found</code>"),
            )
                .into_response()
        })?;

    let content_type = mime_guess::from_path(path.get_with_slash()).first_or_octet_stream();
    let response = (
        [(http::header::CONTENT_TYPE, content_type.as_ref())],
        content.clone(),
    );

    Ok(response.into_response())
}

fn compile_to_memory(context: Arc<GlobalContext>) -> HashMap<VirtualPath, Vec<u8>> {
    let mut stream = StandardStream::stderr(Default::default());

    let paths = WalkDir::new(context.directory())
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.path().extension().is_some_and(|ext| ext == "typ")
                && entry.metadata().is_ok_and(|m| m.is_file())
                && !path_is_hidden(entry.path())
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

    let mut pages = HashMap::new();
    let mut diagnostics: Vec<_> = wiki.diagnostics().cloned().collect();

    for (path, document) in wiki.pages() {
        let out_path = path.with_extension("html");
        match typst_html::html(document, &Default::default()) {
            Ok(text) => {
                pages.insert(out_path, text.into_bytes());
            }
            Err(errs) => diagnostics.extend(errs),
        }
    }

    match typst_routines::collect_assets(&wiki) {
        Ok(resources) => {
            for (path, content) in resources {
                pages.insert(path, content.into_vec());
            }
        }
        Err(errs) => diagnostics.extend(errs),
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
    )
    .unwrap_or_else(|_| todo!());

    pages
}

fn path_is_hidden(path: &std::path::Path) -> bool {
    path.iter()
        .any(|segment| segment.as_encoded_bytes().starts_with(".".as_ref()))
}
