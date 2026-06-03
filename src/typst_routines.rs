use rustc_hash::{FxBuildHasher, FxHashMap};
use typst::{
    Features, Library, LibraryExt as _,
    diag::{SourceDiagnostic, Warned},
    ecow::{EcoString, EcoVec, eco_format, eco_vec},
    engine::Engine,
    foundations::{
        Arg, Args, Content, Dynamic, Element, IntoValue, NativeElement, NativeFunc as _, Packed,
        Recipe, Repr, Selector, Style, Transformation, Value,
    },
    introspection::{Introspector as _, Location, QueryIntrospection},
    model::RefElem,
    syntax::{Span, Spanned, VirtualPath},
    text::TextElem,
    utils::{LazyHash, ManuallyHash, hash128},
};
use typst_html::{HtmlAttr, HtmlElem, HtmlTag};

use crate::typst_world::{GlobalContext, TemporaryWorld};

#[derive(Clone, Debug, PartialEq)]
struct Page {
    anchors: FxHashMap<Location, EcoString>,
    document: ManuallyHash<typst_html::HtmlDocument>,
}

#[typst_macros::elem]
pub struct ExtRefElem {}

#[typst_macros::ty]
#[derive(Clone, Debug, PartialEq)]
pub struct Wiki(FxHashMap<VirtualPath, Page>);

impl std::hash::Hash for Wiki {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for (path, page) in self.0.iter() {
            path.hash(state);
            page.document.root().hash(state);
        }
    }
}

impl Wiki {
    fn empty() -> Self {
        Self(FxHashMap::with_hasher(FxBuildHasher))
    }

    fn query(&self, selector: &Selector) -> EcoVec<(VirtualPath, Content)> {
        let mut results = EcoVec::new();
        for (path, page) in self.0.iter() {
            results.extend(
                page.document
                    .introspector()
                    .query(selector)
                    .into_iter()
                    .map(|content| (path.clone(), content)),
            )
        }

        results
    }
}

impl Repr for Wiki {
    fn repr(&self) -> typst::ecow::EcoString {
        "external".into()
    }
}

#[typst_macros::func]
fn ignore_refs(_body: Content) -> Result<Value, typst::ecow::EcoVec<SourceDiagnostic>> {
    Ok(Value::None)
}

#[typst_macros::func]
fn resolve_refs_externally(
    engine: &mut Engine,
    #[named]
    #[default(Wiki::empty())]
    wiki: Wiki,
    body: Content,
) -> Result<Value, typst::ecow::EcoVec<SourceDiagnostic>> {
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

    // TODO: ExtRefElem

    let mut inter_doc_labeled = wiki.query(&Selector::Label(packed.target));
    let (target_path, target_content) = match inter_doc_labeled.pop() {
        None => {
            return Err(eco_vec![SourceDiagnostic::error(packed.span(), ":(")]);
        }
        Some(_) if inter_doc_labeled.len() > 0 => {
            return Err(eco_vec![SourceDiagnostic::error(packed.span(), ":(")]);
        }
        Some(queried) => queried,
    };
    let target_page = &wiki.0[&target_path];

    let target_link = target_content
        .location()
        .and_then(|loc| target_page.anchors.get(&loc))
        .map(|id| eco_format!("{}#{}", target_path.get_with_slash(), id))
        .unwrap_or_else(|| target_path.into_with_slash());

    Ok(HtmlElem::new(HtmlTag::constant("a"))
        .with_attr(HtmlAttr::constant("href"), target_link)
        .with_body(Some(TextElem::packed("TODO")))
        .pack()
        .into_value())
}

pub fn collect_labels(
    context: &GlobalContext,
    paths: impl IntoIterator<Item = VirtualPath>,
) -> (Wiki, EcoVec<SourceDiagnostic>) {
    let mut pages = FxHashMap::with_hasher(FxBuildHasher);
    let mut errors = EcoVec::new();

    let mut library = Library::builder().with_features(Features::all()).build();
    library.styles.push(Style::Recipe(Recipe::new(
        Some(Selector::Elem(Element::of::<RefElem>(), Default::default())),
        Transformation::Func(ignore_refs::func()),
        Span::detached(),
    )));
    let library = LazyHash::new(library);

    for path in paths {
        let world = TemporaryWorld {
            main: &path,
            library: &library,
            context,
        };

        // TODO: generate IDs with AnchorGenerator

        match typst::compile::<typst_html::HtmlDocument>(&world).output {
            Ok(mut document) => {
                let targets = document
                    .introspector()
                    .query_labelled()
                    .into_iter()
                    .filter_map(|content| content.location())
                    .collect();
                let anchors = typst_html::create_link_anchors(&mut document, &targets);

                let hash = hash128(document.root());
                pages.insert(
                    path.with_extension("html"),
                    Page {
                        anchors,
                        document: ManuallyHash::new(document, hash),
                    },
                );
            }
            Err(errs) => errors.extend(errs),
        };
    }

    (Wiki(pages), errors)
}

pub fn render_wiki(
    wiki: Wiki,
    context: &GlobalContext,
    paths: impl IntoIterator<Item = VirtualPath>,
) -> (
    FxHashMap<VirtualPath, typst_html::HtmlDocument>,
    EcoVec<SourceDiagnostic>,
) {
    let mut output = FxHashMap::with_hasher(FxBuildHasher);
    let mut diagnostics = EcoVec::new();

    let mut library = Library::builder().with_features(Features::all()).build();
    library.styles.push(Style::Recipe(Recipe::new(
        Some(Selector::Elem(Element::of::<RefElem>(), Default::default())),
        Transformation::Func(resolve_refs_externally::func().with(&mut Args {
            span: Span::detached(),
            items: eco_vec![Arg {
                span: Span::detached(),
                name: Some("wiki".into()),
                value: Spanned::detached(Value::Dyn(Dynamic::new(wiki))),
            }],
        })),
        Span::detached(),
    )));
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
                diagnostics.extend(warnings);

                let targets = document
                    .introspector()
                    .query_labelled()
                    .into_iter()
                    .filter_map(|content| content.location())
                    .collect();
                typst_html::create_link_anchors(&mut document, &targets);

                output.insert(path, document);
            }
            Warned {
                output: Err(errors),
                warnings,
            } => {
                diagnostics.extend(warnings);
                diagnostics.extend(errors);
            }
        };
    }

    (output, diagnostics)
}
