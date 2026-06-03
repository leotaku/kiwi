use std::{any::Any, collections::HashMap, ops::Deref};

use typst::{
    Features, Library, LibraryExt as _,
    diag::{SourceDiagnostic, Warned},
    ecow::{EcoVec, eco_vec},
    engine::Engine,
    foundations::{
        Arg, Args, Content, Dynamic, Element, IntoValue, NativeElement, NativeFunc as _, Packed,
        Recipe, Repr, Selector, Style, Transformation, Value,
    },
    introspection::{Introspector as _, QueryIntrospection},
    model::RefElem,
    syntax::{Span, Spanned, VirtualPath},
    text::TextElem,
    utils::{LazyHash, ManuallyHash, hash128},
};
use typst_html::{HtmlAttr, HtmlElem, HtmlTag};

use crate::typst_world::{GlobalContext, TemporaryWorld};

#[typst_macros::ty]
#[derive(Hash, Clone, Debug, PartialEq)]
struct Wiki(Vec<ManuallyHash<typst_html::HtmlDocument>>);

impl Wiki {
    fn empty() -> Self {
        Self(Vec::new())
    }

    fn new(documents: Vec<typst_html::HtmlDocument>) -> Self {
        let hashed = documents
            .into_iter()
            .map(|doc| {
                let hash = hash128(doc.root());
                ManuallyHash::new(doc, hash)
            })
            .collect();
        Self(hashed)
    }

    fn query(&self, selector: &Selector) -> EcoVec<Content> {
        let mut results = EcoVec::new();
        for document in self.0.iter() {
            results.extend(document.introspector().query(selector));
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
    let intra_doc_labels = engine.introspect(QueryIntrospection(
        Selector::Label(packed.target),
        Span::detached(),
    ));
    if intra_doc_labels.len() > 0 {
        return Ok(packed.pack().into_value());
    }

    let inter_doc_labels = wiki.query(&Selector::Label(packed.target));
    match inter_doc_labels.len() {
        0 => Err(eco_vec![SourceDiagnostic::error(packed.span(), ":(")]),
        1 => Ok(HtmlElem::new(HtmlTag::constant("a"))
            .with_attr(HtmlAttr::constant("href"), "penis")
            .with_body(Some(TextElem::packed("penis")))
            .pack()
            .into_value()),
        _ => Err(eco_vec![SourceDiagnostic::error(packed.span(), ":(")]),
    }
}

pub fn collect_labels(
    context: &GlobalContext,
    paths: impl IntoIterator<Item = VirtualPath>,
) -> (Vec<Content>, EcoVec<SourceDiagnostic>) {
    let mut content = Vec::new();
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
            Ok(document) => content.extend(document.introspector().query_labelled().into_iter()),
            Err(errs) => errors.extend(errs),
        };
    }

    (content, errors)
}

pub fn render_wiki(
    labeled_content: Vec<Content>,
    context: &GlobalContext,
    paths: impl IntoIterator<Item = VirtualPath>,
) -> (
    HashMap<VirtualPath, typst_html::HtmlDocument>,
    EcoVec<SourceDiagnostic>,
) {
    let mut output = HashMap::new();
    let mut diagnostics = EcoVec::new();

    let mut library = Library::builder().with_features(Features::all()).build();
    library.styles.push(Style::Recipe(Recipe::new(
        Some(Selector::Elem(Element::of::<RefElem>(), Default::default())),
        Transformation::Func(resolve_refs_externally::func().with(&mut Args {
            span: Span::detached(),
            items: eco_vec![Arg {
                span: Span::detached(),
                name: Some("wiki".into()),
                value: Spanned::detached(Value::Dyn(Dynamic::new(Wiki::empty()))),
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
