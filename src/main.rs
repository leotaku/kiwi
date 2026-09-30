mod errors;
mod typst_addons;
mod typst_wiki;
mod typst_world;

use std::{process::ExitCode, sync::Arc};

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
use tracing::{error, info, trace, warn};
use typst::{
    Features, Library, LibraryExt as _,
    foundations::{Dict, IntoValue as _, Repr as _},
    syntax::VirtualPath,
};
use typst_kit::diagnostics::{DiagnosticFormat, termcolor::StandardStream};

use crate::{
    errors::EmptyError, typst_addons::WikiScope, typst_wiki::Wiki, typst_world::AutoIncludeWorld,
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

    #[arg(long = "lsp-index")]
    #[arg(help = "Path to write LSP index file to")]
    index: Option<std::path::PathBuf>,
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt().init();

    let cmd = Command::parse();
    let result = match cmd {
        Command::Compile(args) => compile(args),
        Command::Watch(args) => watch(args).await,
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            if !err.is::<EmptyError>() {
                error!("{}", err)
            }
            ExitCode::FAILURE
        }
    }
}

fn compile(args: Compile) -> Result<(), Box<dyn std::error::Error>> {
    let world = make_world(args.directory);

    let mut writes_errored = false;
    let wiki = compile_to_memory(&world)?;
    for (path, contents) in wiki.entries {
        let path = path
            .realize(&args.output)
            .unwrap_or_else(|_| unreachable!());
        path.parent().and_then(|p| std::fs::create_dir_all(p).ok());
        if let Err(err) = std::fs::write(&path, contents) {
            error!(path = ?path, "{}", err);
            writes_errored = true;
        }
    }

    if writes_errored {
        Err(EmptyError.into())
    } else {
        Ok(())
    }
}

async fn watch(args: Watch) -> Result<(), Box<dyn std::error::Error>> {
    let mut world = make_world(args.directory);

    let pages = Arc::new(RwLock::new(None));

    let (send, mut recv) = tokio::sync::mpsc::channel(100);
    send.send(Event::new(EventKind::Other)).await.ok();
    let mut watcher = notify::recommended_watcher(move |event: Result<Event, _>| {
        if let Ok(evt) = event
            && !evt.kind.is_access()
        {
            send.blocking_send(evt).ok();
        }
    })?;
    watcher.watch(world.root().path(), notify::RecursiveMode::Recursive)?;

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
            *pages.write().await = compile_to_memory(&world).ok();
            if let Some(ref index_path) = args.index {
                generate_typst_index(pages.read().await.as_ref(), world.root().path(), index_path)
                    .unwrap_or_else(|err| {
                        warn!("could not write LSP index file: {}", err.to_string())
                    })
            }
            reloader.reload();
            info!("reload");

            let (loader, deps) = world.files_mut().dependencies();
            for file_id in deps {
                loader
                    .resolve(file_id)
                    .map(|path| watcher.watch(&path, notify::RecursiveMode::NonRecursive))
                    .ok();
            }
            world.files_mut().reset();

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

type AppState = State<(Arc<RwLock<Option<Wiki>>>, Option<ServeDir>)>;

async fn handler_with_servedir<T: Send + 'static>(
    State((pages, serve_dir)): AppState,
    req: Request<T>,
) -> Response {
    let guard = pages.read().await;
    match (serve_dir, handler(req.uri(), &guard).await) {
        (_, Ok(rsp)) => rsp.into_response(),
        (Some(serve_dir), Err(_)) => serve_dir.oneshot(req).await.into_response(),
        (_, Err(err)) => err.into_response(),
    }
}

type ErrResponse = (http::StatusCode, axum::response::Html<&'static str>);

async fn handler(uri: &axum::http::Uri, wiki: &Option<Wiki>) -> Result<Response, ErrResponse> {
    let pages = wiki.as_ref().map(|wiki| &wiki.entries).ok_or((
        http::StatusCode::INTERNAL_SERVER_ERROR,
        axum::response::Html("<code>compilation error</code>"),
    ))?;

    let path = VirtualPath::new(uri.path()).map_err(|_| {
        (
            http::StatusCode::BAD_REQUEST,
            axum::response::Html("<code>malformed uri</code>"),
        )
    })?;

    let (path, content) = pages
        .get_key_value(&path)
        .or_else(|| {
            path.join("index.html")
                .ok()
                .and_then(|path| pages.get_key_value(&path))
        })
        .ok_or((
            http::StatusCode::NOT_FOUND,
            axum::response::Html("<code>page not found</code>"),
        ))?;

    let content_type = mime_guess::from_path(path.get_with_slash()).first_or_octet_stream();
    let response = (
        [(http::header::CONTENT_TYPE, content_type.as_ref())],
        content.to_vec(),
    );

    Ok(response.into_response())
}

fn make_world(root_path: std::path::PathBuf) -> AutoIncludeWorld {
    let mut inputs = Dict::new();
    inputs.insert("x-wiki".into(), WikiScope.into_value());

    let mut library = Library::builder()
        .with_features(Features::all())
        .with_inputs(inputs)
        .build();
    library.styles.push(typst_addons::HIDE_ASSET_RECIPE.clone());

    AutoIncludeWorld::new(root_path, library.into())
}

fn compile_to_memory<W>(world: &W) -> Result<Wiki, EmptyError>
where
    W: typst::World + typst_kit::diagnostics::DiagnosticWorld,
{
    let mut stream = StandardStream::stderr(Default::default());
    let mut diagnostics = Vec::new();

    let warned = typst_wiki::compile(world);
    diagnostics.extend(warned.warnings);

    let result = match warned.output {
        Ok(wiki) => Ok(wiki),
        Err(errors) => {
            diagnostics.extend(errors);
            Err(EmptyError)
        }
    };

    typst_kit::diagnostics::emit(
        &mut stream,
        world,
        diagnostics.iter().filter(|diag| {
            diag.message != "html export is under active development and incomplete"
                && diag.message != "bundle export is experimental"
        }),
        DiagnosticFormat::Human,
    )
    .unwrap_or_else(|_| todo!());

    result
}

fn generate_typst_index(
    wiki: Option<&Wiki>,
    root_path: &std::path::Path,
    index_path: &std::path::Path,
) -> std::io::Result<()> {
    let wiki = match wiki {
        Some(wiki) => wiki,
        None => return Ok(()),
    };

    let index_path_parent = index_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new(".."));

    let paths = typst_addons::collect_index_paths(&*wiki.introspector)
        .into_iter()
        .filter_map(|path| path.realize(root_path).ok())
        .filter_map(|path| VirtualPath::virtualize(index_path_parent, &path).ok());

    let mut include_statements: Vec<_> = paths
        .into_iter()
        .map(|path| format!(r#"#include({})"#, path.get_with_slash().repr()))
        .collect();
    include_statements.insert(0, r#"#set heading(numbering: "1.")"#.to_owned());
    include_statements.push("".to_owned());

    std::fs::write(index_path, include_statements.join("\n"))?;

    Ok(())
}
