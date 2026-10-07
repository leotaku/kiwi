use std::{collections::HashMap, num::NonZeroUsize, sync::Arc};

use comemo::{Track as _, Tracked, TrackedMut};
use rustc_hash::FxHashMap;
use typst::{
    World,
    diag::{At as _, SourceDiagnostic, SourceResult, StrResult, Warned},
    ecow::{EcoString, EcoVec, eco_format},
    engine::Engine,
    foundations::{
        BundlePath, Bytes, Content, Label, NativeElement as _, Output, Packed, Repr as _, Selector,
        StyleChain, Target, TargetElem,
    },
    introspection::{
        DocumentPosition, ElementIntrospector, ElementIntrospectorBuilder, Introspector, Location,
        Locator, SplitLocator, TagElem,
    },
    model::{AssetElem, DocumentElem, LateLinkResolver, Numbering},
    routines::{Arenas, RealizationKind},
    syntax::{Spanned, VirtualPath},
};
use typst_html::HtmlDocument;
use typst_utils::Protected;

pub struct Wiki {
    pub entries: HashMap<VirtualPath, Bytes>,
    pub introspector: Arc<WikiIntrospector>,
}

// TODO: what happens with deferred warnings here?

pub fn compile(world: &dyn World) -> Warned<SourceResult<Wiki>> {
    let compiled = typst::compile::<WikiDocuments>(world);

    compiled.map(|result| {
        result.and_then(|wiki| {
            let mut entries = Vec::new();
            let mut all_errors = EcoVec::new();

            for (out_path, document) in wiki.documents {
                let resolver = LateLinkResolver::new(
                    Some(&out_path),
                    wiki.introspector.as_ref() as &dyn Introspector,
                );
                match typst_html::html_in_bundle(
                    document.v.root(),
                    &Default::default(),
                    resolver.track(),
                ) {
                    Ok(text) => {
                        entries.push((
                            out_path,
                            Spanned::new(Bytes::from_string(text), document.span),
                        ));
                    }
                    Err(errors) => all_errors.extend(errors),
                }
            }

            for elem in wiki
                .introspector
                .query(&Selector::Elem(AssetElem::ELEM, None))
                .into_iter()
                .filter_map(|content| content.into_packed::<AssetElem>().ok())
            {
                entries.push((
                    elem.path.as_ref().clone(),
                    Spanned::new(elem.data.0.clone(), elem.span()),
                ));
            }

            if all_errors.is_empty() {
                Ok(Wiki {
                    entries: deduplicate_entries(entries)?,
                    introspector: wiki.introspector,
                })
            } else {
                Err(all_errors)
            }
        })
    })
}

fn deduplicate_entries<T: std::cmp::PartialEq>(
    entries: Vec<(VirtualPath, Spanned<T>)>,
) -> SourceResult<HashMap<VirtualPath, T>> {
    let mut partitioned: HashMap<_, Vec<_>> = HashMap::new();
    for (path, spanned) in entries {
        partitioned.entry(path).or_default().push(spanned)
    }

    let mut deduplicated = HashMap::new();
    let mut all_errors = EcoVec::new();
    for (path, mut entries) in partitioned.drain() {
        let last_entry = entries.pop().unwrap_or_else(|| unreachable!());
        if entries.iter().all(|entry| entry.v == last_entry.v) {
            deduplicated.insert(path, last_entry.v);
        } else {
            let mut error = SourceDiagnostic::error(
                last_entry.span,
                eco_format!(
                    "path `{}` occurs multiple times in the bundle with differing outputs",
                    path.get_without_slash()
                ),
            );
            for spanned in entries {
                error = error.with_spanned_hint("path is also used here", spanned.span);
            }
            all_errors.push(error)
        }
    }

    if all_errors.is_empty() {
        Ok(deduplicated)
    } else {
        Err(all_errors)
    }
}

pub struct WikiDocuments {
    pub documents: Vec<(VirtualPath, Spanned<typst_html::HtmlDocument>)>,
    pub introspector: Arc<WikiIntrospector>,
}

impl Output for WikiDocuments {
    fn target() -> Target {
        Target::Bundle
    }

    fn create(engine: &mut Engine, content: &Content, styles: StyleChain) -> SourceResult<Self> {
        let mut locator = Locator::root().split();
        let arenas = Arenas::default();

        let styles = styles.to_map().outside();
        let styles = StyleChain::new(&styles);

        let pairs = (engine.library.routines.realize)(
            RealizationKind::Bundle,
            engine,
            &mut locator,
            &arenas,
            content,
            styles,
        )?;

        let mut documents = Vec::new();
        let mut all_anchors = HashMap::new();
        let mut introspector = ElementIntrospectorBuilder::new();

        for (content, styles) in pairs {
            if let Some(tag) = content.to_packed::<TagElem>() {
                introspector.discover_tag(&tag.tag, None);
            } else if let Some(document_elem) = content.to_packed::<DocumentElem>() {
                let (path, document, anchors) =
                    match compile_document(engine, document_elem, styles, &mut locator) {
                        Ok(ok) => ok,
                        Err(errors) => {
                            engine.sink.delayed_errors(errors);
                            continue;
                        }
                    };

                all_anchors.extend(anchors);
                introspector.discover_elements(document.introspector().elements(), |_| {
                    document_elem.location()
                });
                documents.push((
                    path.into_inner(),
                    Spanned::new(document, document_elem.span()),
                ));
            }
        }

        Ok(WikiDocuments {
            documents,
            introspector: Arc::new(WikiIntrospector {
                elements: introspector.finalize(),
                anchors: all_anchors,
            }),
        })
    }

    fn introspector(&self) -> &dyn Introspector {
        self.introspector.as_ref() as &dyn Introspector
    }
}

fn compile_document(
    engine: &mut Engine,
    document_elem: &Packed<DocumentElem>,
    styles: StyleChain<'_>,
    locator: &mut SplitLocator,
) -> SourceResult<(BundlePath, HtmlDocument, FxHashMap<Location, EcoString>)> {
    let document_location = document_elem.location().unwrap_or_else(|| unreachable!());

    let format = document_elem
        .determine_format(styles)
        .at(document_elem.span())?;
    let target = TargetElem::target.set(format.target()).wrap();
    let styles = styles.chain(&target);

    with_focused_engine(engine, document_location, |engine| {
        let mut document = typst_html::html_document_for_bundle(
            engine,
            &document_elem.body,
            locator.next(&document_elem.span()),
            styles,
        )?;

        let targets = document
            .introspector()
            .query_labelled()
            .into_iter()
            .filter_map(|content| content.location())
            .collect();
        let anchors = typst_html::create_link_anchors(&mut document, &targets);

        Ok((document_elem.path.clone(), document, anchors))
    })
}

fn with_focused_engine<F, O>(engine: &mut Engine, document_location: Location, mut f: F) -> O
where
    F: FnMut(&mut Engine) -> O,
{
    let Engine {
        world,
        library,
        introspector,
        traced,
        sink,
        route,
    } = engine;

    let focused_introspector = FocusedIntrospector {
        inner: *introspector.access("create focused engine"),
        ancestor: document_location,
    };

    let mut engine = Engine {
        world: *world,
        library,
        introspector: Protected::new((&focused_introspector as &dyn Introspector).track()),
        traced: *traced,
        sink: TrackedMut::reborrow_mut(sink),
        route: route.clone(),
    };

    f(&mut engine)
}

pub struct GlobalQueryMarker;

pub struct WikiIntrospector {
    elements: ElementIntrospector<Option<Location>>,
    anchors: HashMap<Location, EcoString>,
}

const EMPTY_STRING: &EcoString = &EcoString::inline("");

impl Introspector for WikiIntrospector {
    fn query(&self, selector: &Selector) -> EcoVec<Content> {
        self.elements.query(selector)
    }

    fn query_first(&self, selector: &Selector) -> Option<Content> {
        self.elements.query_first(selector)
    }

    fn query_unique(&self, selector: &Selector) -> StrResult<Content> {
        self.elements.query_unique(selector)
    }

    fn query_label(&self, label: Label) -> StrResult<&Content> {
        self.elements.query_label(label)
    }

    fn query_labelled(&self) -> EcoVec<Content> {
        self.elements.query_labelled()
    }

    fn query_count_before(&self, selector: &Selector, end: Location) -> usize {
        self.elements.query_count_before(selector, end)
    }

    fn label_count(&self, label: Label) -> usize {
        self.elements.label_count(label)
    }

    fn locator(&self, key: u128, base: Location) -> Option<Location> {
        self.elements.locator(key, base)
    }

    fn pages(&self, _location: Location) -> Option<NonZeroUsize> {
        None
    }

    fn page(&self, _location: Location) -> Option<NonZeroUsize> {
        None
    }

    fn position(&self, _location: Location) -> Option<DocumentPosition> {
        None
    }

    fn page_numbering(&self, _location: Location) -> Option<&Numbering> {
        None
    }

    fn page_supplement(&self, _location: Location) -> Option<&Content> {
        None
    }

    fn anchor(&self, location: Location) -> Option<&EcoString> {
        if self
            .elements
            .get_by_loc(&location)
            .and_then(|content| content.to_packed::<DocumentElem>())
            .is_some()
        {
            return Some(EMPTY_STRING);
        }
        self.anchors.get(&location)
    }

    fn document(&self, location: Location) -> Option<Location> {
        self.elements.position(location).copied()?
    }

    fn path(&self, location: Location) -> Option<&VirtualPath> {
        self.elements
            .get_by_loc(&self.document(location).unwrap_or(location))
            .and_then(|content| content.to_packed::<DocumentElem>())
            .map(|document| document.path.as_ref())
    }
}

struct FocusedIntrospector<'t> {
    inner: Tracked<'t, dyn Introspector + 't>,
    ancestor: Location,
}

impl<'a> FocusedIntrospector<'a> {
    fn adapt_selector(&self, selector: &Selector) -> Selector {
        match selector {
            Selector::Within { selector, ancestor }
                if ancestor.as_ref() == &Selector::can::<GlobalQueryMarker>() =>
            {
                (**selector).clone()
            }
            _ => selector.clone().within(self.ancestor.into()),
        }
    }
}

#[comemo::track]
impl<'a> Introspector for FocusedIntrospector<'a> {
    fn query(&self, selector: &Selector) -> EcoVec<Content> {
        self.inner.query(&self.adapt_selector(selector))
    }

    fn query_first(&self, selector: &Selector) -> Option<Content> {
        self.inner.query_first(&self.adapt_selector(selector))
    }

    fn query_unique(&self, selector: &Selector) -> StrResult<Content> {
        self.inner.query_unique(&self.adapt_selector(selector))
    }

    fn query_label(&self, label: Label) -> StrResult<&Content> {
        self.inner.query_label(label).and_then(|content| {
            if content
                .location()
                .and_then(|loc| self.document(loc))
                .is_some_and(|loc| loc == self.ancestor)
            {
                Ok(content)
            } else {
                Err(eco_format!(
                    "label `{}` does not exist in the document",
                    label.repr()
                ))
            }
        })
    }

    fn query_labelled(&self) -> EcoVec<Content> {
        self.inner
            .query_labelled()
            .into_iter()
            .filter(|content| {
                content
                    .location()
                    .and_then(|loc| self.document(loc))
                    .is_some_and(|loc| loc == self.ancestor)
            })
            .collect()
    }

    fn query_count_before(&self, selector: &Selector, end: Location) -> usize {
        self.inner
            .query_count_before(&self.adapt_selector(selector), end)
    }

    fn label_count(&self, label: Label) -> usize {
        self.query(&self.adapt_selector(&Selector::Label(label)))
            .len()
    }

    fn locator(&self, key: u128, base: Location) -> Option<Location> {
        self.inner.locator(key, base)
    }

    fn pages(&self, location: Location) -> Option<NonZeroUsize> {
        self.inner.pages(location)
    }

    fn page(&self, location: Location) -> Option<NonZeroUsize> {
        self.inner.page(location)
    }

    fn position(&self, location: Location) -> Option<DocumentPosition> {
        self.inner.position(location)
    }

    fn page_numbering(&self, location: Location) -> Option<&Numbering> {
        self.inner.page_numbering(location)
    }

    fn page_supplement(&self, location: Location) -> Option<&Content> {
        self.inner.page_supplement(location)
    }

    fn anchor(&self, location: Location) -> Option<&EcoString> {
        self.inner.anchor(location)
    }

    fn document(&self, location: Location) -> Option<Location> {
        self.inner.document(location)
    }

    fn path(&self, location: Location) -> Option<&VirtualPath> {
        self.inner.path(location)
    }
}
