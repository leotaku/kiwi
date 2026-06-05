use std::sync::Arc;

use rustc_hash::FxHashMap;
use typst::{
    Features, Library, LibraryExt as _, World,
    diag::{SourceDiagnostic, Warned},
    ecow::{EcoString, EcoVec, eco_format},
    engine::Engine,
    foundations::{Content, Dict, IntoValue, Label, Repr, Selector, Value},
    introspection::{Introspector as _, Location},
    syntax::VirtualPath,
    utils::{LazyHash, ManuallyHash, hash128},
};
use typst_macros::func;

use crate::typst_world::{GlobalContext, TemporaryWorld};

#[typst_macros::elem(scope)]
#[derive(Clone, Debug, PartialEq, Hash)]
struct ExtRefElem {
    #[required]
    element: Content,
    #[required]
    page: Arc<Page>,
}

typst_macros::cast! {
   ExtRefElem,
   v: Content => v.unpack::<Self>().map_err(|_| "expected an ext-ref element")?,
}

#[typst_macros::scope]
impl ExtRefElem {
    #[func]
    fn html_link(&self, engine: &Engine) -> EcoString {
        let relative_path = self.page.html_link(engine);
        self.element
            .location()
            .and_then(|loc| self.page.anchors.get(&loc))
            .map(|id| eco_format!("{}#{}", relative_path, id))
            .unwrap_or_else(|| relative_path)
    }
}

impl Repr for ExtRefElem {
    fn repr(&self) -> EcoString {
        "ext-ref".into()
    }
}

#[typst_macros::ty(scope)]
#[derive(Clone, Debug, PartialEq)]
struct Page {
    path: VirtualPath,
    anchors: FxHashMap<Location, EcoString>,
    document: ManuallyHash<typst_html::HtmlDocument>,
}

#[typst_macros::scope]
impl Page {
    #[func]
    fn html_link(&self, engine: &Engine) -> EcoString {
        let output_path = self.path.with_extension("html");
        engine.world.main().vpath().parent().map_or_else(
            || output_path.get_without_slash().into(),
            |parent| output_path.relative_from(&parent),
        )
    }
}

impl Repr for Page {
    fn repr(&self) -> EcoString {
        "page".into()
    }
}

impl std::hash::Hash for Page {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.path.hash(state);
        self.document.hash(state);
    }
}

#[derive(Clone, Debug, PartialEq, Hash)]
enum WikiEntry {
    Empty(VirtualPath),
    Rendered(Arc<Page>),
    Error(EcoVec<SourceDiagnostic>),
}

#[typst_macros::ty(scope)]
#[derive(Clone, Debug, PartialEq, Hash)]
pub struct Wiki(Vec<Warned<WikiEntry>>);

impl Wiki {
    pub fn from_paths(paths: impl IntoIterator<Item = VirtualPath>) -> Self {
        let entries = paths.into_iter().map(|path| Warned {
            output: WikiEntry::Empty(path),
            warnings: EcoVec::new(),
        });
        Self(entries.collect())
    }

    pub fn pages(&self) -> impl Iterator<Item = (&VirtualPath, &typst_html::HtmlDocument)> {
        self.0.iter().filter_map(|entry| match entry.output {
            WikiEntry::Rendered(ref page) => Some((&page.path, &*page.document)),
            _ => None,
        })
    }

    pub fn diagnostics(&self) -> impl Iterator<Item = &SourceDiagnostic> {
        fn errors(entry: &WikiEntry) -> impl Iterator<Item = &SourceDiagnostic> {
            match entry {
                WikiEntry::Error(errors) => Some(errors.iter()).into_iter(),
                _ => None.into_iter(),
            }
            .flatten()
        }

        self.0
            .iter()
            .flat_map(|entry| entry.warnings.iter().chain(errors(&entry.output)))
    }

    fn is_incomplete(&self) -> bool {
        self.0
            .iter()
            .any(|entry| matches!(entry.output, WikiEntry::Empty(_)))
    }
}

#[typst_macros::scope]
impl Wiki {
    #[func]
    fn query(&self, selector: Selector) -> Vec<ExtRefElem> {
        let mut results = Vec::new();
        for entry in self.0.iter() {
            if let WikiEntry::Rendered(ref page) = entry.output {
                results.extend(
                    page.document
                        .introspector()
                        .query(&selector)
                        .into_iter()
                        .map(|element| ExtRefElem {
                            element,
                            page: page.clone(),
                        }),
                )
            }
        }
        results
    }

    #[func]
    fn query_label(&self, engine: &Engine, label: Label) -> Result<Value, EcoString> {
        let wiki = get_wiki(engine);
        let mut queried = wiki.query(Selector::Label(label));
        match queried.pop() {
            None => {
                if wiki.is_incomplete() {
                    Ok(Value::None)
                } else {
                    Err(eco_format!(
                        "label `<{}>` does not exist in the wiki",
                        label.resolve()
                    ))
                }
            }
            Some(_) if !queried.is_empty() => Err(eco_format!(
                "label `<{}>` occurs multiple times in the wiki",
                label.resolve()
            )),
            Some(ext_ref) => Ok(ext_ref.into_value()),
        }
    }
}

impl Repr for Wiki {
    fn repr(&self) -> EcoString {
        "wiki".into()
    }
}

fn get_wiki<'a>(engine: &'a Engine) -> &'a Wiki {
    let sys = engine.library.global.scope().get("sys").unwrap().read();
    let Value::Module(sys) = sys else {
        unreachable!()
    };
    let Value::Dict(dict) = sys.scope().get("inputs").unwrap().read() else {
        unreachable!()
    };
    let Value::Dyn(wiki) = dict.get("x-wiki").unwrap() else {
        unreachable!()
    };

    wiki.downcast().unwrap()
}

pub fn render_wiki(wiki: Wiki, context: &GlobalContext) -> Wiki {
    let mut entries = Vec::new();
    let mut paths = Vec::new();
    for entry in wiki.0.iter() {
        match entry.output {
            WikiEntry::Empty(ref path) => paths.push(path.clone()),
            WikiEntry::Rendered(ref page) => paths.push(page.path.clone()),
            _ => entries.push(entry.clone()),
        }
    }

    let mut inputs = Dict::new();
    inputs.insert("x-wiki".into(), wiki.into_value());

    let mut library = Library::builder()
        .with_features(Features::all())
        .with_inputs(inputs)
        .build();
    let global = library.global.scope_mut();
    global.define_elem::<ExtRefElem>();
    let library = LazyHash::new(library);

    for path in paths {
        let world = TemporaryWorld {
            main: &path,
            library: &library,
            context,
        };

        match typst::compile::<typst_html::HtmlDocument>(&world) {
            Warned {
                output: Ok(mut document),
                warnings,
            } => {
                let targets = document
                    .introspector()
                    .query_labelled()
                    .into_iter()
                    .filter_map(|content| content.location())
                    .collect();
                let anchors = typst_html::create_link_anchors(&mut document, &targets);

                let hash = hash128(document.root());
                entries.push(Warned {
                    output: WikiEntry::Rendered(Arc::new(Page {
                        path,
                        anchors,
                        document: ManuallyHash::new(document, hash),
                    })),
                    warnings,
                });
            }
            Warned {
                output: Err(errors),
                warnings,
            } => {
                entries.push(Warned {
                    output: WikiEntry::Error(errors),
                    warnings,
                });
            }
        };
    }

    Wiki(entries)
}
