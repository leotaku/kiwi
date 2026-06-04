use std::sync::Arc;

use rustc_hash::{FxBuildHasher, FxHashMap};
use typst::{
    Features, Library, LibraryExt as _, World,
    diag::{SourceDiagnostic, Warned},
    ecow::{EcoString, EcoVec, eco_format, eco_vec},
    engine::Engine,
    foundations::{
        Arg, Args, Array, Content, Dict, Dynamic, Element, Fold, IntoValue, Label, NativeElement,
        NativeFunc as _, Packed, Recipe, Repr, Selector, Str, Style, Transformation, Value,
    },
    introspection::{Introspector as _, Location, MetadataElem, QueryIntrospection},
    model::RefElem,
    syntax::{RootedPath, Span, Spanned, VirtualPath},
    text::TextElem,
    utils::{LazyHash, ManuallyHash, hash128},
};
use typst_html::{HtmlAttr, HtmlElem, HtmlTag};
use typst_macros::func;

use crate::typst_world::{GlobalContext, TemporaryWorld};

#[typst_macros::elem(scope)]
#[derive(Clone, Debug, PartialEq, Hash)]
pub struct ExtRefElem {
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
    fn html_link(&self, engine: &Engine) -> Str {
        let output_path = self.page.path.with_extension("html");

        let relative_path = engine.world.main().vpath().parent().map_or_else(
            || output_path.get_without_slash().into(),
            |parent| output_path.relative_from(&parent),
        );

        let target_link = self
            .element
            .location()
            .and_then(|loc| self.page.anchors.get(&loc))
            .map(|id| eco_format!("{}#{}", relative_path, id))
            .unwrap_or_else(|| relative_path);

        target_link.into()
    }
}

impl Repr for ExtRefElem {
    fn repr(&self) -> typst::ecow::EcoString {
        "ext-ref".into()
    }
}

#[typst_macros::ty]
#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub path: VirtualPath,
    pub anchors: FxHashMap<Location, EcoString>,
    pub document: ManuallyHash<typst_html::HtmlDocument>,
}

impl Repr for Page {
    fn repr(&self) -> typst::ecow::EcoString {
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

#[typst_macros::ty]
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

    pub fn pages<'a>(&'a self) -> impl Iterator<Item = &'a Page> {
        self.0.iter().filter_map(|entry| match entry.output {
            WikiEntry::Rendered(ref page) => Some(page.as_ref()),
            _ => None,
        })
    }

    pub fn diagnostics<'a>(&'a self) -> impl Iterator<Item = &'a SourceDiagnostic> {
        fn errors<'a>(entry: &'a WikiEntry) -> impl Iterator<Item = &'a SourceDiagnostic> {
            match entry {
                WikiEntry::Error(errors) => Some(errors.iter()).into_iter(),
                _ => None.into_iter(),
            }
            .flatten()
        }

        self.0
            .iter()
            .filter_map(|entry| Some(entry.warnings.iter().chain(errors(&entry.output))))
            .flatten()
    }

    fn is_incomplete(&self) -> bool {
        self.0.iter().fold(false, |agg, entry| {
            agg || matches!(entry.output, WikiEntry::Empty(_))
        })
    }

    fn query(&self, selector: &Selector) -> EcoVec<ExtRefElem> {
        let mut results = EcoVec::new();
        for entry in self.0.iter() {
            if let WikiEntry::Rendered(ref page) = entry.output {
                results.extend(
                    page.document
                        .introspector()
                        .query(selector)
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
}

impl Repr for Wiki {
    fn repr(&self) -> typst::ecow::EcoString {
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

#[typst_macros::func]
fn resolve_refs_externally(
    engine: &mut Engine,
    body: Content,
) -> Result<Value, EcoVec<SourceDiagnostic>> {
    let packed = match Packed::<RefElem>::from_owned(body) {
        Ok(packed) => packed,
        Err(content) => return Ok(content.into_value()),
    };
    let intra_doc_labeled = engine.introspect(QueryIntrospection(
        Selector::Label(packed.target),
        Span::detached(),
    ));
    if intra_doc_labeled.len() > 0 {
        return Ok(packed.pack().into_value());
    }

    let wiki = get_wiki(engine);
    let mut queried = wiki.query(&Selector::Label(packed.target));
    match queried.pop() {
        None => {
            if wiki.is_incomplete() {
                engine.sink.warn(SourceDiagnostic::error(
                    packed.span(),
                    "unresolved wiki links",
                ));
                Ok(Value::None)
            } else {
                Err(eco_vec![SourceDiagnostic::error(
                    packed.span(),
                    eco_format!(
                        "label `<{}>` does not exist in the wiki",
                        packed.target.into_inner().resolve()
                    )
                )])
            }
        }
        Some(_) if queried.len() > 0 => Err(eco_vec![SourceDiagnostic::error(
            packed.span(),
            eco_format!(
                "label `<{}>` occurs multiple times in the wiki",
                packed.target.into_inner().resolve()
            )
        )]),
        Some(ext_ref) => Ok(ext_ref.into_value()),
    }
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
    library.styles.push(Style::Recipe(Recipe::new(
        Some(Selector::Elem(Element::of::<RefElem>(), Default::default())),
        Transformation::Func(resolve_refs_externally::func()),
        Span::detached(),
    )));
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
