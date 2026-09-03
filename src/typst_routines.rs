use std::sync::Arc;

use rustc_hash::FxHashMap;
use typst::{
    Features, Library, LibraryExt as _, World,
    diag::{HintedString, Severity, SourceDiagnostic, Warned},
    ecow::{EcoString, EcoVec, eco_format},
    engine::Engine,
    foundations::{
        BundlePath, Bytes, Content, Dict, IntoValue as _, Label, NativeElement as _, PathOrStr,
        Recipe, Repr, Selector, Transformation, Value,
    },
    introspection::{Introspector as _, Location},
    model::{AssetData, AssetElem},
    syntax::{Span, Spanned, VirtualPath},
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
        relative_from_parent(&output_path, engine.world.main().vpath())
    }
}

impl Repr for Page {
    fn repr(&self) -> EcoString {
        eco_format!("page({:?})", self.path)
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
    fn query_label(&self, label: Label) -> Result<Value, EcoString> {
        let mut queried = self.query(Selector::Label(label));
        match queried.pop() {
            None => {
                if self.is_incomplete() {
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

    #[func]
    fn make_relative(&self, engine: &Engine, path: PathOrStr) -> Result<EcoString, HintedString> {
        let main = engine.world.main();

        let resolved = path.resolve(main)?;
        if resolved.root() != main.root() {
            return Err("relativizing paths from outside the main root is not supported".into());
        }

        Ok(relative_from_parent(&resolved.vpath(), main.vpath()))
    }

    #[func]
    fn main(&self, engine: &Engine) -> EcoString {
        engine.world.main().vpath().get_with_slash().into()
    }

    #[func]
    fn read_asset(
        &mut self,
        engine: &Engine,
        path: Spanned<PathOrStr>,
        #[default] anchor: Option<Content>,
    ) -> Result<Value, HintedString> {
        let anchor_span = anchor
            .map(|content| content.span())
            .unwrap_or_else(|| path.span);
        let resolved = path
            .v
            .resolve(anchor_span.id().ok_or("the containing file is unknown")?)?;
        if resolved.root() != engine.world.main().root() {
            return Err("including resources from outside the main root is not supported".into());
        }

        let content = engine.world.file(resolved.clone().intern())?;

        Ok(AssetElem::new(
            BundlePath::new(resolved.vpath().clone())?,
            AssetData(content),
        )
        .into_value())
    }
}

impl Repr for Wiki {
    fn repr(&self) -> EcoString {
        "wiki".into()
    }
}

// #[func]
// fn document_to_asset(
//     engine: &mut Engine,
//     context: Tracked<Context>,
//     content: Content,
// ) -> Result<Value, EcoVec<SourceDiagnostic>> {
//     let document = content
//         .into_packed::<DocumentElem>()
//         .unwrap_or_else(|_| unreachable!());
//     let styles = context
//         .styles()
//         .unwrap_or_else(|_| unreachable!())
//         .to_map()
//         .outside();

//     let html_document = typst_html::html_document_for_bundle(
//         engine,
//         &document.body,
//         Locator::root(),
//         StyleChain::new(&styles),
//     )?;
//     let html_data = typst_html::html(&html_document, &Default::default())?;

//     let mut assets: Vec<Content> = html_document
//         .introspector()
//         .query(&Selector::Elem(AssetElem::ELEM, None))
//         .into_iter()
//         .collect();
//     assets.push(
//         AssetElem::new(
//             document.path.clone(),
//             AssetData(Bytes::from_string(html_data)),
//         )
//         .pack(),
//     );

//     Ok(SequenceElem::new(assets).into_value())
// }

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
    library.styles.push(Recipe::new(
        Some(AssetElem::ELEM.select()),
        Transformation::Content(Content::empty()),
        Span::detached(),
    ));
    // library.styles.push(Recipe::new(
    //     Some(DocumentElem::ELEM.select()),
    //     Transformation::Func(document_to_asset::func()),
    //     Span::detached(),
    // ));
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
                match document.root().children[1].clone() {
                    typst_html::HtmlNode::Tag(tag) => todo!(),
                    typst_html::HtmlNode::Text(eco_string, span) => todo!(),
                    typst_html::HtmlNode::Element(html_element) => {
                        dbg!(html_element.tag, html_element.children.len());
                    }
                    typst_html::HtmlNode::Frame(html_frame) => todo!(),
                }

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

pub fn collect_assets(wiki: &Wiki) -> Result<Vec<(VirtualPath, Bytes)>, Vec<SourceDiagnostic>> {
    let mut potential_outputs = FxHashMap::default();

    for (_, document) in wiki.pages() {
        for (span, asset) in document
            .introspector()
            .query(&Selector::Elem(AssetElem::ELEM, None))
            .into_iter()
            .filter_map(|content| content.into_packed::<AssetElem>().ok())
            .map(|packed| (packed.span(), packed.unpack()))
        {
            potential_outputs
                .entry(asset.path.into_inner())
                .or_insert_with(Vec::new)
                .push((span, asset.data));
        }
    }

    let mut outputs = Vec::new();
    let mut errors = Vec::new();
    for (path, mut candidates) in potential_outputs.drain() {
        let (first_span, first_data) = candidates.pop().unwrap_or_else(|| unreachable!());
        let conflict_messages: EcoVec<_> = candidates
            .into_iter()
            .filter(|(_, data)| *data != first_data)
            .map(|(span, _)| Spanned::new("conflicting asset".into(), span.into()))
            .collect();

        if !conflict_messages.is_empty() {
            errors.push(SourceDiagnostic {
                severity: Severity::Error,
                span: first_span.into(),
                message: eco_format!(
                    r#"multiple conflicting assets for output "{}""#,
                    path.get_with_slash()
                ),
                trace: Default::default(),
                hints: conflict_messages,
            });
        } else {
            outputs.push((path, first_data.0))
        }
    }

    if errors.is_empty() {
        Ok(outputs)
    } else {
        Err(errors)
    }
}

fn relative_from_parent(path: &VirtualPath, base: &VirtualPath) -> EcoString {
    let relative = base
        .parent()
        .map_or_else(|| unreachable!(), |parent| path.relative_from(&parent));

    if relative == "" && path != base {
        ".".into()
    } else {
        relative
    }
}
